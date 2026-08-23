#!/usr/bin/env bash
set -euo pipefail

# Build and package TinyShell.app locally.
#
# Usage:
#   ./scripts/package-macos-app.sh                     # small build without FreeRDP
#   ./scripts/package-macos-app.sh --edition rdp       # build with FreeRDP
#   ./scripts/package-macos-app.sh --target aarch64-apple-darwin
#   ./scripts/package-macos-app.sh --version 1.0.9 --target x86_64-apple-darwin
#   ./scripts/package-macos-app.sh --edition rdp --binary target/.../tiny-shell --skip-build
#
# The Info.plist is generated from the shared template at
# assets/macos/Info.plist so this script and release.yml stay in sync.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="tiny-shell"
EDITION="base"
DISPLAY_NAME="TinyShell"
BUNDLE_ID="dev.tiny-shell.app"
TARGET=""
VERSION=""
OUTPUT_DIR="target/release"
BINARY=""
SKIP_BUILD=false

usage() {
  echo "Usage: $0 [--edition base|rdp] [--version <ver>] [--target <triple>] [--output-dir <dir>] [--binary <path> --skip-build]" >&2
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --edition)
      [[ $# -ge 2 ]] || usage
      EDITION="$2"
      shift 2
      ;;
    --version)
      [[ $# -ge 2 ]] || usage
      VERSION="$2"
      shift 2
      ;;
    --target)
      [[ $# -ge 2 ]] || usage
      TARGET="$2"
      shift 2
      ;;
    --output-dir)
      [[ $# -ge 2 ]] || usage
      OUTPUT_DIR="$2"
      shift 2
      ;;
    --binary)
      [[ $# -ge 2 ]] || usage
      BINARY="$2"
      shift 2
      ;;
    --skip-build)
      SKIP_BUILD=true
      shift
      ;;
    -h|--help)
      usage
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage
      ;;
  esac
done

cd "$ROOT_DIR"

case "$EDITION" in
  base)
    BUILD_ARGS=(--no-default-features)
    ;;
  rdp)
    BUILD_ARGS=(--features freerdp)
    ;;
  *)
    echo "invalid edition: $EDITION (expected base or rdp)" >&2
    exit 2
    ;;
esac

if [[ "$OUTPUT_DIR" != /* ]]; then
  OUTPUT_DIR="$ROOT_DIR/$OUTPUT_DIR"
fi

if [[ -n "$BINARY" && "$BINARY" != /* ]]; then
  BINARY="$ROOT_DIR/$BINARY"
fi

if [[ "$SKIP_BUILD" == true && -z "$BINARY" ]]; then
  echo "--skip-build requires --binary <path>" >&2
  exit 2
fi

if [[ "$SKIP_BUILD" == false && -n "$BINARY" ]]; then
  echo "--binary can only be used together with --skip-build" >&2
  exit 2
fi

if [[ -z "$VERSION" ]]; then
  VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/')"
  if [[ -z "$VERSION" ]]; then
    echo "failed to read version from Cargo.toml" >&2
    exit 1
  fi
fi
# Strip a leading 'v' so CFBundleShortVersionString stays numeric.
VERSION_NUM="${VERSION#v}"

if [[ "$SKIP_BUILD" == false ]]; then
  if [[ -n "$TARGET" ]]; then
    cargo build --locked --release --target "$TARGET" "${BUILD_ARGS[@]}"
    BINARY="$ROOT_DIR/target/$TARGET/release/$APP_NAME"
  else
    cargo build --locked --release "${BUILD_ARGS[@]}"
    BINARY="$ROOT_DIR/target/release/$APP_NAME"
  fi
fi

if [[ ! -f "$BINARY" ]]; then
  echo "compiled binary not found at $BINARY" >&2
  exit 1
fi

APP_DIR="$OUTPUT_DIR/${DISPLAY_NAME}.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
FRAMEWORKS_DIR="$CONTENTS_DIR/Frameworks"
SIGN_IDENTITY="${SIGN_IDENTITY:--}"

rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR" "$FRAMEWORKS_DIR"
cp "$BINARY" "$MACOS_DIR/$APP_NAME"

cp "$ROOT_DIR/assets/icons/tiny-shell.icns" "$RESOURCES_DIR/tiny-shell.icns"

sed -e "s/{{VERSION}}/${VERSION_NUM}/g" \
  -e "s/{{BUNDLE_ID}}/${BUNDLE_ID}/g" \
  -e "s/{{DISPLAY_NAME}}/${DISPLAY_NAME}/g" \
  "$ROOT_DIR/assets/macos/Info.plist" > "$CONTENTS_DIR/Info.plist"

printf 'APPL????' > "$CONTENTS_DIR/PkgInfo"

LINKED_FREERDP=false
if otool -L "$MACOS_DIR/$APP_NAME" | grep -Eq 'lib(freerdp|winpr)'; then
  LINKED_FREERDP=true
fi

if [[ "$EDITION" == "base" && "$LINKED_FREERDP" == true ]]; then
  echo "base macOS package unexpectedly links FreeRDP; build it with --no-default-features" >&2
  exit 1
fi

if [[ "$EDITION" == "rdp" && "$LINKED_FREERDP" == false ]]; then
  echo "RDP macOS package does not link FreeRDP; install FreeRDP 3 and build with --features freerdp" >&2
  exit 1
fi

if [[ "$LINKED_FREERDP" == true ]]; then
  if ! command -v dylibbundler >/dev/null 2>&1; then
    echo "FreeRDP is linked, but dylibbundler is required to package its runtime libraries" >&2
    echo "Install it with: brew install dylibbundler" >&2
    exit 1
  fi
  dylibbundler \
    -od -b \
    -x "$MACOS_DIR/$APP_NAME" \
    -d "$FRAMEWORKS_DIR" \
    -p @executable_path/../Frameworks/
fi

if command -v codesign >/dev/null 2>&1; then
  # Important: do not pass an entitlements file here.
  # A sandboxed macOS app carries the `com.apple.security.app-sandbox` entitlement,
  # which would prevent the file access behavior this app needs.
  codesign --force --deep --sign "$SIGN_IDENTITY" "$APP_DIR" >/dev/null

  ENTITLEMENTS_XML="$(codesign -d --entitlements :- "$APP_DIR" 2>/dev/null || true)"
  if printf '%s' "$ENTITLEMENTS_XML" | grep -q "com.apple.security.app-sandbox"; then
    echo "error: app bundle is sandboxed; remove app sandbox entitlements before packaging" >&2
    exit 1
  fi
fi

echo "$APP_DIR"
