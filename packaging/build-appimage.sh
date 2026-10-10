#!/usr/bin/env bash
# Build dist/Inkpad-x86_64.AppImage from a release build.
# Requires appimagetool on PATH or APPIMAGETOOL pointing at it.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release
APPDIR=target/AppDir
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps" \
         "$APPDIR/usr/share/icons/hicolor/512x512/apps" \
         "$APPDIR/usr/share/icons/hicolor/scalable/apps"

cp target/release/inkpad "$APPDIR/usr/bin/inkpad"
ID=io.github.50hoursaweek.inkpad
cp "packaging/$ID.desktop" "$APPDIR/$ID.desktop"
cp "packaging/$ID.desktop" "$APPDIR/usr/share/applications/$ID.desktop"
mkdir -p "$APPDIR/usr/share/metainfo"
cp "packaging/$ID.metainfo.xml" "$APPDIR/usr/share/metainfo/$ID.metainfo.xml"
cp assets/icon-256.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/$ID.png"
cp assets/icon-512.png "$APPDIR/usr/share/icons/hicolor/512x512/apps/$ID.png"
cp assets/inkpad-icon-4a.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/$ID.svg"
# Optional bundled music (assets/music, not tracked), found by the app relative to the executable.
if compgen -G "assets/music/*.*" >/dev/null; then
  mkdir -p "$APPDIR/usr/share/inkpad/music"
  cp assets/music/*.* "$APPDIR/usr/share/inkpad/music/"
fi
cp assets/icon-256.png "$APPDIR/$ID.png"
cp assets/icon-256.png "$APPDIR/.DirIcon"
ln -sf usr/bin/inkpad "$APPDIR/AppRun"

mkdir -p dist
# Build to a temporary name and rename over the old one, so packaging works
# while a previous AppImage is still running (overwriting it in place fails
# with "Text file busy").
OUT=dist/Inkpad-x86_64.AppImage
ARCH=x86_64 "${APPIMAGETOOL:-appimagetool}" --no-appstream "$APPDIR" "$OUT.tmp"
mv -f "$OUT.tmp" "$OUT"
ls -la "$OUT"
