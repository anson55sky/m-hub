/**
 * 守卫：macOS 剪贴板浮层的定位必须夹**工作区**，不能夹整块屏
 *
 * ## 为什么需要它
 *
 * 用户报「浮层跳到下方挡住的地方」。根因不是夹取算错，而是**边界取错了**：
 * Windows 那条路径取 `GetMonitorInfoW` 的 `rcWork`（本来就是工作区），
 * 移植到 macOS 时换成了整屏 `CGDisplayBounds`，并用「macOS 本就允许浮窗压在
 * 菜单栏上」把差异合理化了。但那条只对**很矮**的浮层成立：520×440 的面板
 * 压到 Dock 底下是实打实的遮挡，而且稳定复现。
 *
 * ## 为什么单测抓不到（这条守卫存在的全部理由）
 *
 * 我先写了 6 条 `anchor_in_work_area` 的纯函数单测，全绿。然后做变异验证，
 * 把调用点改回 `display_bounds_at`（**完全恢复修复前的行为**）——
 * **6 条全过**。
 *
 * 原因：纯函数接收的是「已经取好的工作区矩形」作为入参，所以它只锁住了夹取算术，
 * 而缺陷在**数据来源**那一层。单测看着覆盖了定位逻辑，实际上一行都没碰到
 * 「去哪儿取边界」。教训与本工程既有的一致（约定 73）：守卫必须对着
 * 「这段代码出错了会怎样」写，并且**先证明自己会红**。
 *
 * ## 判据
 *  ① macOS 定位函数**不得**引用 `display_bounds_at`（整块屏）
 *  ② macOS 定位函数**必须**引用 `work_area_logical_at`
 *  ③ `work_area_logical_at` 必须用 `work_area()` 取边界，而不是 `size()`
 *     （`size()` 是整块显示器尺寸，正是要避开的那个）
 *  ④ Windows 路径仍取 `rcWork` —— 不许为了统一而把它也改掉
 */
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const SRC = join(root, 'src-tauri/src/clipboard.rs')

const problems = []
const src = readFileSync(SRC, 'utf8')

/** 取出所有同名函数的函数体 —— 两平台各有一份，必须先分清是哪一份 */
function allBodies(sig) {
  const out = []
  let from = 0
  for (;;) {
    const at = src.indexOf(sig, from)
    if (at < 0) break
    const open = src.indexOf('{', at + sig.length)
    if (open < 0) break
    let depth = 0
    let closed = false
    for (let i = open; i < src.length; i++) {
      if (src[i] === '{') depth++
      else if (src[i] === '}') {
        depth--
        if (depth === 0) {
          out.push(src.slice(open, i + 1))
          from = i + 1
          closed = true
          break
        }
      }
    }
    if (!closed) break
  }
  return out
}

// 分平台：Windows 那份带 MonitorFromPoint，macOS 那份不带。
// ⚠️ 不能取「第一个匹配」—— Windows 版在文件里排在前面，取错一份会让守卫
// 对着正确的 macOS 实现报「没走工作区」。这正是它第一版的写法，已被实测打脸。
const anchors = allBodies('fn cursor_anchor_position')
const mac = anchors.filter((b) => !b.includes('MonitorFromPoint'))
const win = anchors.filter((b) => b.includes('MonitorFromPoint'))

// ①② macOS 版：不得引用整块屏，必须引用工作区
if (mac.length !== 1) {
  problems.push(`macOS 版 cursor_anchor_position 应恰好 1 份，实得 ${mac.length} 份`)
} else if (mac[0].includes('display_bounds_at')) {
  problems.push(
    'macOS 浮层定位引用了 display_bounds_at（CGDisplayBounds 整块屏，含 Dock 与菜单栏区域）——' +
      '这正是「浮层被摆到下方、被 Dock 压住」的根因，必须改用工作区',
  )
} else if (!mac[0].includes('work_area_logical_at')) {
  problems.push('macOS 浮层定位没有引用 work_area_logical_at：边界来源需要显式走工作区')
}

// ③ 取边界的那份必须用 work_area()，不能用整块 size()
const waBodies = allBodies('fn work_area_logical_at')
if (waBodies.length !== 1) {
  problems.push(`work_area_logical_at 应恰好 1 份，实得 ${waBodies.length} 份`)
} else {
  const wa = waBodies[0]
  if (!wa.includes('work_area()')) {
    problems.push('work_area_logical_at 没有调用 work_area()：拿不到 Dock/菜单栏之外的真实可用区')
  }
  // 返回边界那几行只能来自 work_area（size() 只能用于「光标是否在这块屏上」的命中测试）
  const tail = wa.slice(wa.indexOf('let r ='))
  if (tail.length && /\.size\(\)/.test(tail)) {
    problems.push('work_area_logical_at 的返回边界取自 size()（整块屏）而不是 work_area()')
  }
}


// ④ 两平台各有一份 cursor_anchor_position：macOS 那份必须走工作区，
//    Windows 那份必须仍取 rcWork（不许为了「统一」把已经正确的那份改掉）。
{
  const bodies = allBodies('fn cursor_anchor_position')
  if (bodies.length < 2) {
    problems.push(
      `只找到 ${bodies.length} 份 cursor_anchor_position（应为两平台各一份）：` +
        '其中一份被误删或被 cfg 门控掉了',
    )
  }
  const mac = bodies.filter((b) => !b.includes('MonitorFromPoint'))
  const win = bodies.filter((b) => b.includes('MonitorFromPoint'))
  if (mac.length !== 1) {
    problems.push(`macOS 版 cursor_anchor_position 应恰好 1 份，实得 ${mac.length} 份`)
  } else if (!mac[0].includes('work_area_logical_at')) {
    problems.push('macOS 版 cursor_anchor_position 没有走 work_area_logical_at')
  }
  if (win.length !== 1) {
    problems.push(`Windows 版 cursor_anchor_position 应恰好 1 份，实得 ${win.length} 份`)
  } else if (!win[0].includes('rcWork')) {
    problems.push(
      'Windows 版 cursor_anchor_position 不再取 rcWork：工作区口径被改掉了，' +
        '「统一成一种写法」不是修复 macOS 的理由',
    )
  }
}


if (problems.length) {
  console.error('剪贴板浮层定位守卫未通过：')
  for (const p of problems) console.error(`  ✗ ${p}`)
  process.exit(1)
}
console.log('✓ 剪贴板浮层：macOS 定位夹工作区（work_area），Windows 仍夹 rcWork')
