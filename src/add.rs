use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use clap::Parser;
use colored::Colorize;
use etcetera::base_strategy::{BaseStrategy, choose_base_strategy};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use rootcause::{Result, report};
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(
    name = "nix-package-add",
    version,
    about = "Create Nix package files, preferring pre-built GitHub release binaries when available",
    long_about = r#"Create Nix package files from URLs.

For GitHub repositories, nix-package-add checks the latest release for pre-built
archives matching Nix platforms and emits a stdenv.mkDerivation with per-platform
fetchurl hashes. Non-GitHub URLs, GitHub repositories without releases, and
repositories without suitable binary assets are passed through to nix-init.

Examples:
  nix-package-add https://github.com/rtk-ai/icm
  nix-package-add https://github.com/Dicklesworthstone/pi_agent_rust --pname pi-agent-rust --binary pi
  nix-package-add https://github.com/owner/repo ./packages/repo.nix
  nix-package-add https://crates.io/crates/ripgrep -- --builder buildRustPackage"#
)]
struct Args {
    /// Package homepage/repository URL.
    url: String,

    /// Output file or directory. Defaults to ~/.config/nix/packages/<pname>.nix.
    output: Option<PathBuf>,

    /// Extra arguments passed to nix-init when falling back. Put these after `--`.
    #[arg(last = true)]
    passthrough: Vec<String>,

    /// Override package name.
    #[arg(long)]
    pname: Option<String>,

    /// Installed binary name. Defaults to pname.
    #[arg(long)]
    binary: Option<String>,

    /// Override package version.
    #[arg(long = "package-version")]
    package_version: Option<String>,

    /// Package description for meta.description.
    #[arg(long)]
    description: Option<String>,

    /// Nixpkgs license attribute, e.g. mit, asl20, unfree.
    #[arg(long, default_value = "unfree")]
    license: String,

    /// Overwrite an existing output file.
    #[arg(short, long)]
    force: bool,

    /// Print generated package without writing it.
    #[arg(long)]
    dry_run: bool,

    /// Always delegate to nix-init instead of generating a binary package.
    #[arg(long)]
    nix_init: bool,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct PrefetchResult {
    hash: String,
}

#[derive(Debug)]
struct GitHubRepo {
    owner: String,
    repo: String,
}

#[derive(Debug)]
struct PlatformAsset {
    system: &'static str,
    suffix: String,
    url: String,
    hash: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let repo = parse_github_url(&args.url);
    let pname = args
        .pname
        .clone()
        .or_else(|| repo.as_ref().map(|repo| repo.repo.clone()))
        .unwrap_or_else(|| guess_pname(&args.url));
    let output = output_path(args.output.clone(), &pname);

    if args.nix_init {
        return run_nix_init(&args, &output, &pname);
    }

    if let Some(repo) = repo {
        return match generate_binary_package(&args, &repo, &pname) {
            Ok(Some(package)) => write_or_print(&args, &output, &package),
            Ok(None) => {
                eprintln!("{}", "No matching GitHub release binaries found; falling back to nix-init".yellow());
                run_nix_init(&args, &output, &pname)
            }
            Err(e) => {
                eprintln!("{} {e}", "Failed to inspect GitHub release; falling back to nix-init:".yellow());
                run_nix_init(&args, &output, &pname)
            }
        };
    }

    eprintln!("{}", "Non-GitHub URL; passing through to nix-init".yellow());
    run_nix_init(&args, &output, &pname)
}

fn parse_github_url(url: &str) -> Option<GitHubRepo> {
    let (_, without_scheme) = url.trim_end_matches('/').trim_end_matches(".git").split_once("github.com/")?;
    let mut parts = without_scheme.split('/');
    let owner = parts.next().filter(|part| !part.is_empty())?;
    let repo = parts.next().filter(|part| !part.is_empty())?;

    Some(GitHubRepo {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

fn guess_pname(url: &str) -> String {
    url.trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit(['/', ':'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or("package")
        .to_string()
}

fn output_path(output: Option<PathBuf>, pname: &str) -> PathBuf {
    if let Some(path) = output {
        if path.extension().is_some() {
            return path;
        }
        return path.join(format!("{pname}.nix"));
    }

    let strategy = choose_base_strategy().expect("Unable to find base strategy");
    strategy.config_dir().join("nix").join("packages").join(format!("{pname}.nix"))
}

fn github_client() -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("nix-package-add"));
    headers.insert(ACCEPT, HeaderValue::from_static("application/vnd.github+json"));

    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {token}"))?);
    }

    Ok(Client::builder().default_headers(headers).build()?)
}

fn latest_release(client: &Client, repo: &GitHubRepo) -> Result<GitHubRelease> {
    let url = format!("https://api.github.com/repos/{}/{}/releases/latest", repo.owner, repo.repo);
    Ok(client.get(url).send()?.error_for_status()?.json()?)
}

fn generate_binary_package(args: &Args, repo: &GitHubRepo, pname: &str) -> Result<Option<String>> {
    let client = github_client()?;
    let release = latest_release(&client, repo)?;
    let version = args.package_version.clone().unwrap_or_else(|| version_from_tag(&release.tag_name, pname));
    let mut platforms = BTreeMap::<&'static str, PlatformAsset>::new();

    for asset in release.assets.iter().filter(|asset| is_archive(&asset.name)) {
        if let Some((system, suffix)) = platform_suffix(&asset.name) {
            let hash = prefetch_hash(&asset.browser_download_url)?;
            platforms.entry(system).or_insert(PlatformAsset {
                system,
                suffix,
                url: asset.browser_download_url.clone(),
                hash,
            });
        }
    }

    if platforms.is_empty() {
        return Ok(None);
    }

    let binary = args.binary.as_deref().unwrap_or(pname);
    let description = args.description.as_deref().unwrap_or("TODO: add description");
    let homepage = format!("https://github.com/{}/{}", repo.owner, repo.repo);
    let first_url = &platforms.values().next().expect("platforms is not empty").url;
    let url_template = url_template(first_url, &release.tag_name, &version, pname);
    let platform_text = platforms
        .values()
        .map(|asset| format!("    {} = {{\n      suffix = \"{}\";\n      hash = \"{}\";\n    }};", asset.system, asset.suffix, asset.hash))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(Some(format!(
        r#"{{
  lib,
  pkgs,
  stdenv,
}}: let
  packages = {{
{platform_text}
  }};
  source =
    packages.${{stdenv.hostPlatform.system}}
      or (throw "Unsupported system: ${{stdenv.hostPlatform.system}}");
in
  pkgs.stdenv.mkDerivation rec {{
    pname = "{pname}";
    version = "{version}";

    src = pkgs.fetchurl {{
      url = "{url_template}";
      inherit (source) hash;
    }};

    dontConfigure = true;
    dontBuild = true;
    dontStrip = true;
    sourceRoot = ".";

    installPhase = ''
      runHook preInstall

      install -m755 -D {binary} $out/bin/{binary}

      runHook postInstall
    '';

    meta = {{
      description = "{description}";
      homepage = "{homepage}";
      license = lib.licenses.{license};
      mainProgram = "{binary}";
    }};
  }}
}}
"#,
        license = args.license
    )))
}

