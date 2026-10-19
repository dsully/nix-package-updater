#[path = "nix/cachix.rs"]
mod cachix;

use std::io::{self, IsTerminal, Write};

use clap::Parser;
use indicatif::MultiProgress;
use rootcause::{Result, report};

/// Push store paths and their closures to a Cachix binary cache.
#[derive(Parser, Debug)]
#[command(name = "nix-cache-push", version)]
struct Args {
    /// Cachix cache name.
    cache: String,

    /// Store paths or installables. Read from stdin when omitted.
    paths: Vec<String>,
}

/// Writes log lines to stderr with the progress bars hidden, so the two do not interleave.
struct SuspendingStderr(MultiProgress);

impl Write for SuspendingStderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.suspend(|| io::stderr().write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

fn main() -> Result<()> {
    let progress = MultiProgress::new();
    let writer = progress.clone();

    tracing_subscriber::fmt().with_writer(move || SuspendingStderr(writer.clone())).init();

    let args = Args::parse();
    let mut paths = args.paths;

    if paths.is_empty() && !io::stdin().is_terminal() {
        paths = io::read_to_string(io::stdin())?.split_whitespace().map(str::to_owned).collect();
    }

    if paths.is_empty() {
        return Err(report!("No store paths given on the command line or stdin"));
    }

    let pushed = cachix::push(&args.cache, &paths, &progress)?;

    eprintln!("Pushed {pushed} paths to {}", args.cache);

    Ok(())
}
