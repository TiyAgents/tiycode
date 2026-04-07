#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SRC_ICON="$ROOT_DIR/public/icon/tiy-macos.png"
OUT_DIR="$ROOT_DIR/src-tauri/icons"
ICONSET_DIR="/tmp/tiy-macos.iconset"

if [[ ! -f "$SRC_ICON" ]]; then
  echo "错误：未找到源图标文件: $SRC_ICON" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"
rm -rf "$ICONSET_DIR"
mkdir -p "$ICONSET_DIR"

# macOS .icns needs a full iconset up to 1024x1024.
sips -z 16 16 "$SRC_ICON" --out "$ICONSET_DIR/icon_16x16.png" >/dev/null
sips -z 32 32 "$SRC_ICON" --out "$ICONSET_DIR/icon_16x16@2x.png" >/dev/null
sips -z 32 32 "$SRC_ICON" --out "$ICONSET_DIR/icon_32x32.png" >/dev/null
sips -z 64 64 "$SRC_ICON" --out "$ICONSET_DIR/icon_32x32@2x.png" >/dev/null
sips -z 128 128 "$SRC_ICON" --out "$ICONSET_DIR/icon_128x128.png" >/dev/null
sips -z 256 256 "$SRC_ICON" --out "$ICONSET_DIR/icon_128x128@2x.png" >/dev/null
sips -z 256 256 "$SRC_ICON" --out "$ICONSET_DIR/icon_256x256.png" >/dev/null
sips -z 512 512 "$SRC_ICON" --out "$ICONSET_DIR/icon_256x256@2x.png" >/dev/null
sips -z 512 512 "$SRC_ICON" --out "$ICONSET_DIR/icon_512x512.png" >/dev/null
cp "$SRC_ICON" "$ICONSET_DIR/icon_512x512@2x.png"

iconutil -c icns "$ICONSET_DIR" -o "$OUT_DIR/icon.icns"

# Export common standalone PNG sizes as well.
sips -z 16 16 "$SRC_ICON" --out "$OUT_DIR/16x16.png" >/dev/null
sips -z 32 32 "$SRC_ICON" --out "$OUT_DIR/32x32.png" >/dev/null
sips -z 64 64 "$SRC_ICON" --out "$OUT_DIR/64x64.png" >/dev/null
sips -z 128 128 "$SRC_ICON" --out "$OUT_DIR/128x128.png" >/dev/null
sips -z 256 256 "$SRC_ICON" --out "$OUT_DIR/128x128@2x.png" >/dev/null
sips -z 256 256 "$SRC_ICON" --out "$OUT_DIR/256x256.png" >/dev/null
sips -z 512 512 "$SRC_ICON" --out "$OUT_DIR/256x256@2x.png" >/dev/null
sips -z 512 512 "$SRC_ICON" --out "$OUT_DIR/512x512.png" >/dev/null
cp "$SRC_ICON" "$OUT_DIR/512x512@2x.png"
cp "$SRC_ICON" "$OUT_DIR/icon.png"

cat <<EOF
已生成以下 mac 图标资源：
- $OUT_DIR/icon.icns
- $OUT_DIR/16x16.png
- $OUT_DIR/32x32.png
- $OUT_DIR/64x64.png
- $OUT_DIR/128x128.png
- $OUT_DIR/128x128@2x.png
- $OUT_DIR/256x256.png
- $OUT_DIR/256x256@2x.png
- $OUT_DIR/512x512.png
- $OUT_DIR/512x512@2x.png
- $OUT_DIR/icon.png

源文件：$SRC_ICON
EOF
