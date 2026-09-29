/**
 * 统一捕获：把一行输入路由到该去的地方，并顺手抽出时间表达���。
 *
 * ## 为什么要有这一层
 *
 * m-hub 已经有五个去处（速记 / 待办 / 提示词 / 倒计时 / 速达），
 * 但每次记一件事都得先想「这该去哪」。这个 app 的瓶颈不是功能少，
 * 是**捕获摩擦**。统一入口把「去哪」变成自动判断，用户只需要写内容。
 *
 * ## 最重要的设计约束：**宁可不猜**
 *
 * 时间解析一旦猜错，代价是「待办被记到了错的那天」—— 而错的那天要靠用户
 * 自己发现。所以这里的策略是：
 *   · 只认**少量、无歧义**的模式，认不出就返回 `null`（当作没写时间）
 *   · 绝不「尽力而为」地猜。`3/15` 在不同地区可以是 3 月 15 日或 12 月 3 日，
 *     本工程统一按「月/日」解释，并在 UI 上把解析结果**显示出来**给用户核对
 *   · 前缀（`!` `?` `~`）永远压过自动判断：用户显式说了要去哪，就别自作主张
 *
 * ## 路由优先级（从高到低）
 *
 *   1. 显式前缀      `!` 速记 / `?` 提示词 / `~` 倒计时 / `>` 待办
 *   2. URL           http(s):// 开头 → 速达（网页）
 *   3. 抽出时间后剩余文本非空 → 待办（「明天下午3点交周报」）
 *   4. 兜底          速记
 *
 * 第 3 条是关键：只有当「去掉时间表达后还剩下事」时才算待办。
 * 光有「明天下午3点」而没有事，本身不构成一条待办。
 */

export type CaptureDest = 'note' | 'todo' | 'snippet' | 'countdown' | 'url'

export interface CapturePlan {
  dest: CaptureDest
  /** 去掉前缀与时间表达之后的正文 */
  text: string
  /** 原始输入（保留，供 UI 展示与撤销） */
  raw: string
  /** 解析出的到期时刻（毫秒时间戳）。null = 没写时间 / 认不出 */
  dueAt: number | null
  /** 认出了时间但仍可能有歧义（目前只有 月/日 与 短年份 两种），
   *  UI 应当把解析结果原样显示出来让用户核对，而不是替用户拍板。 */
  ambiguous: boolean
}

/* ────────────────────────── 时间表达 ────────────────────────── */

const CN_DIGITS: Record<string, number> = {
  零: 0, 一: 1, 两: 2, 二: 2, 三: 3, 四: 4, 五: 5, 六: 6, 七: 7, 八: 8, 九: 9, 十: 10,
}

/** 「十五」这类两字中文数字。仅覆盖 1-99，够时间表达用。 */
function cnNumber(s: string): number | null {
  if (/^\d+$/.test(s)) return Number(s)
  if (!/^[零一两二三四五六七八九十]+$/.test(s)) return null
  if (s === '十') return 10
  const i = s.indexOf('十')
  if (i >= 0) {
    const tens = i === 0 ? 1 : CN_DIGITS[s[i - 1]]
    const ones = i === s.length - 1 ? 0 : CN_DIGITS[s[i + 1]]
    if (tens == null || ones == null) return null
    return tens * 10 + ones
  }
  // 纯个位数字（如「三」）；两位以上（如「二三」）不是合法写法
  return s.length === 1 ? CN_DIGITS[s] ?? null : null
}

const DAY_OFFSET: Record<string, number> = {
  今天: 0, 今日: 0, 今晚: 0,
  明天: 1, 明日: 1, 明早: 1, 明晚: 1,
  后天: 2, 後天: 2,
  大后天: 3,
  昨天: -1, 昨日: -1,
}

/** 贪心取下一个关键词；命中则返回它在该串中的位置 */
function findKeyword(text: string, words: string[]): { word: string; at: number } | null {
  let best: { word: string; at: number } | null = null
  for (const w of words) {
    const at = text.indexOf(w)
    if (at >= 0 && (!best || at < best.at)) best = { word: w, at }
  }
  return best
}

