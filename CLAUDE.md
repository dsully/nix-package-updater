# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Rust CLI tool that automatically updates Nix package definitions from multiple sources (PyPI, GitHub releases, Cargo, npm, Go, Git). It discovers Nix package files, checks for updates, applies changes to the Nix AST, builds packages to verify updates, and optionally pushes to cachix.

## Build & Development Commands

```bash
nix build                  # Build (creates ./result/bin/nix-package-updater)
nix build .#clippy         # Build with clippy checks (--deny warnings)
nix develop                # Development shell (cargo, rustc, clippy, rustfmt)
```

The flake uses crane with a separated `buildDepsOnly`/`buildPackage` strategy for Nix store caching. There are currently no unit or integration tests.

## Usage

```bash
./result/bin/nix-package-updater                    # Update all packages
./result/bin/nix-package-updater package1 package2  # Update specific packages
./result/bin/nix-package-updater --type pypi         # Filter by package type
./result/bin/nix-package-updater --build-only        # Build only, skip updates
./result/bin/nix-package-updater --force             # Force update even if up to date
./result/bin/nix-package-updater --cache             # Push builds to cachix
./result/bin/nix-package-updater --dry-run           # Show what would be updated
```

## Architecture

### Module Structure

- **`main.rs`** — Entry point, CLI parsing (clap), config loading (figment), package discovery, parallel processing with rayon, progress bars with indicatif
- **`package.rs`** — `Package` struct (name, path, kind, homepage, version, hash, AST, results) and `Package::discover()` which walks directories to find .nix files
- **`nix/ast.rs`** — AST manipulation using rnix. `Ast` wraps parsed Nix code; key methods: `get()`, `set()`, `platforms()`, `update_git()`, `update_vendor()`
- **`nix/builder.rs`** — Builds packages with `nix build`, writes logs to `build-results/`, pushes to cachix
- **`updater/`** — Trait-based updater system with implementations per package source:
  - `pypi.rs` — PyPI packages (handles platform-specific wheels)
  - `github.rs` — GitHub release-based packages
  - `cargo.rs` — Rust crates (both fetchCrate and git-based)
  - `npm.rs` — npm packages (downloads package-lock.json, updates npmDepsHash)
  - `go.rs` — Go modules (buildGoModule, updates vendorHash)
  - `git.rs` — Generic git repository fallback
- **`clients/`** — HTTP clients: `pypi.rs`, `github.rs` (octocrab with tokio async-to-sync wrapper), `crates.rs`, `npm.rs`, `nix.rs` (CLI wrapper for nix/nurl commands)

### Updater Trait

The core extension point — each package type implements:

```rust
pub trait Updater: Sized {
    fn new(config: &Config) -> Result<Self>;
    fn update(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()>;
    fn should_skip_update(&self, force: bool, current: &str, latest: &str) -> bool;
}
```

### Package Type Detection

`Package::detect_package_kind()` checks the Nix AST for function calls:
- **PyPi**: `fetchPypi`
- **Cargo**: `rustPlatform.buildRustPackage`
- **Npm**: `buildNpmPackage`
- **Go**: `buildGoModule`
- **GitHub**: content heuristic — "github.com" + "releases" + "download"
- **Git**: default fallback

### Update Flow

1. **Discovery** — Walk `packages/` and `nix/packages/` directories, parse Nix files, extract metadata (pname, version, hash, homepage)
2. **Parallel Updates** — rayon `par_iter_mut()` processes packages concurrently, each with its own ProgressBar
3. **Version Check** — Query external API for latest version
4. **AST Updates** — `Ast::set()` updates values by finding AST nodes and replacing text ranges, then re-parses to keep the tree in sync
5. **Build Verification** — `nix build .#{name} --no-link`, logs to `build-results/`
6. **Cachix Push** — Optional push using `whoami::username()`

### Key Patterns

- **AST text-range mutation**: rnix parses Nix into a syntax tree; `Ast` maintains both the text (`String`) and tree (`Parse<Root>`). Updates replace text at exact ranges then re-parse, preserving formatting and comments. Strings with interpolation (`${...}`) are skipped.
- **Vendor hash discovery**: For Go/Cargo packages, the updater clears the vendor hash to an empty string, runs `nix build`, and parses the expected hash from stderr ("got: ...") to get the correct value.
- **Platform-specific hashes**: `ast.platforms()` extracts `platformData`/`dists` attribute sets. Each platform's hash is fetched via `Nix::prefetch_hash()` and updated individually.

## Configuration

Uses figment to merge config from multiple sources (in priority order):
1. Command-line arguments (clap)
2. `~/.config/nix-updater/config.toml` (optional)
3. Environment variables prefixed with `NIX_UPDATER_`

## Implementation Details

- Package names are hyperlinked in terminal output using OSC-8 escape sequences
- Git hashes shortened to 8 characters for display via `short_hash()`
- Version comparison uses semver with fallback to string comparison
- Package files must have `pname`, `version`, `hash`, and `homepage` attributes
- Clippy pedantic/perf/correctness all denied in Cargo.toml
