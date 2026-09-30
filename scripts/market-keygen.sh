#!/usr/bin/env bash
# 生成 m-hub 的 Ed25519 签名密钥对（市场清单 + 应用更新共用一把，见 signing.rs）。
#
# ## 为什么需要它
#
# 市场清单 `registry.json` 与更新清单 `update.json` 都由**私钥**对原始字节做分离
# 签名，客户端只内嵌公钥、验签通过才信任内容（signing.rs）。公钥在仓库里
# （`src-tauri/keys/market_public.key`），**私钥按约定绝不进仓库、绝不进二进制**。
#
# 仓库里也没有现成的私钥 —— 上游那把只存在于它的发版机上，本仓库当初轮换过一次，
# 新旧公钥不同（`zy7ReyGl...` vs 上游的 `gNSitjwb...`），所以那把私钥这里用不了。
#
# ## 私钥放哪
#
# 默认 `~/.m-hub-signing/`，**在仓库之外**。脚本会把该目录权限收到 700。
# 私钥永远不会被打印、不会被写进仓库、不会被本脚本以外的任何文件带出去。
#
# ## 怎么用
#
#   bash scripts/market-keygen.sh
#
# 它会：
#   1. 生成密钥对到 ~/.m-hub-signing/（私钥 market_private.pem，权限 600）
#   2. 把**公钥**写进 src-tauri/keys/market_public.key（这个是公开的，可以进仓库）
#   3. 用新私钥给 signing.rs 里的测试向量签名，打印出可直接粘贴的常量
#   4. 提示你重新构建（公钥是 include_str! 进二进制的）
#
# ## 安全提醒
#
# - 私钥泄露 = 任何人都能伪造市场清单和更新包，你的用户会被投毒。丢了就重新生成
#   （发版一次，成本 = 你的用户手动更新一次）
# - 不要把私钥贴进任何对话、issue、聊天窗口
# - 备份一份到离线介质；本脚本不会替你备份
set -euo pipefail

KEY_DIR="${MHUB_SIGNING_DIR:-$HOME/.m-hub-signing}"
PRIV="$KEY_DIR/market_private.pem"
PUB_RAW="$KEY_DIR/market_public.raw.b64"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PUB_DEST="$REPO_ROOT/src-tauri/keys/market_public.key"

if [ -e "$PRIV" ]; then
  echo "已存在私钥：$PRIV" >&2
  echo "要重新生成请先手动删除那个文件（重生成会让现有客户端的旧签名失效）。" >&2
  exit 1
fi

command -v openssl >/dev/null || { echo "需要 openssl" >&2; exit 1; }

mkdir -p "$KEY_DIR"
chmod 700 "$KEY_DIR"

# Ed25519 私钥：openssl 用 PKCS#8 包装，取最后 32 字节即 seed（与 ed25519-dalek 兼容）
openssl genpkey -algorithm ed25519 -out "$KEY_DIR/market_private.pem" 2>/dev/null
chmod 600 "$KEY_DIR/market_private.pem"
openssl pkey -in "$KEY_DIR/market_private.pem" -pubout -outform DER \
  | tail -c 32 | base64 > "$PUB_RAW"
chmod 600 "$PUB_RAW"

PUB="$(tr -d '\n' < "$PUB_RAW")"
if [ "${#PUB}" -ne 44 ]; then
  echo "公钥长度异常（${#PUB}，应为 44）：openssl 输出格式变了，先别继续" >&2
  exit 1
fi

printf '%s' "$PUB" > "$PUB_DEST"
echo "✓ 公钥已写入 src-tauri/keys/market_public.key"
echo "  $PUB"
echo
# ⚠️ 这里必须用 ${PRIV} 而不是 $PRIV：中文全角括号紧跟变量名时，
# bash 会把那个多字节字符的前半截当成变量名的一部分（set -u 下直接报
# "unbound variable"）。踩过一次。
echo "✓ 私钥已写入 ${PRIV}（权限 600，目录 700）—— 它不会离开这台机器"
echo

# signing.rs 的测试向量：用新私钥对固定文本签名，打印出可粘贴的常量。
#
# ⚠️ `-rawin` 必须喂**普通文件**，不能喂管道：管道下 openssl 拿不到文件长度，
# 会报 "unable to determine file size for oneshot operation"（踩过一次）。
TEST_DATA="m-hub market registry test vector v1"
TMP_TV="$(mktemp)"
trap 'rm -f "$TMP_TV"' EXIT
printf '%s' "$TEST_DATA" > "$TMP_TV"
SIG="$(openssl pkeyutl -sign -inkey "$KEY_DIR/market_private.pem" -rawin -in "$TMP_TV" \
  | base64 | tr -d '\n')"

cat <<EOF
下一步：把 signing.rs 里 tests 模块的 TEST_SIGNATURE 换成下面这串（它由这把新私钥签出，
公钥换了它必须一起换，否则 valid_signature_passes 会失败）：

    const TEST_SIGNATURE: &str =
        "$SIG";

然后重新构建（公钥是 include_str! 烘进二进制的）：
    npm run tauri:build
EOF
