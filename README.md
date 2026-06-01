# nix-package-updater

Automatically update Nix packages from PyPI, GitHub releases, Cargo, and Git repositories.

## Requirements

- Nix flake with package derivations in a `packages/` directory relative to your `flake.nix` file.

## Installation

```bash
nix build
```

## Usage

```command
# Update all packages
./result/bin/nix-package-updater

# Update specific packages
./result/bin/nix-package-updater --packages "package1,package2"

# Build only, skip updates
./result/bin/nix-package-updater --no-update

# Push successful builds to cachix
./result/bin/nix-package-updater --cache

# Add a package, preferring pre-built GitHub release binaries
./result/bin/nix-package-add https://github.com/rtk-ai/icm --license asl20
./result/bin/nix-package-add https://github.com/Dicklesworthstone/pi_agent_rust \
  --pname pi-agent-rust --binary pi --license mit
```

## nix-package-add

`nix-package-add` creates `~/.config/nix/packages/<pname>.nix` by default. For GitHub repositories it checks the latest release for pre-built archives matching Nix platforms, prefetches their hashes, and writes a per-platform `fetchurl` package. Non-GitHub URLs, GitHub repositories without releases, and repositories without matching binaries are passed through to `nix-init`. Extra `nix-init` arguments can be supplied after `--`.

## Features

- **Updates from**: PyPI, GitHub releases, Cargo, and Git packages
- **Parallel processing**: Updates and builds packages concurrently
- **Smart detection**: Automatically finds packages in your Nix files
- **Build verification**: Tests updates before committing changes
- **Cachix integration**: Push successful builds to cache
