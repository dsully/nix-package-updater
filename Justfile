set shell := ["fish", "-c"]

export NIXPKGS_ALLOW_UNFREE := "1"
export NIXPKGS_ALLOW_UNSUPPORTED_SYSTEM := "1"
export NIX_CONFIG := "experimental-features = nix-command flakes"

# This list
default:
    @just --list

# Update all the flake inputs
[group('nix')]
up:
    @nix flake update

# Open a nix shell with the flake
[group('nix')]
repl:
    @nix repl -f flake:nixpkgs

# Format the nix files in this repo

alias format := fmt

[group('nix')]
fmt:
    @alejandra .
    @deadnix .
    @statix check