fn version_from_tag(tag: &str, pname: &str) -> String {
    tag.strip_prefix(&format!("{pname}-v")).or_else(|| tag.strip_prefix('v')).unwrap_or(tag).to_string()
}

fn is_archive(name: &str) -> bool {
    [".tar.gz", ".tgz", ".tar.xz", ".tar.bz2", ".zip"].iter().any(|suffix| name.ends_with(suffix))
}

fn platform_suffix(name: &str) -> Option<(&'static str, String)> {
    const MATCHES: [(&str, &str); 7] = [
        ("aarch64-apple-darwin", "aarch64-darwin"),
        ("arm64-apple-darwin", "aarch64-darwin"),
        ("darwin-arm64", "aarch64-darwin"),
        ("macos-arm64", "aarch64-darwin"),
        ("x86_64-unknown-linux-gnu", "x86_64-linux"),
        ("x86_64-linux", "x86_64-linux"),
        ("linux-amd64", "x86_64-linux"),
    ];

    MATCHES.iter().find_map(|(needle, system)| name.contains(needle).then(|| (*system, (*needle).to_string())))
}

fn prefetch_hash(url: &str) -> Result<String> {
    let output = Command::new("nix").args(["store", "prefetch-file", url, "--json"]).output()?;

    if !output.status.success() {
        return Err(report!("nix store prefetch-file failed for {url}: {}", String::from_utf8_lossy(&output.stderr)));
    }

    Ok(serde_json::from_slice::<PrefetchResult>(&output.stdout)?.hash)
}

fn url_template(url: &str, tag: &str, version: &str, pname: &str) -> String {
    let tag_template = if tag == format!("v{version}") {
        "v${version}".to_string()
    } else if tag == format!("{pname}-v{version}") {
        "${pname}-v${version}".to_string()
    } else {
        tag.replace(version, "${version}")
    };

    url.replace(&format!("/download/{tag}/"), &format!("/download/{tag_template}/"))
        .replace(version, "${version}")
        .replace(&platform_suffix(url).map_or_else(String::new, |(_, suffix)| suffix), "${source.suffix}")
}

fn write_or_print(args: &Args, output: &PathBuf, package: &str) -> Result<()> {
    if args.dry_run {
        print!("{package}");
        return Ok(());
    }

    if output.exists() && !args.force {
        return Err(report!("Output already exists: {} (use --force to overwrite)", output.display()));
    }

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(output, package)?;
    println!("{} {}", "Wrote".green(), output.display());
    Ok(())
}

fn run_nix_init(args: &Args, output: &PathBuf, pname: &str) -> Result<()> {
    if output.exists() && !args.force {
        return Err(report!("Output already exists: {} (use --force to overwrite)", output.display()));
    }

    if args.dry_run {
        let mut command = vec!["nix-init".to_string(), "--url".to_string(), args.url.clone(), "--headless".to_string()];
        if args.pname.is_some() {
            command.extend(["--pname".to_string(), pname.to_string()]);
        }
        if args.force {
            command.push("--overwrite".to_string());
        }
        if let Some(version) = &args.package_version {
            command.extend(["--version".to_string(), version.clone()]);
        }
        command.extend(args.passthrough.clone());
        command.push(output.display().to_string());
        println!("{}", command.join(" "));
        return Ok(());
    }

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut command = Command::new("nix-init");
    command.args(["--url", &args.url, "--headless"]);

    if args.pname.is_some() {
        command.args(["--pname", pname]);
    }

    if args.force {
        command.arg("--overwrite");
    }

    if let Some(version) = &args.package_version {
        command.args(["--version", version]);
    }

    command.args(&args.passthrough);

    let status = command.arg(output).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(report!("nix-init failed with status: {status}"))
    }
}
