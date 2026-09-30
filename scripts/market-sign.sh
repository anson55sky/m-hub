#!/usr/bin/env bash
# 用发布私钥给清单签名，产出客户端要的 `.sig` 文件（与清单并列上传即可）。
#
#   bash scripts/market-sign.sh <清单文件>
#
# 例：
#   bash scripts/market-sign.sh dist/registry.json   # → dist/registry.json.sig
#   bash scripts/market-sign.sh dist/update.json     # → dist/update.json.sig
#
# ## 签名的是「原始字节」
#
# 客户端 `market.rs` / `updater.rs` 是对**下载到的原始字节**做验签的
# （`verify_detached(&content, &sig)`）。所以：
#   · 签完之后**不要**再改清单内容（哪怕只是格式化）—— 一改就验不过
#   · 上传时清单与 .sig 必须是同一份字节
#
# ## 私钥不经过管道
#
# 私钥只从 ~/.m-hub-signing/ 读，openssl 直接读文件；本脚本不打印它，
# 也不会把它写进任何输出文件。
set -euo pipefail

KEY_DIR="${MHUB_SIGNING_DIR:-$HOME/.m-hub-signing}"
PRIV="$KEY_DIR/market_private.pem"

if [ $# -ne 1 ]; then
  echo "用法: bash scripts/market-sign.sh <清单文件>" >&2
  exit 1
fi
TARGET="$1"

if [ ! -f "$TARGET" ]; then
  echo "找不到清单：$TARGET" >&2
  exit 1
fi
if [ ! -f "$PRIV" ]; then
  echo "找不到私钥：$PRIV" >&2
  echo "先跑 bash scripts/market-keygen.sh 生成密钥对。" >&2
  exit 1
fi

# 写成 .sig（base64 文本，与客户端 String::from_utf8_lossy 的读法一致）
openssl pkeyutl -sign -inkey "$PRIV" -rawin -in "$TARGET" \
  | base64 | tr -d '\n' > "$TARGET.sig"

echo "✓ 已签名 → $TARGET.sig"
echo "  清单 $(wc -c < "$TARGET" | tr -d ' ') 字节 / 签名 $(wc -c < "$TARGET.sig" | tr -d ' ') 字节"
echo
echo "⚠️  签完不要再改 $TARGET —— 客户端验的是原始字节，改了就对不上了。"
echo "   两个文件要一起上传：$TARGET 与 $TARGET.sig"
