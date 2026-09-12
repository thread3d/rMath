#!/usr/bin/env bash
# Assemble dist/rMath.app — a self-contained macOS application bundle
# containing the kernel, the notebook server, KaTeX assets and the demo.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$ROOT/dist"
APP="$DIST/rMath.app"
RES="$APP/Contents/Resources"
CARGO="${CARGO:-$(command -v cargo || echo "$HOME/.cargo/bin/cargo")}"
VERSION="0.1.0"

echo "==> building kernel (release)"
(cd "$ROOT/rmath" && "$CARGO" build --release)
KERNEL="$ROOT/rmath/target/release/rmath"
[ -x "$KERNEL" ] || { echo "kernel binary missing: $KERNEL" >&2; exit 1; }

echo "==> assembling bundle"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$RES/rmathpy/katex/dist"

cp "$KERNEL" "$RES/rmath"
chmod +x "$RES/rmath"

# Python front end (stdlib only)
cp "$ROOT"/rmathpy/*.py "$RES/rmathpy/"
cp -R "$ROOT"/rmathpy/katex/dist/katex "$RES/rmathpy/katex/dist/katex"

# Demo files (copied into the user's work dir on first launch)
mkdir -p "$RES/demo"
cp "$ROOT"/demo/demo.mt "$ROOT"/demo/data.csv "$RES/demo/"

# Launcher
cp "$ROOT/scripts/rmath-launcher.sh" "$APP/Contents/MacOS/rMath"
chmod +x "$APP/Contents/MacOS/rMath"

echo "==> icon"
python3 "$ROOT/scripts/make_icon.py" "$RES/rMath.icns" >/dev/null
[ -f "$RES/rMath.icns" ] || { echo "icon generation failed" >&2; exit 1; }

echo "==> Info.plist"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>rMath</string>
  <key>CFBundleDisplayName</key><string>rMath</string>
  <key>CFBundleIdentifier</key><string>local.rmath.app</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>rMath</string>
  <key>CFBundleIconFile</key><string>rMath</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleSignature</key><string>????</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.education</string>
</dict>
</plist>
PLIST

# Make sure LaunchServices sees the fresh bundle.
touch "$APP"

echo "==> built $APP"
du -sh "$APP" | sed 's/^/    size: /'
