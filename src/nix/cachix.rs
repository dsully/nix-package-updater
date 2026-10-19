use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use etcetera::base_strategy::{BaseStrategy, choose_base_strategy};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use md5::Md5;
use nix_archive::nar::{CaseHack, encode_path};
use rayon::prelude::*;
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::{CONTENT_TYPE, ETAG};
use rootcause::{Result, report};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const API: &str = "https://cachix.org/api/v1/cache";
const STORE_DIR: &str = "/nix/store/";
const CHUNK_SIZE: usize = 32 * 1024 * 1024;
const ZSTD_LEVEL: i32 = 9;
const ATTEMPTS: u32 = 3;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathInfo {
    path: String,
    nar_hash: String,
    nar_size: u64,
    references: Vec<String>,
    deriver: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatedNar {
    nar_id: String,
    upload_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadUrl {
    upload_url: String,
}

struct Cachix {
    client: Client,
    token: String,
    base: String,
}

/// Push the closures of `paths` (store paths or installables) to `cache`; returns the number of paths uploaded.
/// Each upload gets a progress bar in `progress`.
pub fn push(cache: &str, paths: &[String], progress: &MultiProgress) -> Result<usize> {
    let cachix = Cachix {
        client: Client::new(),
        token: auth_token()?,
        base: format!("{API}/{cache}"),
    };

    let closure = query_closure(paths)?;
    let missing = cachix.missing(&closure)?;

    tracing::info!("Pushing {} paths ({} already present) to {cache}", missing.len(), closure.len() - missing.len());

    missing.par_iter().try_for_each(|info| cachix.upload(info, progress))?;

    Ok(missing.len())
}

fn auth_token() -> Result<String> {
    if let Ok(token) = std::env::var("CACHIX_AUTH_TOKEN") {
        return Ok(token);
    }

    let path = choose_base_strategy()?.config_dir().join("cachix/auth-token");

    match std::fs::read_to_string(&path) {
        Ok(token) => Ok(token.trim().to_owned()),
        Err(e) => Err(report!("CACHIX_AUTH_TOKEN is unset and {} is unreadable: {e}", path.display())),
    }
}

fn query_closure(paths: &[String]) -> Result<Vec<PathInfo>> {
    let output = Command::new("nix").args(["path-info", "--json", "--recursive"]).args(paths).output()?;

    if !output.status.success() {
        return Err(report!("nix path-info failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    // Lix and Nix < 2.19 print an array; newer Nix prints an object keyed by store path.
    let infos = match serde_json::from_slice(&output.stdout)? {
        Value::Array(infos) => infos,
        Value::Object(map) => map
            .into_iter()
            .map(|(path, mut info)| {
                if let Some(fields) = info.as_object_mut() {
                    fields.insert("path".into(), path.into());
                }
                info
            })
            .collect(),
        other => return Err(report!("Unexpected nix path-info output: {other}")),
    };

    infos.into_iter().map(|info| Ok(serde_json::from_value(info)?)).collect()
}

impl Cachix {
    fn post(&self, url: &str) -> RequestBuilder {
        self.client.post(url).bearer_auth(&self.token)
    }

    fn missing<'a>(&self, closure: &'a [PathInfo]) -> Result<Vec<&'a PathInfo>> {
        let hashes: Vec<&str> = closure.iter().map(|info| store_hash(&info.path)).collect();
        let mut missing = HashSet::new();

        // The server limits request bodies to 1 MB.
        for chunk in hashes.chunks(32_768) {
            let found: Vec<String> = check(self.post(&format!("{}/narinfo", self.base)).json(chunk).send()?)?.json()?;
            missing.extend(found);
        }

        Ok(closure.iter().filter(|info| missing.contains(store_hash(&info.path))).collect())
    }

    fn upload(&self, info: &PathInfo, progress: &MultiProgress) -> Result<()> {
        let style = ProgressStyle::with_template("{bar:30.blue} {bytes:>10}/{total_bytes:<10} {bytes_per_sec:>12} {msg}").unwrap_or_else(|_| ProgressStyle::default_bar());
        let bar = progress.add(ProgressBar::new(info.nar_size).with_style(style).with_message(base_name(&info.path).to_owned()));
        let mut attempt = 1;

        loop {
            bar.set_position(0);

            match self.upload_once(info, &bar) {
                Err(e) if attempt < ATTEMPTS => {
                    tracing::warn!("Retry #{attempt} for {}: {e}", info.path);
                    attempt += 1;
                }
                result => {
                    if result.is_ok() {
                        bar.finish();
                    } else {
                        bar.abandon();
                    }
                    return result;
                }
            }
        }
    }

    fn upload_once(&self, info: &PathInfo, bar: &ProgressBar) -> Result<()> {
        let created: CreatedNar = check(self.post(&format!("{}/multipart-nar?compression=zst", self.base)).send()?)?.json()?;

        let nar_url = format!("{}/multipart-nar/{}", self.base, created.nar_id);
        let result = self.upload_nar(info, &nar_url, &created.upload_id, bar);

        if result.is_err() {
            let _ = self.post(&format!("{nar_url}/abort")).query(&[("uploadId", &created.upload_id)]).send();
        }

        result
    }

    fn upload_nar(&self, info: &PathInfo, nar_url: &str, upload_id: &str, bar: &ProgressBar) -> Result<()> {
        let parts = Parts {
            cachix: self,
            nar_url,
            upload_id,
            buffer: Vec::new(),
            completed: Vec::new(),
            file_hash: Sha256::new(),
            file_size: 0,
        };

        let mut nar = NarWriter {
            inner: zstd::Encoder::new(parts, ZSTD_LEVEL)?,
            hash: Sha256::new(),
            size: 0,
            bar,
        };

        encode_path(&mut nar, Path::new(&info.path), CaseHack::native())?;

        let mut parts = nar.inner.finish()?;

        if !parts.buffer.is_empty() {
            let last = std::mem::take(&mut parts.buffer);
            parts.upload_part(last)?;
        }

        let nar_hash = nar.hash.finalize();
        let expected = info.nar_hash.strip_prefix("sha256-").map(|b64| STANDARD.decode(b64)).transpose()?;

        if expected.as_deref() != Some(nar_hash.as_slice()) {
            return Err(report!("{}: computed NAR hash does not match the store narHash {}", info.path, info.nar_hash));
        }

        let name = base_name(&info.path);
        let mut references: Vec<&str> = info.references.iter().map(|r| base_name(r)).collect();
        references.sort_unstable();

        let body = json!({
            "parts": parts.completed,
            "narInfoCreate": {
                "cStoreHash": store_hash(&info.path),
                "cStoreSuffix": name.get(33..).unwrap_or_default(),
                "cNarHash": format!("sha256:{}", nix32(&nar_hash)),
                "cNarSize": nar.size,
                "cFileHash": hex(&parts.file_hash.finalize()),
                "cFileSize": parts.file_size,
                "cReferences": references,
                "cDeriver": info.deriver.as_deref().map_or("unknown-deriver", base_name),
                "cSig": null,
            },
        });

        check(self.post(&format!("{nar_url}/complete")).query(&[("uploadId", upload_id)]).json(&body).send()?)?;

        Ok(())
    }
}

struct NarWriter<'a, W> {
    inner: W,
    hash: Sha256,
    size: u64,
    bar: &'a ProgressBar,
}

impl<W: Write> Write for NarWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        self.size += n as u64;
        self.bar.inc(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Buffers compressed NAR bytes and uploads them as S3 multipart parts.
struct Parts<'a> {
    cachix: &'a Cachix,
    nar_url: &'a str,
    upload_id: &'a str,
    buffer: Vec<u8>,
    completed: Vec<Value>,
    file_hash: Sha256,
    file_size: u64,
}

impl Parts<'_> {
    fn upload_part(&mut self, part: Vec<u8>) -> io::Result<()> {
        let part_number = self.completed.len() + 1;
        let content_md5 = STANDARD.encode(Md5::digest(&part));

        self.file_hash.update(&part);
        self.file_size += part.len() as u64;

        let signed: UploadUrl = self
            .cachix
            .post(self.nar_url)
            .query(&[("uploadId", self.upload_id)])
            .query(&[("partNumber", part_number)])
            .json(&json!({ "contentMD5": content_md5 }))
            .send()
            .map_err(io::Error::other)
            .and_then(check)?
            .json()
            .map_err(io::Error::other)?;

        let response = self
            .cachix
            .client
            .put(signed.upload_url)
            .header(CONTENT_TYPE, "application/octet-stream")
            .header("Content-MD5", content_md5)
            .body(part)
            .send()
            .map_err(io::Error::other)
            .and_then(check)?;

        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| io::Error::other("part upload returned no ETag"))?;

        self.completed.push(json!({ "partNumber": part_number, "eTag": etag }));

        Ok(())
    }
}

