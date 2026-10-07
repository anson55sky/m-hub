/**
 * 速记双链（v0.8.0，发布说明 ②）。
 *
 * 正文里写 `[[笔记标题]]` 即链接到另一篇笔记；本文件负责三件事：
 *
 * 1. **抽出**正文里所有 `[[...]]`（含 `|显示文字` 与 `#小节` 两种后缀）
 * 2. **建索引**：某篇笔记的出链（它引用了谁）与反链（谁引用了它）
 * 3. **解析**目标：按标题找到那篇笔记；重名 / 不存在时给出可显示的说法
 *
 * ## 为什么标题是唯一标识（而不是 id）
 *
 * 发布说明写的就是 `[[笔记标题]]`。代价是**重名无法消歧** —— 只能指向第一篇。
 * 这是刻意的：用户手写的是标题，不是数字；而且速记有「未命名时自动取正文首行
 * 当标题」这条自动派生规则，标题本身已经是稳定可读的标识。
 * 真要消歧得写成 `[[标题#2]]`，那要求用户记住编号 —— 不做。
 */

/** 一条出链的解析结果。 */
export interface WikiLink {
  /** 用户写的那一段（含可能的 `|文字` 与 `#小节`），用于回显与定位。 */
  raw: string
  /** 目标笔记标题（已去掉 `|文字` 与 `#小节`）。 */
  target: string
  /** `|显示文字` 里的自定义文字；没有则为空串。 */
  label: string
  /** `#小节` 里的锚点文字；没有则为空串。 */
  anchor: string
  /** 正文中的字符偏移，用于把编辑器光标定位回这一处。 */
  offset: number
}

/**
 * 匹配 `[[...]]`。
 *
 * ⚠️ 三处刻意的边界：
 *
 * - **不跨行**：`[^\]\n]*`。`[[` 在一行、`]]` 在下一行时那是两个残留标记，
 *   不是链接。跨行匹配会把中间整段正文吞成「标题」，用户看到的是
 *   「这篇笔记的某条链接标题变成了一整段话」。
 * - **不吃 `]]` 之外的 `]`**：`[^\]]*` 而不是 `.*?`。用 `.*?` 的话
 *   `[[a](b)]]` 这种畸形会被匹配成 `a](b)`。
 * - **`[[` 后面必须非空**：`[^\]]+`。`[[]]` 是用户打了一半，不是链接。
 *   空标题进索引会让「引用一条不存在的笔记」变成「引用自己」。
 */
const WIKI_RE = /\[\[([^\]\n]+)\]\]/g

/**
 * 从 `[[目标|显示文字#小节]]` 里拆出三部分。
 *
 * ## ⚠️ `[[A#B]]` 本身是**歧义**的
 *
 * 它可以读成「笔记 A 的小节 B」（wiki 惯例），也可以读成「标题就叫 `A#B`」
 * —— `[[C# 学习笔记]]` 就是后者，但按惯例会被拆成 `C` + 锚点 `学习笔记`，
 * 于是链接指向一篇不存在的笔记、界面上什么都不显示（静默失败）。
 *
 * **判据：可预测 > 猜得准。** 所以按 wiki 惯例拆（`#` 一律当锚点），
 * 而标题里真的含 `#` 时用**带引号形式** `[["C# 学习笔记"]]` ——
 * 引号包**整个** inner，内容整体是标题，不做任何拆分。
 *
 * ⚠️ 引号形式**不能**再带 `|显示文字`：`[["C# 笔记"|显示]]` 的 inner 是
 *   `"C# 笔记"|显示`，首尾不是同一对引号，于是退化成按惯例拆 → 目标 `"C# 笔记"`。
 *   要显示文字就别用引号形式（标题里没 `#` 时本来也不需要）。
 *   这是**刻意**的简化：引号 + 显示文字同时需要的场景极少，而支持它要让
 *   语法出现「引号可以只包一部分」的歧义。
 *
 * ⚠️ 标题里含 `]` 的**不支持**。正则 `[^\]\n]+` 会在第一个 `]]` 处收口，
 *   于是 `[[数组 Array[]] 笔记]]` 匹配成目标 `数组 Array[` —— 一条指向不存在
 *   笔记的链接。这比「整条不匹配」好：用户能看到自己写错了标记，而不是
 *   看到一段原样文本却不知道哪里出了问题。
 */
function parseInner(inner: string): { target: string; label: string; anchor: string } {
  const t = inner.trim()
  // 带引号形式：整体当标题
  if (t.length >= 2 && t.startsWith('"') && t.endsWith('"')) {
    return { target: t.slice(1, -1).trim(), label: '', anchor: '' }
  }
  let rest = t
  let label = ''
  let anchor = ''
  // `|` 只切一次：显示文字里可以再写 `|`，但那是文字不是分隔符
  const bar = rest.indexOf('|')
  if (bar >= 0) {
    label = rest.slice(bar + 1).trim()
    rest = rest.slice(0, bar)
  }
  // `#` 取**最后一个**：wiki 惯例是「目标#小节」
  const hash = rest.lastIndexOf('#')
  if (hash > 0) {
    anchor = rest.slice(hash + 1).trim()
    rest = rest.slice(0, hash)
  }
  return { target: rest.trim(), label, anchor }
}

