#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
MAC_SRC_ICON="$ROOT_DIR/public/icon/tiy-macos.png"
COMMON_SRC_ICON="$ROOT_DIR/public/icon/tiy.png"
OUT_DIR="$ROOT_DIR/src-tauri/icons"
ICONSET_DIR="/tmp/tiy.iconset"
ICO_TMP_DIR="/tmp/tiy-ico"

require_file() {
  local path="$1"
  local label="$2"
  if [[ ! -f "$path" ]]; then
    echo "错误：未找到${label}: $path" >&2
    exit 1
  fi
}

require_tool() {
  local tool="$1"
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "错误：缺少命令 $tool" >&2
    exit 1
  fi
}

resize_png() {
  local src="$1"
  local size="$2"
  local out="$3"
  sips -z "$size" "$size" "$src" --out "$out" >/dev/null
}

generate_ico_with_uvx() {
  local src_dir="$1"
  local out_file="$2"
  uvx --from pillow python - <<'PY' "$src_dir" "$out_file"
import sys
from pathlib import Path
from PIL import Image

src_dir = Path(sys.argv[1])
out_file = Path(sys.argv[2])
sizes = [16, 24, 32, 48, 64, 128, 256]
images = [src_dir / f"{size}.png" for size in sizes]
for path in images:
    if not path.exists():
        raise SystemExit(f"missing png for ico: {path}")

base = Image.open(images[-1])
base.save(out_file, format="ICO", sizes=[(s, s) for s in sizes])
PY
}

generate_ico_with_node() {
  local src_dir="$1"
  local out_file="$2"
  node - <<'JS' "$src_dir" "$out_file"
const fs = require('fs');
const path = require('path');
const pngToIco = require('png-to-ico');

(async () => {
  const srcDir = process.argv[2];
  const outFile = process.argv[3];
  const sizes = [16, 24, 32, 48, 64, 128, 256];
  const files = sizes.map((size) => path.join(srcDir, `${size}.png`));
  for (const file of files) {
    if (!fs.existsSync(file)) {
      throw new Error(`missing png for ico: ${file}`);
    }
  }
  const buf = await pngToIco(files);
  fs.writeFileSync(outFile, buf);
})().catch((err) => {
  console.error(err.message || err);
  process.exit(1);
});
JS
}

generate_ico_with_python() {
  local src_dir="$1"
  local out_file="$2"
  python3 - <<'PY' "$src_dir" "$out_file"
import sys
from pathlib import Path

src_dir = Path(sys.argv[1])
out_file = Path(sys.argv[2])
sizes = [16, 24, 32, 48, 64, 128, 256]
images = []
for size in sizes:
    path = src_dir / f"{size}.png"
    if not path.exists():
        raise SystemExit(f"missing png for ico: {path}")
    images.append(path)

try:
    from PIL import Image
except ImportError as exc:
    raise SystemExit(
        "Pillow 未安装，无法生成 icon.ico。可选方式：\n"
        "1. 安装 uv 后重试，脚本会优先使用 uvx。\n"
        "2. 安装 Node 后执行: npm i -D png-to-ico\n"
        "3. 或执行: python3 -m pip install Pillow"
    ) from exc

base = Image.open(images[-1])
base.save(out_file, format="ICO", sizes=[(s, s) for s in sizes])
PY
}

generate_ico() {
  local src_dir="$1"
  local out_file="$2"

  if command -v uvx >/dev/null 2>&1; then
    echo "使用 uvx 生成 Windows icon.ico"
    generate_ico_with_uvx "$src_dir" "$out_file"
    return
  fi

  if command -v node >/dev/null 2>&1; then
    if node -e "require.resolve('png-to-ico')" >/dev/null 2>&1; then
      echo "使用 Node + png-to-ico 生成 Windows icon.ico"
      generate_ico_with_node "$src_dir" "$out_file"
      return
    fi
  fi

  if command -v python3 >/dev/null 2>&1; then
    echo "使用 python3 + Pillow 生成 Windows icon.ico"
    generate_ico_with_python "$src_dir" "$out_file"
    return
  fi

  echo "错误：无法生成 icon.ico。需要以下任一环境：uvx，或 node + png-to-ico，或 python3 + Pillow。" >&2
  exit 1
}

require_tool sips
require_tool iconutil
require_file "$MAC_SRC_ICON" "macOS 源图标"
require_file "$COMMON_SRC_ICON" "Windows/Linux 源图标"

mkdir -p "$OUT_DIR"
rm -rf "$ICONSET_DIR" "$ICO_TMP_DIR"
mkdir -p "$ICONSET_DIR" "$ICO_TMP_DIR"

# macOS iconset -> .icns
resize_png "$MAC_SRC_ICON" 16 "$ICONSET_DIR/icon_16x16.png"
resize_png "$MAC_SRC_ICON" 32 "$ICONSET_DIR/icon_16x16@2x.png"
resize_png "$MAC_SRC_ICON" 32 "$ICONSET_DIR/icon_32x32.png"
resize_png "$MAC_SRC_ICON" 64 "$ICONSET_DIR/icon_32x32@2x.png"
resize_png "$MAC_SRC_ICON" 128 "$ICONSET_DIR/icon_128x128.png"
resize_png "$MAC_SRC_ICON" 256 "$ICONSET_DIR/icon_128x128@2x.png"
resize_png "$MAC_SRC_ICON" 256 "$ICONSET_DIR/icon_256x256.png"
resize_png "$MAC_SRC_ICON" 512 "$ICONSET_DIR/icon_256x256@2x.png"
resize_png "$MAC_SRC_ICON" 512 "$ICONSET_DIR/icon_512x512.png"
cp "$MAC_SRC_ICON" "$ICONSET_DIR/icon_512x512@2x.png"
iconutil -c icns "$ICONSET_DIR" -o "$OUT_DIR/icon.icns"

# Common PNGs for Linux / Tauri bundle.
resize_png "$COMMON_SRC_ICON" 16 "$OUT_DIR/16x16.png"
resize_png "$COMMON_SRC_ICON" 32 "$OUT_DIR/32x32.png"
resize_png "$COMMON_SRC_ICON" 64 "$OUT_DIR/64x64.png"
resize_png "$COMMON_SRC_ICON" 128 "$OUT_DIR/128x128.png"
resize_png "$COMMON_SRC_ICON" 256 "$OUT_DIR/128x128@2x.png"
resize_png "$COMMON_SRC_ICON" 256 "$OUT_DIR/256x256.png"
resize_png "$COMMON_SRC_ICON" 512 "$OUT_DIR/256x256@2x.png"
resize_png "$COMMON_SRC_ICON" 512 "$OUT_DIR/512x512.png"
cp "$COMMON_SRC_ICON" "$OUT_DIR/512x512@2x.png"
cp "$COMMON_SRC_ICON" "$OUT_DIR/icon.png"

# Windows .ico with embedded multiple sizes.
for size in 16 24 32 48 64 128 256; do
  resize_png "$COMMON_SRC_ICON" "$size" "$ICO_TMP_DIR/${size}.png"
done

generate_ico "$ICO_TMP_DIR" "$OUT_DIR/icon.ico"

rm -rf "$ICONSET_DIR" "$ICO_TMP_DIR"

cat <<EOF
已生成三平台图标资源：
- $OUT_DIR/icon.icns
- $OUT_DIR/icon.ico
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

源文件：
- macOS: $MAC_SRC_ICON
- Windows/Linux: $COMMON_SRC_ICON
EOF
