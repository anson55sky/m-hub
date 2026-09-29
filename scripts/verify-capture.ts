/**
 * `capture.ts` 的验证脚本（不进 prebuild，手动跑）。
 *
 * 跑法：node --experimental-strip-types scripts/verify-capture.ts
 *
 * ## 为什么这个文件比一般的验证脚本更认真
 *
 * 时间解析是**会写错用户数据**的那一类：把「3月15日」记成 12 月 3 日，
 * 用户要靠自己才能发现。所以这里的期望值一律写成**绝对时间戳**
 * （基于一个固定的 `now`），而不是「应该大于今天」这种模糊断言 ——
 * 后者在时区、夏令时、跨月边界下都可能蒙对，测了等于没测。
 */
import { parseCapture, DEST_LABEL } from '../src/utils/capture.ts'

// 基准时刻：2026-09-29 10:30 本地时间（周二）
// 选周二上午是为了让「今天/明天/下周一」的换算结果一眼可核对。
const NOW = new Date(2026, 8, 29, 10, 30, 0, 0)
const ts = (y: number, m: number, d: number, hh: number, mm = 0) =>
  new Date(y, m, d, hh, mm, 0, 0).getTime()

let failed = 0
function eq(name: string, actual: unknown, expected: unknown) {
  const a = JSON.stringify(actual)
  const e = JSON.stringify(expected)
  if (a === e) console.log(`  ✓ ${name}`)
  else {
    console.error(`  ✗ ${name}\n      实际 ${a}\n      期望 ${e}`)
    failed++
  }
}
function isTs(name: string, actual: number | null, expected: number) {
  if (actual === expected) console.log(`  ✓ ${name}`)
  else {
    const d = actual == null ? 'null' : new Date(actual).toLocaleString('zh-CN')
    console.error(`  ✗ ${name}\n      实际 ${d}\n      期望 ${new Date(expected).toLocaleString('zh-CN')}`)
    failed++
  }
}

console.log(`基准 now = ${NOW.toLocaleString('zh-CN')}\n`)

// ---- 路由 -----------------------------------------------------------------
console.log('① 去向判断')
eq('纯文字 → 速记', parseCapture('买牛奶', NOW).dest, 'note')
eq('! 前缀 → 速记', parseCapture('!买牛奶', NOW).dest, 'note')
eq('? 前缀 → 提示词', parseCapture('?常用签名', NOW).dest, 'snippet')
eq('> 前缀 → 待办', parseCapture('>交周报', NOW).dest, 'todo')
eq('http 开头 → 速达', parseCapture('https://example.com', NOW).dest, 'url')
eq('有裸域名不算网址（宁可不猜）', parseCapture('example.com', NOW).dest, 'note')
eq('带时间且有正文 → 待办', parseCapture('明天下午3点交周报', NOW).dest, 'todo')
eq('只有时间没正文 → 速记（不生成空待办）', parseCapture('明天下午3点', NOW).dest, 'note')

// ---- 相对时长 -------------------------------------------------------------
console.log('\n② 相对时长')
isTs('30分钟后', parseCapture('30分钟后交周报', NOW).dueAt, NOW.getTime() + 30 * 60_000)
isTs('2小时后', parseCapture('2 小时后起来', NOW).dueAt, NOW.getTime() + 2 * 3_600_000)
isTs('三天后', parseCapture('三天后复查', NOW).dueAt, NOW.getTime() + 3 * 86_400_000)
isTs('一周后', parseCapture('一周后跟进', NOW).dueAt, NOW.getTime() + 7 * 86_400_000)
eq('相对时长后正文保留', parseCapture('30分钟后交周报', NOW).text, '交周报')

// ---- 今天/明天/后天 -------------------------------------------------------
console.log('\n③ 今天 / 明天 / 后天')
isTs('今天下午3点 → 15:00', parseCapture('今天下午3点交周报', NOW).dueAt, ts(2026, 8, 29, 15, 0))
isTs('明天上午9:30', parseCapture('明天上午9:30 面试', NOW).dueAt, ts(2026, 8, 30, 9, 30))
isTs('后天晚上8点 → 20:00', parseCapture('后天晚上8点吃饭', NOW).dueAt, ts(2026, 8, 31, 20, 0))
isTs('明天下午3点半 → 15:30', parseCapture('明天下午3点半交周报', NOW).dueAt, ts(2026, 8, 30, 15, 30))
isTs('明天不带时刻 → 9:00（不是 0 点）', parseCapture('明天交周报', NOW).dueAt, ts(2026, 8, 30, 9, 0))
isTs('中午12点 → 12:00', parseCapture('明天中午12点吃饭', NOW).dueAt, ts(2026, 8, 30, 12, 0))
isTs('下午12点 → 12:00（不是 0 点）', parseCapture('明天下午12点吃饭', NOW).dueAt, ts(2026, 8, 30, 12, 0))
isTs('凌晨12点 → 0 点', parseCapture('明天凌晨12点做饭', NOW).dueAt, ts(2026, 8, 30, 0, 0))
isTs('上午12点 → 12:00', parseCapture('明天上午12点开会', NOW).dueAt, ts(2026, 8, 30, 12, 0))