/** 抽出正文里的全部双链，按出现顺序。 */
export function extractWikiLinks(content: string): WikiLink[] {
  const out: WikiLink[] = []
  // ⚠️ 正则带 /g 有 lastIndex 状态，复用前必须新建或重置
  const re = new RegExp(WIKI_RE.source, 'g')
  let m: RegExpExecArray | null
  while ((m = re.exec(content)) !== null) {
    const { target, label, anchor } = parseInner(m[1])
    // `[[]]` / `[[|文字]]` / `[[""]]`：打了一半，不是链接。
    // ⚠️ 空目标绝不能进索引 —— 它会命中「未命名笔记」，而未命名笔记的
    //   标题是自动派生的（见 normalizeTitle），于是「引用一条不存在的笔记」
    //   悄悄变成了「引用自己」。
    if (!target) continue
    out.push({ raw: m[0], target, label, anchor, offset: m.index })
  }
  return out
}

/** 一篇笔记的入链/出链索引条目。 */
export interface BacklinkEntry {
  id: number
  title: string
  /** 该笔记的正文片段（含被引用的那一处上下文），供列表预览。 */
  snippet: string
  /** 被引用的那几处在正文里的偏移，编辑器点击时可逐个跳转。 */
  offsets: number[]
}

export interface BacklinkIndex {
  /** noteId → 它引用了哪些笔记（出链）。 */
  outgoing: Map<number, WikiLink[]>
  /** noteId → 哪些笔记引用了它（反链）。 */
  incoming: Map<number, BacklinkEntry[]>
  /** 标题（小写、trim 后）→ 命中的笔记 id 列表。用于消歧统计。 */
  byTitle: Map<string, number[]>
  /** 被引用但**找不到**的目标标题。界面上要交代，不能静默。 */
  dangling: Map<string, number>
}

export interface NoteLike {
  id: number
  title: string
  content: string
}

/** 标题归一：与 Rust 侧 `normalize_title`（trim + 折叠空白）保持同口径。 */
export function normalizeTitle(t: string): string {
  return t.replace(/\s+/g, ' ').trim()
}

/**
 * 建全库双链索引。
 *
 * @param notes 参与索引的笔记（应只含**未删除**的 —— 回收站里的笔记
 *   不该出现在别人的反链里，否则用户点进去看到的是一篇「已删除」）
 * @param snippetRadius 摘要半径（字符）
 */
export function buildBacklinkIndex(notes: NoteLike[], snippetRadius = 40): BacklinkIndex {
  const index: BacklinkIndex = {
    outgoing: new Map(),
    incoming: new Map(),
    byTitle: new Map(),
    dangling: new Map(),
  }

  // 先建标题表：重名要**全部**记下来（消歧统计要用），解析时取第一个
  for (const n of notes) {
    const key = normalizeTitle(n.title).toLowerCase()
    if (!key) continue // 未命名笔记不作为链接目标（见下）
    const list = index.byTitle.get(key)
    if (list) list.push(n.id)
    else index.byTitle.set(key, [n.id])
  }

  for (const n of notes) {
    const links = extractWikiLinks(n.content)
    if (links.length) index.outgoing.set(n.id, links)

    for (const l of links) {
      const key = normalizeTitle(l.target).toLowerCase()
      const hits = index.byTitle.get(key)
      if (!hits || hits.length === 0) {
        // 引用了不存在的笔记 —— 界面必须交代（「有 1 条指向已删除的笔记」），
        // 静默丢弃的话用户会以为是自己写错了标记
        index.dangling.set(l.target, (index.dangling.get(l.target) ?? 0) + 1)
        continue
      }
      const targetId = hits[0]
      const entry: BacklinkEntry = {
        id: n.id,
        title: n.title,
        snippet: snippetAround(n.content, l.offset, snippetRadius),
        offsets: [l.offset],
      }
      const list = index.incoming.get(targetId)
      if (list) {
        // 同一篇引用同一目标多次 → 合并成一条、记多个偏移。
        // 分成多条会让右侧面板出现「会议记录 ×3」，而那是同一篇。
        const dup = list.find((e) => e.id === n.id)
        if (dup) {
          dup.offsets.push(l.offset)
          dup.snippet = snippetAround(
            n.content,
            Math.min(dup.offsets[0], l.offset),
            snippetRadius,
          )
        } else {
          list.push(entry)
        }
      } else {
        index.incoming.set(targetId, [entry])
      }
    }
  }
  return index
}

/** 取 `offset` 前后各若干字符作摘要，并把 `[[…]]` 里的标记**去掉**（界面上不显示标记）。 */
export function snippetAround(content: string, offset: number, radius: number): string {
  const start = Math.max(0, offset - radius)
  const end = Math.min(content.length, offset + radius)
  let s = content.slice(start, end)
  // 摘要里的 `[[笔记标题]]` 应显示成 `笔记标题` —— 用户看到的是正文渲染后的样子
  s = s.replace(/\[\[([^\]\n]+)\]\]/g, (_m, inner: string) => {
    const { target, label } = parseInner(inner)
    return label || target
  })
  return (start > 0 ? '…' : '') + s.replace(/\s+/g, ' ').trim() + (end < content.length ? '…' : '')
}

/** 标题重名的提示文案；无重名返回空串。 */
export function titleAmbiguity(index: BacklinkIndex, target: string): string {
  const hits = index.byTitle.get(normalizeTitle(target).toLowerCase()) ?? []
  return hits.length > 1 ? `（有 ${hits.length} 篇同名笔记，链接指向第一篇）` : ''
}
