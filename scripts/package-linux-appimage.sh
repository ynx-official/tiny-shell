#!/usr/bin/env bash
set -euo pipefail

# Build a reproducible TinyShell x86_64 AppImage from the Linux release binary.
# linuxdeploy is pinned and checksum-verified so CI and local packaging use the
# same dependency deployment logic instead of downloading a moving release.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="tiny-shell"
TARGET="x86_64-unknown-linux-gnu"
VERSION=""
OUTPUT_DIR="dist"
BINARY=""
LINUXDEPLOY_PATH="${LINUXDEPLOY:-}"
SKIP_BUILD=false

LINUXDEPLOY_VERSION="1-alpha-20251107-1"
LINUXDEPLOY_SHA256="c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d"
LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/${LINUXDEPLOY_VERSION}/linuxdeploy-x86_64.AppImage"

usage() {
  cat >&2 <<'EOF'
Usage: scripts/package-linux-appimage.sh [options]

Options:
  --version <ver>       Release version (default: Cargo.toml package version)
  --target <triple>     Rust target (default: x86_64-unknown-linux-gnu)
  --output-dir <dir>    Destination directory (default: dist)
  --binary <path>       Use an explicit release binary
  --linuxdeploy <path>  Use an existing linuxdeploy executable
  --skip-build          Reuse the existing release binary
  -h, --help            Show this help
EOF
}

fail() {
  echo "AppImage packaging failed: $*" >&2
  exit 1
}

require_value() {
  local option="$1"
  local value="${2-}"
  [[ -n "$value" ]] || fail "$option requires a value"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      require_value "$1" "${2-}"
      VERSION="$2"
      shift 2
      ;;
    --target)
      require_value "$1" "${2-}"
      TARGET="$2"
      shift 2
      ;;
    --output-dir)
      require_value "$1" "${2-}"
      OUTPUT_DIR="$2"
      shift 2
      ;;
    --binary)
      require_value "$1" "${2-}"
      BINARY="$2"
      shift 2
      ;;
    --linuxdeploy)
      require_value "$1" "${2-}"
      LINUXDEPLOY_PATH="$2"
      shift 2
      ;;
    --skip-build)
      SKIP_BUILD=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

[[ "$(uname -s)" == "Linux" ]] || fail "this script must run on Linux"
[[ "$(uname -m)" == "x86_64" ]] || fail "only Linux x86_64 packaging is currently supported"
[[ "$TARGET" == "x86_64-unknown-linux-gnu" ]] || fail "unsupported target: $TARGET"

for command in file ldd mktemp patchelf sha256sum; do
  command -v "$command" >/dev/null 2>&1 || fail "required command not found: $command"
done
command -v desktop-file-validate >/dev/null 2>&1 \
  || fail "desktop-file-validate is required (install desktop-file-utils)"

cd "$ROOT_DIR"

if [[ -z "$VERSION" ]]; then
  VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/')"
fi
VERSION_NUM="${VERSION#v}"
[[ "$VERSION_NUM" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]] \
  || fail "invalid semantic version: $VERSION"
VERSION_TAG="v${VERSION_NUM}"

if [[ "$SKIP_BUILD" == false ]]; then
  command -v cargo >/dev/null 2>&1 || fail "cargo is required unless --skip-build is used"
  cargo build \
    --locked \
    --release \
    --target "$TARGET" \
    --no-default-features \
    --features freerdp
fi

if [[ -z "$BINARY" ]]; then
  BINARY="target/$TARGET/release/$APP_NAME"
fi
[[ -f "$BINARY" ]] || fail "compiled binary not found: $BINARY"
BINARY="$(realpath "$BINARY")"

DESKTOP_FILE="$(realpath assets/tiny-shell.desktop)"
ICON_FILE="$(realpath assets/icons/256x256/tiny-shell.png)"
LICENSE_FILE="$(realpath LICENSE)"
desktop-file-validate "$DESKTOP_FILE"
file "$BINARY" | grep -Eq 'ELF 64-bit.*x86-64' \
  || fail "release binary is not an x86_64 ELF executable: $BINARY"

mkdir -p "$OUTPUT_DIR"
OUTPUT_DIR="$(realpath "$OUTPUT_DIR")"
OUTPUT_FILE="$OUTPUT_DIR/tiny-shell-${VERSION_TAG}-linux-x86_64.AppImage"

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/tiny-shell-appimage.XXXXXXXX")"
cleanup() {
  if [[ -n "${WORK_DIR:-}" && "$WORK_DIR" != "/" && -d "$WORK_DIR" ]]; then
    rm -rf -- "$WORK_DIR"
  fi
}
trap cleanup EXIT

