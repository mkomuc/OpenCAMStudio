#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "$(uname -s)" != Darwin ]]; then
  echo "This script must run on macOS." >&2
  exit 1
fi
if [[ "$(uname -m)" != arm64 ]]; then
  echo "This project currently packages macOS arm64 only; run this on Apple silicon." >&2
  exit 1
fi

export OCCT_ROOT="${OCCT_ROOT:-$ROOT/.local/occt}"
export OCCT_SOURCE="${OCCT_SOURCE:-$ROOT/.local/occt-source}"
export OCCT_BUILD="${OCCT_BUILD:-$ROOT/.local/occt-build}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT/dist}"
APP="$OUTPUT_DIR/Open CAM Studio.app"
TARGET="aarch64-apple-darwin"

for command in cargo python3 sips codesign otool install_name_tool; do
  command -v "$command" >/dev/null || {
    echo "Required command not found: $command" >&2
    exit 1
  }
done
for path in \
  "$OCCT_ROOT/include/opencascade/STEPControl_Reader.hxx" \
  "$OCCT_ROOT/lib"; do
  if [[ ! -e "$path" ]]; then
    echo "OCCT 7.9.3 SDK is incomplete: missing $path" >&2
    echo "Set OCCT_ROOT to a complete OCCT 7.9.3 installation." >&2
    exit 1
  fi
done
if [[ ! -d "$OCCT_ROOT/share/occt/resources/StdResource" && \
      ! -d "$OCCT_ROOT/share/opencascade/resources/StdResource" ]]; then
  echo "OCCT SDK has no StdResource data under share/occt or share/opencascade." >&2
  exit 1
fi
for name in LICENSE_LGPL_21.txt OCCT_LGPL_EXCEPTION.txt; do
  if [[ ! -f "$OCCT_SOURCE/$name" && \
        ! -f "$OCCT_ROOT/share/doc/opencascade/$name" ]]; then
    echo "Missing OCCT license $name in OCCT_SOURCE or OCCT_ROOT/share/doc/opencascade." >&2
    exit 1
  fi
done

echo "Building Open CAM Studio with gui,occt..."
cargo build --locked --release --target "$TARGET" \
  -p cam-app --features gui,occt --bin opencamstudio --bin opencamstudio-mcp
cargo build --locked --release --target "$TARGET" \
  -p cam-kernel --features occt --example occt_smoke

BIN="$ROOT/target/$TARGET/release/opencamstudio"
SMOKE="$ROOT/target/$TARGET/release/examples/occt_smoke"
[[ -x "$BIN" && -x "$SMOKE" ]] || {
  echo "Cargo completed without producing the expected release binaries." >&2
  exit 1
}

mkdir -p "$OUTPUT_DIR"
if [[ "$APP" == / || "$APP" == "$ROOT" ]]; then
  echo "Refusing unsafe output path: $APP" >&2
  exit 1
fi
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/opencamstudio"
cp "$ROOT/target/$TARGET/release/opencamstudio-mcp" "$APP/Contents/MacOS/opencamstudio-mcp"

python3 .github/scripts/package-occt.py stage "$APP"

ICON_TMP="$(mktemp -d "${TMPDIR:-/tmp}/opencamstudio-icon.XXXXXX")"
trap 'rm -rf "$ICON_TMP"' EXIT
sips -z 16 16 "$ROOT/crates/cam-app/assets/opencamstudio-32.png" \
  --out "$ICON_TMP/opencamstudio-16.png" >/dev/null
sips -z 48 48 "$ROOT/crates/cam-app/assets/opencamstudio-64.png" \
  --out "$ICON_TMP/opencamstudio-48.png" >/dev/null
python3 - "$ROOT/crates/cam-app/assets" "$ICON_TMP/opencamstudio-48.png" \
  "$APP/Contents/Resources/opencamstudio.icns" <<'PY'
import struct
import sys
from pathlib import Path

assets, icon48, output = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
images = [
    ('icp4', icon48.parent / 'opencamstudio-16.png'),
    ('icp5', assets / 'opencamstudio-32.png'),
    ('icp6', icon48),
    ('ic07', assets / 'opencamstudio-128.png'),
    ('ic08', assets / 'opencamstudio-256.png'),
    ('ic09', assets / 'opencamstudio-512.png'),
    ('ic10', assets / 'opencamstudio-1024.png'),
]
chunks = []
for kind, image in images:
    data = image.read_bytes()
    chunks.append(kind.encode('ascii') + struct.pack('>I', len(data) + 8) + data)
body = b''.join(chunks)
output.write_bytes(b'icns' + struct.pack('>I', len(body) + 8) + body)
PY
file "$APP/Contents/Resources/opencamstudio.icns" | grep -q 'Mac OS X icon'

VERSION="$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "cam-app"))')"
python3 - "$APP/Contents/Info.plist" "$VERSION" <<'PY'
import plistlib
import sys
from pathlib import Path

plist = {
    'CFBundleName': 'Open CAM Studio',
    'CFBundleDisplayName': 'Open CAM Studio',
    'CFBundleIdentifier': 'io.github.pr0m1th3as.OpenCAMStudio',
    'CFBundleVersion': sys.argv[2],
    'CFBundleShortVersionString': sys.argv[2],
    'CFBundleExecutable': 'opencamstudio',
    'CFBundleIconFile': 'opencamstudio',
    'CFBundlePackageType': 'APPL',
    'LSMinimumSystemVersion': '11.0',
    'NSHighResolutionCapable': True,
}
Path(sys.argv[1]).write_bytes(plistlib.dumps(plist, fmt=plistlib.FMT_XML))
PY
printf 'APPL????' > "$APP/Contents/PkgInfo"

python3 .github/scripts/package-occt.py verify "$APP" --smoke "$SMOKE"
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"
plutil -lint "$APP/Contents/Info.plist"

echo "Created self-contained macOS app: $APP"
