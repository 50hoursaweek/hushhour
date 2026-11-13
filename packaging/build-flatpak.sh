#!/usr/bin/env bash
# Build dist/Slowpage-x86_64.flatpak, a single-file bundle of this checkout.
# Requires flatpak with the Flathub remote and org.flatpak.Builder installed; the SDK and
# the Rust extension are installed from Flathub on the first run.
# Install the result with: flatpak install --user dist/Slowpage-x86_64.flatpak
set -euo pipefail
cd "$(dirname "$0")/.."

ID=io.github._50hoursaweek.hushhour
WORK=target/flatpak
mkdir -p "$WORK" dist
# --disable-rofiles-fuse: CI runners have no FUSE.
flatpak run org.flatpak.Builder --user --install-deps-from=flathub --force-clean --disable-rofiles-fuse \
  --state-dir="$WORK/state" --repo="$WORK/repo" "$WORK/build" "packaging/$ID.yml"
# --runtime-repo lets the installer fetch the Freedesktop runtime from Flathub if missing.
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
  "$WORK/repo" dist/Slowpage-x86_64.flatpak "$ID"
ls -l dist/Slowpage-x86_64.flatpak
