/**
 * 更新说明（`RELEASE_NOTES.md` 的一节）的结构化解析。
 *
 * ## 为什么要有它
 *
 * 更新弹窗原先把整段 notes 当**纯文本**塞进一个 `white-space: pre-wrap` 的
 * 容器。于是 `## 新增`、`- 某条修复`、`**重点**` 这些标记原样显示 ——
 * 「标题、列表、重点」全都不分明，正是发布说明要修的那条。
 *
 * ## 为什么不引 Markdown 渲染器
 *
 * 工程里唯一的 Markdown 渲染是 Milkdown（编辑器）那条链，为一段纯文本说明
 * 把整套编辑器拉进更新弹窗的包里不划算；而任何「字符串 → HTML」的第三方
 * 渲染器都要另配消毒（这份 notes 来自远端更新清单，是**不可信输入**）。
 * 自己按行拆成三类块、用 Vue 模板渲染，天然没有 HTML 注入面 ——
 * **全文没有一个 v-html**。
 *
 * ## 只认三种结构，不做通用 Markdown
 *
 * 发布说明的写法是固定的（`# vX.Y.Z` + `## 新增/优化/修复` + `- 条目`）。
 * 认不出的行按**段落**原样显示，而不是猜它是标题还是列表 ——
 * 猜错会让一行字突然变成大字，比不排版更难读。
 */

export type NoteBlock =
  | { kind: 'h'; text: string; level: 1 | 2 | 3 }
  | { kind: 'li'; text: string; level: number }
  | { kind: 'p'; text: string }

/** 最多支持三级标题；`####` 及更深的当作三级（再深就没有视觉层级了） */
function headingLevel(hashes: number): 1 | 2 | 3 {
  return hashes <= 1 ? 1 : hashes === 2 ? 2 : 3
}

/**
 * 把一段 notes 拆成结构块。
 *
 * ⚠️ 空行被丢弃而不是保留成空段落：它们在原文里只是 Markdown 的分段符，
 *   保留下来会在块之间多出一堆看不见的间距（`gap` 已经管了分段）。
 */
export function parseNotes(raw: string): NoteBlock[] {
  const out: NoteBlock[] = []
  for (const rawLine of raw.split('\n')) {
    const line = rawLine.replace(/\s+$/, '')
    if (!line.trim()) continue

    const h = /^(#{1,6})\s+(.*)$/.exec(line)
    if (h) {
      out.push({ kind: 'h', text: h[2]!.trim(), level: headingLevel(h[1]!.length) })
      continue
    }

    // 列表：`- ` / `* ` / `+ `，缩进决定层级（发布说明最多两级）
    const li = /^(\s*)[-*+]\s+(.*)$/.exec(line)
    if (li) {
      out.push({
        kind: 'li',
        text: li[2]!.trim(),
        // ⚠️ 按**缩进宽度**算层级，不按行数：一行写 4 空格和两行各 2 空格等价
        level: Math.min(Math.floor(li[1]!.length / 2), 2),
      })
      continue
    }

    out.push({ kind: 'p', text: line.trim() })
  }
  return out
}

/** 段内的一小段：`{ bold: true }` 表示该段要加粗 */
export interface InlineSeg {
  text: string
  bold: boolean
}

/**
 * 把一行拆成普通 / 加粗两三种，交替渲染（`**重点**`）。
 *
 * ⚠️ 只认成对的 `**`：`****` 或只有一个 `**` 的行原样保留 ——
 *   猜着补一个会把「C++ 里 a**b」这种正文改成加粗，那才叫改坏。
 */
export function parseInline(text: string): InlineSeg[] {
  const out: InlineSeg[] = []
  const re = /\*\*([^*]+)\*\*/g
  let last = 0
  let m: RegExpExecArray | null
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) out.push({ text: text.slice(last, m.index), bold: false })
    out.push({ text: m[1]!, bold: true })
    last = m.index + m[0].length
  }
  if (last < text.length) out.push({ text: text.slice(last), bold: false })
  return out.length ? out : [{ text, bold: false }]
}