impl Write for Parts<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);

        while let Some(part) = take_full_part(&mut self.buffer, CHUNK_SIZE) {
            self.upload_part(part)?;
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Cloudflare R2 rejects multipart uploads whose non-final parts differ in size.
fn take_full_part(buffer: &mut Vec<u8>, size: usize) -> Option<Vec<u8>> {
    (buffer.len() >= size).then(|| {
        let rest = buffer.split_off(size);
        std::mem::replace(buffer, rest)
    })
}

/// Like `Response::error_for_status`, but keeps the body, which holds the server's reason.
fn check(response: Response) -> io::Result<Response> {
    let status = response.status();

    if status.is_success() {
        return Ok(response);
    }

    let path = response.url().path().to_owned();
    Err(io::Error::other(format!("{status} for {path}: {}", response.text().unwrap_or_default())))
}

fn base_name(path: &str) -> &str {
    path.strip_prefix(STORE_DIR).unwrap_or(path)
}

fn store_hash(path: &str) -> &str {
    let name = base_name(path);
    name.get(..32).unwrap_or(name)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Nix's base32 variant: custom alphabet, least significant bits first.
fn nix32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789abcdfghijklmnpqrsvwxyz";

    let len = (bytes.len() * 8 - 1) / 5 + 1;

    (0..len)
        .rev()
        .map(|n| {
            let (i, j) = (n * 5 / 8, n * 5 % 8);
            let low = u16::from(bytes[i]) >> j;
            let high = bytes.get(i + 1).map_or(0, |&b| u16::from(b) << (8 - j));
            char::from(ALPHABET[usize::from((low | high) & 0x1f)])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nix32_matches_nix_hash_convert() {
        assert_eq!(nix32(&Sha256::digest(b"")), "0mdqa9w1p6cmli6976v4wi0sw9r4p5prkj7lzfd1877wk11c9c73");
    }

    #[test]
    fn hex_matches_sha256sum() {
        assert_eq!(hex(&Sha256::digest(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn full_parts_are_exactly_the_chunk_size() {
        let mut buffer = b"0123456789".to_vec();
        assert_eq!(take_full_part(&mut buffer, 4).as_deref(), Some(&b"0123"[..]));
        assert_eq!(take_full_part(&mut buffer, 4).as_deref(), Some(&b"4567"[..]));
        assert_eq!(take_full_part(&mut buffer, 4), None);
        assert_eq!(buffer, b"89");
    }

    #[test]
    fn store_path_parts() {
        let path = "/nix/store/y0y7p7snbqjf0rqprxa145jlsxrqbw3d-lix-2.96.0";
        assert_eq!(store_hash(path), "y0y7p7snbqjf0rqprxa145jlsxrqbw3d");
        assert_eq!(&base_name(path)[33..], "lix-2.96.0");
    }
}
