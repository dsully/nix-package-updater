# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

This is a Rust CLI tool that automatically updates Nix package definitions from multiple sources (PyPI, GitHub releases, Cargo, Git repositories). It discovers Nix package files, checks for updates, applies changes to the Nix AST, builds packages to verify updates, and optionally pushes to cachix.

## Build & Development Commands

```bash
# Build the project (creates ./result/bin/nix-package-updater)
nix build

# Build with clippy checks
nix build .#clippy

# Development shell
nix develop

# Run the binary (after building)
./result/bin/nix-package-updater

# Common development tasks (requires just)
just fmt                    # Format Nix files (alejandra, deadnix, statix)
just up                     # Update flake inputs
```

## Testing & Running

```bash
# Update all packages in packages/ or nix/packages/ directories
./result/bin/nix-package-updater

# Update specific packages only
./result/bin/nix-package-updater package1 package2

# Filter by package type
./result/bin/nix-package-updater --type pypi

# Build only, skip updates
./result/bin/nix-package-updater --build-only

# Force update even if up to date
./result/bin/nix-package-updater --force

# Push successful builds to cachix (uses whoami::username())
./result/bin/nix-package-updater --cache

# Dry run (show what would be updated)
./result/bin/nix-package-updater --dry-run

# Generate shell completions
./result/bin/nix-package-updater completions bash
```

## Architecture

### Module Structure

- **`main.rs`**: Entry point, CLI parsing (clap), config loading (figment from `~/.config/nix-updater/config.toml` + env vars), package discovery, parallel processing with rayon, progress bars with indicatif
- **`package.rs`**: Core `Package` struct containing name, path, kind (PyPi/GitHub/Cargo/Git), homepage URL, version, hash, Nix AST, and update results. `Package::discover()` walks `packages/` and `nix/packages/` directories to find .nix files
- **`nix/ast.rs`**: AST manipulation using rnix parser. `Ast` struct wraps parsed Nix code and provides methods to get/set attributes while maintaining parse tree integrity. Key methods: `get()`, `set()`, `platforms()`, `update_git()`, `update_vendor()`
- **`nix/builder.rs`**: Builds packages with `nix build`, writes logs to `build-results/`, pushes to cachix using username from whoami
- **`updater/`**: Trait-based updater system with implementations for each package source type
- **`clients/`**: HTTP clients for external APIs (PyPI, GitHub, Nix)

### Package Type Detection

Package types are detected in `Package::discover()` by analyzing Nix AST and content:
- **PyPi**: Looks for `fetchPypi` function call
- **Cargo**: Looks for `rustPlatform.buildRustPackage` function call
- **GitHub**: Checks for "github.com", "releases", "download" in content
- **Git**: Default fallback for other packages

### Update Flow

1. **Discovery**: Walk directories, parse Nix files, extract metadata (pname, version, hash, homepage)
2. **Parallel Updates**: Use rayon to process packages concurrently
3. **Version Check**: Query external API (PyPI, GitHub, etc.) for latest version
4. **AST Updates**: Use `Ast::set()` to update version, hash, and platform-specific hashes
5. **Build Verification**: Run `nix build` to verify changes work
6. **Cachix Push**: Optionally push successful builds to user's cachix cache

### AST Manipulation

The `Ast` struct uses rnix to parse and modify Nix expressions:
- Preserves formatting and comments
- Updates values by finding exact AST nodes and replacing text ranges
- Skips strings with interpolation (`${...}`)
- Re-parses after each change to keep AST in sync
- `platforms()` method extracts platformData/dists structures for multi-platform packages

### Platform-Specific Updates

Many packages have `platformData` or `dists` attribute sets with platform-specific filenames and hashes. The updater:
1. Calls `ast.platforms()` to extract platform blocks
2. For each platform, constructs the download URL
3. Uses `Nix::prefetch_hash()` to get new hash
4. Updates each hash individually using `ast.set()`

## Configuration

Uses figment to merge config from multiple sources (in priority order):
1. Command-line arguments (clap)
2. `~/.config/nix-updater/config.toml` (optional)
3. Environment variables prefixed with `NIX_UPDATER_`

## Code Style Notes

- Extensive clippy lints configured in Cargo.toml (pedantic, perf, correctness all denied)
- Uses `edition = "2024"` Rust edition
- Parallel processing with rayon for performance
- Progress indicators for all operations
- Colored terminal output for status

## Important Implementation Details

- Package names are hyperlinked in terminal output using OSC-8 escape sequences
- Git hashes are shortened to 8 characters for display
- Build logs are written to `build-results/<package>.log`
- The tool expects package files to have `pname`, `version`, `hash`, and `homepage` attributes
- Updater trait provides `should_skip_update()` to avoid redundant updates unless `--force` is used