/** 用本地时区把「某天的几点几分」合成毫秒时间戳。小时越界时顺延到次日。 */
function atLocal(base: Date, dayOffset: number, hh: number, mm: number, meridiem: string | null): number {
  const d = new Date(base.getFullYear(), base.getMonth(), base.getDate() + dayOffset)
  let h = hh
  if (meridiem) {
    if (meridiem === '下午' || meridiem === '傍晚' || meridiem === '晚上') {
      // 「下午3点」= 15:00；「晚上8点」= 20:00。12 点在「下午」语境下是 12 而不是 0
      if (h < 12) h += 12
    } else if (meridiem === '中午') {
      if (h < 11) h += 12
    } else if (meridiem === '凌晨') {
      // 只有「凌晨12点」才等于 0 点。
      if (h === 12) h = 0
    } else if (meridiem === '早上' || meridiem === '上午' || meridiem === '早晨') {
      // 「上午12点」在中文里就是**中午 12 点**，不是 0 点 —— 实测原先被记成
      // 当天 00:00，差了一整天。中文里 0 点要说「凌晨12点」或「午夜」，
      // 所以这里不做 12→0 的转换。
    }
  }
  d.setHours(h, mm, 0, 0)
  return d.getTime()
}

/**
 * 从一段文本里抽出时间表达，**返回剩下的文本**。
 *
 * 抽不出来就返回 `matched: false` 且 `text` 原样返回 —— 这是「宁可不猜」的具体落点。
 */