if [[ -z "$LINUXDEPLOY_PATH" ]]; then
  command -v curl >/dev/null 2>&1 || fail "curl is required to download linuxdeploy"
  LINUXDEPLOY_PATH="$WORK_DIR/linuxdeploy-x86_64.AppImage"
  curl \
    --fail \
    --location \
    --retry 3 \
    --retry-all-errors \
    --proto '=https' \
    --tlsv1.2 \
    --output "$LINUXDEPLOY_PATH" \
    "$LINUXDEPLOY_URL"
  echo "$LINUXDEPLOY_SHA256  $LINUXDEPLOY_PATH" | sha256sum --check --status \
    || fail "linuxdeploy checksum verification failed"
  chmod 0755 "$LINUXDEPLOY_PATH"
else
  [[ -x "$LINUXDEPLOY_PATH" ]] || fail "linuxdeploy is not executable: $LINUXDEPLOY_PATH"
  LINUXDEPLOY_PATH="$(realpath "$LINUXDEPLOY_PATH")"
fi

APPDIR="$WORK_DIR/TinyShell.AppDir"
TEMP_APPIMAGE="$WORK_DIR/TinyShell.AppImage"
mkdir -p "$APPDIR/usr/share/licenses/tiny-shell"
cp "$LICENSE_FILE" "$APPDIR/usr/share/licenses/tiny-shell/LICENSE"

APPIMAGE_EXTRACT_AND_RUN=1 \
ARCH=x86_64 \
VERSION="$VERSION_NUM" \
LDAI_VERSION="$VERSION_NUM" \
LDAI_OUTPUT="$TEMP_APPIMAGE" \
LDAI_NO_APPSTREAM=1 \
  "$LINUXDEPLOY_PATH" \
    --appdir "$APPDIR" \
    --executable "$BINARY" \
    --desktop-file "$DESKTOP_FILE" \
    --icon-file "$ICON_FILE" \
    --output appimage

validate_appdir() {
  local appdir="$1"
  local executable="$appdir/usr/bin/tiny-shell"
  local desktop="$appdir/usr/share/applications/tiny-shell.desktop"

  [[ -x "$appdir/AppRun" ]] || fail "AppDir is missing executable AppRun"
  [[ -x "$executable" ]] || fail "AppDir is missing usr/bin/tiny-shell"
  [[ -f "$desktop" ]] || fail "AppDir is missing tiny-shell.desktop"
  [[ -e "$appdir/.DirIcon" ]] || fail "AppDir is missing .DirIcon"
  [[ -e "$appdir/tiny-shell.png" ]] || fail "AppDir is missing its root icon"
  desktop-file-validate "$desktop"

  local pattern
  for pattern in 'libfreerdp-client3.so*' 'libfreerdp3.so*' 'libwinpr3.so*'; do
    find "$appdir/usr/lib" -name "$pattern" -print -quit | grep -q . \
      || fail "AppDir is missing required FreeRDP runtime: $pattern"
  done

  local dependencies
  dependencies="$(ldd "$executable")"
  if grep -q 'not found' <<<"$dependencies"; then
    echo "$dependencies" >&2
    fail "AppDir executable has unresolved dynamic libraries"
  fi

  local rpath
  rpath="$(patchelf --print-rpath "$executable")"
  local entry
  local -a rpath_entries=()
  IFS=':' read -r -a rpath_entries <<<"$rpath"
  for entry in "${rpath_entries[@]}"; do
    if [[ -n "$entry" && "$entry" != "\$ORIGIN"* && "$entry" != "\${ORIGIN}"* ]]; then
      fail "AppDir executable contains a non-relocatable RPATH entry: $entry"
    fi
  done
}

validate_appdir "$APPDIR"
[[ -x "$TEMP_APPIMAGE" ]] || fail "linuxdeploy did not create an executable AppImage"
file "$TEMP_APPIMAGE" | grep -Eq 'ELF 64-bit.*x86-64' \
  || fail "generated AppImage is not an x86_64 ELF executable"
"$TEMP_APPIMAGE" --appimage-version >/dev/null

EXTRACT_DIR="$WORK_DIR/extracted"
mkdir -p "$EXTRACT_DIR"
(
  cd "$EXTRACT_DIR"
  "$TEMP_APPIMAGE" --appimage-extract >/dev/null
)
validate_appdir "$EXTRACT_DIR/squashfs-root"

install -m 0755 "$TEMP_APPIMAGE" "$OUTPUT_FILE"
echo "AppImage created: $OUTPUT_FILE"
