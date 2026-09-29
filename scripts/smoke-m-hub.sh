#!/bin/zsh
# 启动刚构建的 m-hub 并做一次「只读体检」：窗口几何 + 启动日志 + 截图。
#
# 为什么不用 `npm run dev` + 浏览器预览：浏览器里没有透明窗口、没有真实
# 窗口几何、也没有 AppKit 侧代码，等于没验。
set -euo pipefail
OUT="${1:-/tmp/m-hub-smoke.png}"
WAIT="${2:-14}"
APP="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/target/release/bundle/macos/m-hub.app"
LOG="$HOME/Library/Application Support/m-hub/logs/m-hub.log"

[ -d "$APP" ] || { echo "还没构建：$APP 不存在，先跑 npm run tauri:build" >&2; exit 1; }
pkill -f "/Applications/m-hub.app" 2>/dev/null || true
pkill -f "bundle/macos/m-hub.app" 2>/dev/null || true
sleep 1

BEFORE=$(wc -l < "$LOG" 2>/dev/null || echo 0)
"$APP/Contents/MacOS/m-hub" >/tmp/m-hub-smoke.log 2>&1 &
PID=$!
sleep "$WAIT"
kill -0 "$PID" 2>/dev/null || { echo "进程已退出，见 /tmp/m-hub-smoke.log"; exit 1; }

echo "=== 窗口几何（AX 实测，非推断）==="
osascript <<'AS' 2>&1
tell application "System Events"
  tell process "m-hub"
    set out to "" & "窗口数=" & (count of windows) & linefeed
    repeat with i from 1 to (count of windows)
      set w to window i
      set out to out & i & ": pos=" & (position of w as string) & " size=" & (size of w as string) & linefeed
    end repeat
    return out
  end tell
end tell
AS

echo "=== 本次启动新增的日志（重点看 WARN/ERROR）==="
tail -n +$((BEFORE + 1)) "$LOG" | grep -E 'WARN|ERROR' || echo "  （无 WARN/ERROR）"
echo "=== 关键里程碑 ==="
tail -n +$((BEFORE + 1)) "$LOG" | grep -E '恢复窗口状态|启动完成|悬浮球窗口已创建' | sed 's/^/  /'

screencapture -x -o "$OUT"
echo "已截图：$OUT （进程 $PID 仍在运行，验完请退出应用）"