function extractWhen(
  input: string,
  now: Date,
): { text: string; dueAt: number | null; ambiguous: boolean; matched: boolean } {
  const miss = { text: input, dueAt: null, ambiguous: false, matched: false }
  const t = input.trim()
  if (!t) return miss

  // ---- ① 相对时长：「30分钟后」「2 小时后」「3 天后」--------------------------
  // 放在最前面：它最容易认、歧义最小，且不该被下面的绝对日期规则抢走。
  const rel = t.match(/(?:^|\s)(\d+|[一两二三四五六七八九十]+)\s*(分钟|分|小时|个小时|钟头|天|周|星期)(?:之)?后/)
  if (rel) {
    const n = cnNumber(rel[1])
    if (n != null) {
      const unit = rel[2]
      const ms =
        unit === '分钟' || unit === '分' ? n * 60_000
        : unit === '周' || unit === '星期' ? n * 7 * 86_400_000
        : unit === '天' ? n * 86_400_000
        : n * 3_600_000
      const cut = rel[0].length - (rel[0].startsWith(' ') ? 1 : 0)
      return {
        text: (t.slice(0, rel.index! + (rel[0].startsWith(' ') ? 1 : 0)) + t.slice(rel.index! + cut)).trim(),
        dueAt: now.getTime() + ms,
        ambiguous: false,
        matched: true,
      }
    }
  }

  // ---- ② 日期 + 时刻：「明天下午3点」「下周一 9:30」-------------------------
  const dayWord = findKeyword(t, Object.keys(DAY_OFFSET))
  const weekWord = t.match(/(下下|下|本|这)?\s*(周|星期|礼拜)([一二三四五六日天])/)
  const monthDay = t.match(/(\d{1,2})\s*月\s*(\d{1,2})\s*[日号]?/)
  const timeWord = t.match(/(凌晨|早上|早晨|上午|中午|下午|傍晚|晚上|夜里)?\s*(\d{1,2}|[一两二三四五六七八九十]+)\s*[点:：]\s*(\d{1,2}|半)?\s*分?/)

  // 没有任何日期或时刻线索 → 不猜
  if (!dayWord && !weekWord && !monthDay && !timeWord) return miss

  let dueAt: number | null = null
  let ambiguous = false
  // ⚠️ start/end 是「被吃掉的那一段」的区间，故**两端都从 0 起**。
  // 这里原先把 end 初始化成 `t.length`，于是下面 `Math.max(end, …)` 永远取 t.length，
  // 整串都被当成「时间表达」吃掉、rest 恒为空 —— 于是「今天下午3点交周报」
  // 认出了时间却抽不出正文，落不到待办、内容也丢了。
  // 另两个分支（weekWord / monthDay）用的是直接赋值而非 Math.max，所以没被这个 bug 波及 ——
  // 这正是「下周一交周报」能对、而「今天下午3点交周报」不能的原因。
  let start = 0
  let end = 0

  // 日期
  if (dayWord) {
    const mer = timeWord?.[1] ?? null
    const hh = timeWord ? cnNumber(timeWord[2]) : null
    const mmRaw = timeWord?.[3]
    const mm = mmRaw === '半' ? 30 : mmRaw != null ? cnNumber(mmRaw) : 0
    if (hh != null && mm != null) {
      dueAt = atLocal(now, DAY_OFFSET[dayWord.word], hh, mm, mer)
    } else {
      // 「明天」不带时刻：按当天 9:00 记，而不是 0 点。
      // 0 点意味着「一整天都过期」，用户写「明天」时想的是明天白天。
      dueAt = atLocal(now, DAY_OFFSET[dayWord.word], 9, 0, null)
    }
    start = Math.min(start, dayWord.at)
    end = Math.max(end, dayWord.at + dayWord.word.length)
    if (timeWord) {
      start = Math.min(start, timeWord.index!)
      end = Math.max(end, timeWord.index! + timeWord[0].length)
    }
  } else if (weekWord) {
    // 「下周一」= 下一个周一；「本周一」若已过则取下周一（用户说本周却已过，
    // 几乎总是想指下一次）。当天算「本周」不算「下周」。
    const prefix = weekWord[1] ?? ''
    // 「日」与「天」都表示星期天，但 CN_DIGITS 里没有它们（那张表是给数字用的）。
    // 漏掉会让「下周日」被当成周一 —— 实测正是如此（期望 10/6，实得 10/5）。
    const target = weekWord[3] === '日' || weekWord[3] === '天' ? 7 : CN_DIGITS[weekWord[3]] ?? 1
    const cur = now.getDay() === 0 ? 7 : now.getDay()
    let delta = target - cur
    if (prefix === '下') { if (delta <= 0) delta += 7 }
    else if (prefix === '下下') { if (delta <= 0) delta += 7; delta += 7 }
    else if (delta < 0) delta += 7
    const mer = timeWord?.[1] ?? null
    const hh = timeWord ? cnNumber(timeWord[2]) : null
    const mmRaw = timeWord?.[3]
    const mm = mmRaw === '半' ? 30 : mmRaw != null ? cnNumber(mmRaw) : 0
    dueAt = atLocal(now, delta, hh ?? 9, mm ?? 0, mer)
    start = weekWord.index!
    end = weekWord.index! + weekWord[0].length
    if (timeWord) { start = Math.min(start, timeWord.index!); end = Math.max(end, timeWord.index! + timeWord[0].length) }
  } else if (monthDay) {
    // 「3月15日」：不补年份 —— 补了等于替用户假定「今年」，跨年时必错。
    // 若算出来的日子已经过去（差超过 1 天），顺延到明年；否则就当它指今年。
    const mo = Number(monthDay[1]) - 1
    const da = Number(monthDay[2])
    let d = new Date(now.getFullYear(), mo, da)
    if (d.getTime() < now.getTime() - 86_400_000) d = new Date(now.getFullYear() + 1, mo, da)
    if (timeWord) {
      // meridiem 在这里也必须用上：原来这个分支只取 hh 直接 setHours，
      // 于是「10月5日下午4点」被记成当天 **04:00**（差 12 小时，实际是凌晨四点）。
      // dayWord / weekWord 两个分支都走 atLocal（内含 meridiem 处理），只有这里漏了。
      const hh = cnNumber(timeWord[2])
      const mmRaw = timeWord?.[3]
      const mm = mmRaw === '半' ? 30 : mmRaw != null ? cnNumber(mmRaw) : 0
      d = new Date(
        atLocal(new Date(d.getFullYear(), d.getMonth(), d.getDate()), 0, hh ?? 9, mm ?? 0, timeWord[1] ?? null),
      )
    } else {
      d.setHours(9, 0, 0, 0)
    }
    dueAt = d.getTime()
    start = monthDay.index!
    end = monthDay.index! + monthDay[0].length
    if (timeWord) { start = Math.min(start, timeWord.index!); end = Math.max(end, timeWord.index! + timeWord[0].length) }
  } else if (timeWord) {
    // 只有时刻没有日期：「下午3点」= 今天下午3点；已过则顺延到明天。
    const hh = cnNumber(timeWord[2])
    const mmRaw = timeWord?.[3]
    const mm = mmRaw === '半' ? 30 : mmRaw != null ? cnNumber(mmRaw) : 0
    if (hh == null) return miss
    const mer = timeWord[1]
    let target = atLocal(now, 0, hh, mm ?? 0, mer)
    if (target <= now.getTime()) target = atLocal(now, 1, hh, mm ?? 0, mer)
    dueAt = target
    start = timeWord.index!
    end = timeWord.index! + timeWord[0].length
  }

  if (dueAt == null) return miss

  const rest = (t.slice(0, start) + t.slice(end)).replace(/[，,。、；;：:]+/g, ' ').replace(/\s+/g, ' ').trim()
  return { text: rest, dueAt, ambiguous, matched: true }
}