// ---- 星期 -----------------------------------------------------------------
console.log('\n④ 星期（基准是周二）')
isTs('下周一 → 10/05', parseCapture('下周一交周报', NOW).dueAt, ts(2026, 9, 5, 9, 0))
isTs('本周一（已过）→ 顺延到 10/05', parseCapture('本周一开会', NOW).dueAt, ts(2026, 9, 5, 9, 0))
isTs('周三（还没到）→ 9/30', parseCapture('周三面试', NOW).dueAt, ts(2026, 8, 30, 9, 0))
// 基准 2026-09-29 是周二，+3 天 = 10-02（周五）。
// ⚠️ 别写成 ts(2026, 8, 31, …)：9 月没有 31 日，Date 会静默顺延成 10-01，
// 断言就变成了「周五 = 10 月 1 日」这种自相矛盾的东西还照样跑。
isTs('周五下午2点 → 10/02', parseCapture('周五下午2点复盘', NOW).dueAt, ts(2026, 9, 2, 14, 0))
isTs('下周日 → 10/04（周二 +5 天）', parseCapture('下周日大扫除', NOW).dueAt, ts(2026, 9, 4, 9, 0))

// ---- 具体日期 -------------------------------------------------------------
console.log('\n⑤ 具体月日')
isTs('3月15日 → 明年（今年已过）', parseCapture('3月15日体检', NOW).dueAt, ts(2027, 2, 15, 9, 0))
isTs('12月3日 → 今年（还没到）', parseCapture('12月3日取快递', NOW).dueAt, ts(2026, 11, 3, 9, 0))
isTs('10月5日下午4点', parseCapture('10月5日下午4点牙医', NOW).dueAt, ts(2026, 9, 5, 16, 0))

// ---- 只有时刻 -------------------------------------------------------------
console.log('\n⑥ 只有时刻')
isTs('下午3点（今天还没到）', parseCapture('下午3点交周报', NOW).dueAt, ts(2026, 8, 29, 15, 0))
isTs('上午9点（今天已过）→ 明天', parseCapture('上午9点交周报', NOW).dueAt, ts(2026, 8, 30, 9, 0))

// ---- 认不出来就不猜（最重要的不变量）------------------------------------
console.log('\n⑦ 认不出就不猜 —— 本次设计的核心约束')
for (const s of ['买牛奶', '1+1', 'TODO: 买牛奶', '明天', '3', '哈哈哈哈哈']) {
  const p = parseCapture(s, NOW)
  eq(`「${s}」不产生 dueAt`, p.dueAt, null)
}
eq('「明天」不产生空待办', parseCapture('明天', NOW).dest, 'note')
eq('「明天」原文保留（没被吃掉）', parseCapture('明天', NOW).text, '明天')

// ---- 倒计时前缀 -----------------------------------------------------------
console.log('\n⑧ 倒计时前缀')
eq('~ 后跟时间 → 倒计时', parseCapture('~30分钟后', NOW).dest, 'countdown')
isTs('~30分钟后 的到期时刻', parseCapture('~30分钟后', NOW).dueAt, NOW.getTime() + 30 * 60_000)
eq('~30分钟（缺「后」）不算时间 → 退回速记', parseCapture('~30分钟', NOW).dest, 'note')
eq('~ 后跟非时间 → 退回速记', parseCapture('~泡茶', NOW).dest, 'note')
eq('~ 退回时原文保留', parseCapture('~泡茶', NOW).text, '泡茶')

// ---- 边界 -----------------------------------------------------------------
console.log('\n⑨ 边界')
eq('空输入', parseCapture('', NOW).dest, 'note')
eq('纯空白', parseCapture('   ', NOW).text, '')
eq('前后空白被裁掉', parseCapture('  买牛奶  ', NOW).text, '买牛奶')
eq('时间表达后残留的标点被清理', parseCapture('明天9点，开会', NOW).text, '开会')
eq('落点名称齐全', Object.keys(DEST_LABEL).sort(), ['countdown', 'note', 'snippet', 'todo', 'url'])

console.log(failed ? `\n失败 ${failed} 条` : '\n全部通过')
process.exit(failed ? 1 : 0)
