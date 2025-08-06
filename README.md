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
```

## Features

- **Updates from**: PyPI, GitHub releases, Cargo, and Git packages
- **Parallel processing**: Updates and builds packages concurrently
- **Smart detection**: Automatically finds packages in your Nix files
- **Build verification**: Tests updates before committing changes
- **Cachix integration**: Push successful builds to cache
