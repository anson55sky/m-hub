/**
 * 守卫：拖动轮询循环里不得每帧调用**阻塞型**窗口 getter
 *
 * ## 为什么需要它
 *
 * tauri-runtime-wry 里两类窗口调用的代价天差地别：
 *   · setter（`set_position` 等）走 `send_user_message` —— **不阻塞**，每帧调没问题；
 *   · getter（`available_monitors` / `outer_position` / `work_area` …）走
 *     `window_getter!` 宏：建 channel、把消息发给主线程、然后 **`rx.recv()` 死等回包**
 *     —— 也就是**每一次调用都同步往返主线程一趟**。
 *
 * 把 getter 放进 16ms 的拖动轮询里 = 每秒 60 次阻塞往返。主线程只要在忙
 * （WKWebView 合成、还有 100ms 的边缘监视循环在抢），每帧就得多等几毫秒到
 * 几十毫秒，**窗口就明显跟不上鼠标**。实机症状：能拖动，但「不跟手」。
 *
 * 这类缺陷测试**完全测不出来**（功能是对的，只是慢），所以只能靠守卫。
 *
 * ## 判据
 *  ① `drag_loop` 体内必须有 `inside_wa(` —— 说明存在「光标是否还在缓存的那块屏上」的判断
 *  ② `work_area_physical(` 在 `drag_loop` 体内只能出现在 `else {` 分支里
 *     （即「换了显示器才重取」），出现在条件之外就说明又变成每帧取
 *  ③ `drag_loop` 体内不得直接出现 `available_monitors` / `outer_position`
 *     —— 它们同样是阻塞 getter，绕开 `work_area_physical` 直接调也一样糟
 */
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const SRC = join(root, 'src-tauri/src/clipboard.rs')

const problems = []
const src = readFileSync(SRC, 'utf8')

function bodyFrom(sig) {
  const at = src.indexOf(sig)
  if (at < 0) return null
  const open = src.indexOf('{', at + sig.length)
  if (open < 0) return null
  let depth = 0
  for (let i = open; i < src.length; i++) {
    if (src[i] === '{') depth++
    else if (src[i] === '}') {
      depth--
      if (depth === 0) return src.slice(open, i + 1)
    }
  }
  return null
}

const loop = bodyFrom('fn drag_loop')
if (!loop) {
  problems.push('找不到 drag_loop：拖动循环被挪走或改名了，本守卫需同步')
} else {
  // ① 必须有「是否还在缓存屏上」的判断
  if (!loop.includes('inside_wa(')) {
    problems.push(
      'drag_loop 里没有 inside_wa 判断：无法区分「还在同一块屏」与「拖到别的屏了」，' +
        '工作区要么取错、要么就得每帧重取（= 每帧一次阻塞往返，拖动不跟手）',
    )
  }

  // ② work_area_physical 只能待在 else 分支里
  const callAt = loop.indexOf('work_area_physical(')
  if (callAt < 0) {
    problems.push('drag_loop 里找不到 work_area_physical 调用：本守卫的判据②已失效，请同步本守卫')
  } else {
    // 取调用之前那一小段，确认它前面紧邻着 else 分支的开头
    const before = loop.slice(Math.max(0, callAt - 160), callAt)
    if (!/else\s*\{[^}]*$/.test(before)) {
      problems.push(
        'drag_loop 里的 work_area_physical 不在 `else {` 分支内：' +
          '它被提到条件之外了 = 每帧一次阻塞的主线程往返，窗口会跟不上鼠标',
      )
    }
  }

  // ③ 不得直接调其它阻塞 getter
  for (const g of ['available_monitors', 'outer_position', 'outer_size', 'current_monitor', 'inner_size']) {
    if (loop.includes(g + '(')) {
      problems.push(
        `drag_loop 里直接调用了 ${g}(：它同样是 window_getter! 阻塞 getter，` +
          '每帧调一次就会让拖动不跟手。取不到就让 drag_target 用兜底矩形。',
      )
    }
  }
}

if (problems.length) {
  console.error('拖动轮询守卫未通过：')
  for (const p of problems) console.error(`  ✗ ${p}`)
  process.exit(1)
}
console.log('✓ 拖动轮询：工作区只在换屏时重取，循环体内无阻塞 getter')
