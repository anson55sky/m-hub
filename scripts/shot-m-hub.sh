#!/bin/zsh
# 启动刚构建的 m-hub 并截图，用于人眼核对「主窗口圆角」这类只能实机确认的改动。
#
# 为什么不能用 `npm run dev` + 浏览器预览：浏览器里没有**透明窗口**，
# 圆角是靠 tauri.conf 的 transparent + .app-shell 裁出来的，预览模式永远是对的，
# 等于没验。
#
# 用法：scripts/shot-m-hub.sh <输出png> [等待秒数]
set -euo pipefail

OUT="${1:-/tmp/m-hub.png}"
WAIT="${2:-10}"
APP="/Users/sky/Desktop/项目/CODE/m-hub/src-tauri/target/release/bundle/macos/m-hub.app"

[ -d "$APP" ] || { echo "还没构建：$APP 不存在，先跑 npm run tauri:build" >&2; exit 1; }

# 旧实例会让 single-instance 插件把启动请求转发过去，新版根本起不来
pkill -f "/Applications/m-hub.app" 2>/dev/null || true
pkill -f "bundle/macos/m-hub.app" 2>/dev/null || true
sleep 1

"$APP/Contents/MacOS/m-hub" >/tmp/m-hub-stdout.log 2>&1 &
PID=$!
sleep "$WAIT"
kill -0 "$PID" 2>/dev/null || { echo "进程已退出，见 /tmp/m-hub-stdout.log"; exit 1; }

# -x 静音快门声；不给 -o 会带桌面壁纸进来——但我们**要**看圆角处透出的桌面，
# 所以刻意不屏蔽。
screencapture -x -o "$OUT"
echo "已截图：$OUT （进程 $PID 仍在运行，验完记得退出应用）"
