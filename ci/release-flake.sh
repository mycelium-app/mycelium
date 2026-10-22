#!/bin/sh
# Assembles the release flake: nix/release/flake.nix with this release's version and checksums,
# beside the node modules and the desktop package it imports.
set -eu

version="${1#v}"
binary_sha256="$2"
desktop_sha256="$3"
out="${4:-mycelium-flake.tar.gz}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/mycelium/nix"

sed -e "s|@VERSION@|$version|" \
    -e "s|@BINARY_SHA256@|$binary_sha256|" \
    -e "s|@DESKTOP_SHA256@|$desktop_sha256|" \
    nix/release/flake.nix > "$work/mycelium/flake.nix"
cp nix/nixos.nix nix/home-manager.nix nix/options.nix nix/launcher.nix \
   nix/release/desktop.nix nix/release/mycelium.desktop "$work/mycelium/nix/"

(cd "$work/mycelium" && nix flake lock)
tar -C "$work" -czf "$out" mycelium