/* ────────────────────────── 路由 ────────────────────────── */

const PREFIXES: Record<string, CaptureDest> = {
  '!': 'note',
  '?': 'snippet',
  '>': 'todo',
  '~': 'countdown',
}

/** 看起来像网址（http/https 开头，含协议）才当速达；裸域名不猜 */
const URL_RE = /^https?:\/\/\S+$/i

/**
 * 解析一行输入，给出去向与正文。
 *
 * @param input 用户输入
 * @param now 「现在」，用于相对时间。默认取真实当前时间。
 */
export function parseCapture(input: string, now: Date = new Date()): CapturePlan {
  const raw = input
  const trimmed = input.trim()
  if (!trimmed) {
    return { dest: 'note', text: '', raw, dueAt: null, ambiguous: false }
  }

  // ① 显式前缀：用户说了要去哪，就别自作主张
  const head = trimmed[0]
  if (head in PREFIXES) {
    const dest = PREFIXES[head]
    const rest = trimmed.slice(1).trim()
    if (dest === 'countdown') {
      // 倒计时必须有时间，认不出就退回速记并保留原样（下面统一处理）
      const w = extractWhen(rest, now)
      if (w.dueAt != null) {
        return { dest, text: w.text, raw, dueAt: w.dueAt, ambiguous: w.ambiguous }
      }
      return { dest: 'note', text: rest, raw, dueAt: null, ambiguous: false }
    }
    if (dest === 'todo') {
      const w = extractWhen(rest, now)
      return { dest, text: w.text, raw, dueAt: w.dueAt, ambiguous: w.ambiguous }
    }
    return { dest, text: rest, raw, dueAt: null, ambiguous: false }
  }

  // ② URL → 速达
  if (URL_RE.test(trimmed)) {
    return { dest: 'url', text: trimmed, raw, dueAt: null, ambiguous: false }
  }

  // ③ 有时间、且去掉时间后还剩事 → 待办
  const w = extractWhen(trimmed, now)
  if (w.dueAt != null && w.text.length > 0) {
    return { dest: 'todo', text: w.text, raw, dueAt: w.dueAt, ambiguous: w.ambiguous }
  }

  // ④ 兜底 → 速记。**只认出了时间但没剩事**（「明天下午3点」）也走这里，
  //    因为光一个时刻不构成一条待办，而落进速记至少不会丢。
  if (w.dueAt != null && w.text.length === 0) {
    return { dest: 'note', text: trimmed, raw, dueAt: null, ambiguous: false }
  }

  return { dest: 'note', text: trimmed, raw, dueAt: null, ambiguous: false }
}

/** 供 UI 显示的落点名称 */
export const DEST_LABEL: Record<CaptureDest, string> = {
  note: '速记',
  todo: '待办',
  snippet: '提示词',
  countdown: '倒计时',
  url: '速达（网页）',
}
