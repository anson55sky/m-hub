/**
 * 笔记编辑器「一键美化」（v0.8.0，发布说明 ⑥）。
 *
 * 三件事：**统一列表符号**、**清理多余空行**、**中英文之间加空格**。
 *
 * ## 最大的坑：中英文加空格**不能**无脑做
 *
 * 「`你好 world`」比「`你好world`」好读，这是排版惯例。但同一条规则作用在
 * 这些地方全是错的：
 *
 * | 位置 | 为什么错 | 症状 |
 * |---|---|---|
 * | 行内代码 / 围栏代码块 | 代码里的空格是**语义** | `int x` → `intx`，直接编译不过 |
 * | Markdown 链接的 URL | `?a=1&b=2` 里加空格 → 链接失效 | 点开是 404 |
 * | Markdown 链接的**文字** | ⚠️ 文字**会**被规整，这是对的 | —— |
| ↑ 只挖 URL 不挖文字 | 文字跟正文同一标准（同一段文字不该有两种标准），且幂等 | —— |
 * | 图片 alt / url | 同上 | 图片裂开 |
 * | 行首标记（`#`、`-`、`>`）后 | 那不是「中英文之间」 | `- 项目` → `-项目`，列表不渲染 |
 * | 全角标点两侧 | 全角标点自带间距 | 「你好，world」→「你好， world」 |
 * | 数字+单位 `1080p`、`30fps` | 加空格反而是错的 | `1080 p` |
 *
 * 做法：**先把代码块、链接 URL、图片、HTML 标签、HTML 实体挖成占位符**，
 * 只对剩下的纯文本做加空格，最后放回。挖/填用私用区字符当哨兵 ——
 * 正文里几乎不可能含有，且不需要转义。
 *
 * ## 为什么不用现成的 Markdown AST
 *
 * 工程里已经有 Milkdown（ProseMirror）能给出 AST，但那是给编辑器用的；
 * 为一个独立按钮拉整套编辑器初始化不划算，而美化时编辑器可能正在加载。
 * 行级正则的边界（围栏、行内代码、链接）在这里足够明确，且**行为可预测**：
 * 美化后的文本用户能肉眼核对。
 *
 * ## 硬要求：幂等
 *
 * 同一段文本调用两次，第二次必须与第一次结果**完全相同**。不幂等的美化是灾难
 * —— 用户会发现「点一次变一样、点两次又变回去」，那比不提供这个按钮更糟。
 * `tightSuffixes` 那段有两处 replace 专门在消灭「上一次留下的空格」。
 */

/** 占位符哨兵：U+E000 私用区，正文几乎不可能含有，且在正则里不需要转义。 */
const PH = "\uE000"

const vault: string[] = []

function stash(text: string): string {
  vault.push(text)
  return `${PH}${vault.length - 1}${PH}`
}

function resetVault() {
  vault.length = 0
}

/** 放回占位符。循环到不再有占位符为止 —— 单趟会在嵌套时静默留下一个裸数字。 */
function releaseAll(s: string): string {
  let out = s
  for (let guard = 0; guard < 64 && out.includes(PH); guard++) {
    let hit = false
    out = out.replace(new RegExp(`${PH}(\\d+)${PH}`, 'g'), (_m, n: string) => {
      hit = true
      const v = vault[Number(n)]
      return v === undefined ? _m : v
    })
    if (!hit) break
  }
  return out
}

const CJK = '\\u2E80-\\u9FFF\\u3000-\\u303F\\uF900-\\uFAFF\\uFF00-\\uFFEF'
const LATIN = 'A-Za-z0-9'

/** 全角右括号类（后接拉丁时不加空格）。 */
const CLOSE_PUNCT = '，。、；：？！）」』】》〉'
/** 全角左括号类（前面接拉丁时不加空格）。 */
const OPEN_PUNCT = '（「『【《〈'

/**
 * 紧贴型后缀：这些词前面的空格是错的。
 *
 * 「1080 p」「30 fps」「H 264」在中文技术写作里会被当成外行。
 * 这份清单**刻意不完整** —— 判据是「加了空格会改变含义、或读起来是外行」，
 * 拿不准的一律维持原样（也就是不加）。加错比漏加更糟：漏加只是不整齐，
 * 加错是把 `1080p` 变成一个不存在的东西。
 */
const TIGHT_SUFFIXES: readonly string[] = [
  // 视频/图像
  'p', 'i', 'fps', 'px', 'pt', 'dpi', 'vw', 'vh', 'bit', 'bps', 'PCM', 'AAC',
  // 编码/格式
  'MP3', 'MP4', 'H264', 'H265', 'AV1', 'PDF', 'HTML', 'CSS', 'SQL', 'JSON', 'XML',
  // 单位
  'ms', 'kg', 'km', 'cm', 'mm', 'GB', 'MB', 'KB', 'TB', 'TB', 'Hz', 'kHz', 'MHz', 'GHz',
  // 常见缩写
  'App', 'API', 'URL', 'UI', 'UX', 'CLI', 'SDK', 'IDE', 'OS', 'CPU', 'GPU', 'RAM',
  'SSD', 'USB', 'HTTP', 'HTTPS', 'TCP', 'UDP', 'DNS', 'SSH', 'XLSX', 'DOCX', 'PPTX',
]

/**
 * 中英文之间加空格（不含行首标记处理 —— 那由调用方拆开做）。
 *
 * 只在两侧**一端 CJK、一端拉丁/数字**时插；再排除三类不该插的情况：
 * 全角标点两侧、紧贴型后缀、以及已经是空格的情况（幂等）。
 */
function spaceCjkLatinBase(line: string): string {
  let out = line
    .replace(new RegExp(`([${CJK}])([${LATIN}])`, 'g'), (_m, a: string, b: string) => `${a} ${b}`)
    .replace(new RegExp(`([${LATIN}])([${CJK}])`, 'g'), (_m, a: string, b: string) => `${a} ${b}`)

  // 全角标点自带左右间距 —— 上面两条规则会在它与拉丁字母之间插空格，这里收回
  out = out
    .replace(new RegExp(`([${CLOSE_PUNCT}])[ ]+([${LATIN}])`, 'g'), '$1$2')
    .replace(new RegExp(`([${LATIN}])[ ]+([${OPEN_PUNCT}])`, 'g'), '$1$2')
    .replace(new RegExp(`([${LATIN}])[ ]+([${CLOSE_PUNCT}])`, 'g'), '$1$2')

  // 紧贴型后缀：既删掉刚加的空格，也删掉原本就有的多余空格（幂等的关键）
  for (const suf of TIGHT_SUFFIXES) {
    out = out
      .replace(new RegExp(`([${LATIN}]) ${suf}`, 'gi'), (_m, a: string) => `${a}${suf}`)
      .replace(new RegExp(`([${LATIN}])[ ]+${suf}`, 'gi'), (_m, a: string) => `${a}${suf}`)
  }
  return out
}

/** 行首 Markdown 标记：保留标记与其后**恰好一个**空格。 */
const LEAD_MARK =
  /^(\s*(?:[-*+]\s+|\d+[.)]\s+|#{1,6}\s+|>\s?))/

/** 统一列表符号：无序一律 `-`，有序保留数字但分隔符统一为 `.`。 */
function unifyListMarker(line: string): string {
  const m = line.match(/^(\s*)([-*+]|\d+[.)])([ \t]+)(.*)$/)
  if (!m) return line
  // ⚠️ 只解构用到的：`gap` 那组是 `[ 	]+`，它的作用是**让正则跳过标记与
  //   内容之间的空白**，而 `rest` 本身不需要空白。所以 `gap` 不能要 ——
  //   `noUnusedLocals` 会报错（TS6133），这条提醒是对的。
  const [, indent, marker, , rest] = m
  // ⚠️ 行首标记里的 `*` 在 Markdown 里可能是强调符号残留，但本函数只在
  //    「标记 + 空白 + 内容」形态下生效，所以不会误伤 `*斜体*`
  const bullet = /^\d/.test(marker) ? marker.replace(/[.)]$/, '.') : '-'
  return `${indent}${bullet} ${rest}`
}

/** 行内处理：拆出行首标记 → 只对内容部分做中英文加空格。 */
function beautifyLine(raw: string): string {
  const line = unifyListMarker(raw.replace(/[ \t]+$/, ''))
  const m = line.match(LEAD_MARK)
  if (!m) return spaceCjkLatinBase(line)
  const mark = m[1]
  // 标记后的空白规范成恰好一个空格：`-  项目` → `- 项目`（这属于「清理多余空行」）
  const rest = line.slice(mark.length).replace(/^[ \t]+/, '')
  return mark.replace(/[ \t]+$/, ' ') + spaceCjkLatinBase(rest)
}

/**
 * 一键美化。
 *
 * @param md 原始 Markdown
 * @returns 美化后的 Markdown；**幂等**（`f(f(x)) === f(x)`）
 */
export function beautifyMarkdown(md: string): string {
  resetVault()

  // ① 挖出不能碰的东西。顺序有讲究：围栏必须**先于**行内代码。
  //
  //    ⚠️ 实测发现：**闭合**围栏其实被行内代码规则顺带覆盖了 ——
  //    `(`+)(?:[^`]|…)*\1` 会贪婪吃掉开头 3 个反引号、把中间全当非反引号、
  //    再回匹配结尾 3 个，整块围栏被当成一个超长行内代码。所以围栏规则
  //    真正**唯一**负责的是**未闭合**围栏（用户打字打到一半）：那种情况
  //    行内代码规则找不到配对的收尾反引号，匹配不上。
  //    两条规则都留着 —— 靠「顺带」兜底太脆，而留着不多花什么。
  //    （这条发现是 `beautify.test.mjs` 里那条未闭合用例逼出来的：
  //    我删掉围栏规则跑测试，绿的。）
  let text = md
    .replace(/```[\s\S]*?(?:```|$)/g, (m) => stash(m))
    .replace(/(`+)(?:[^`]|(?!\1)`)*\1/g, (m) => stash(m))
    .replace(/!\[[^\]]*\]\([^)]*\)/g, (m) => stash(m))
    // 链接：**只挖 URL**，文字保留原样（文字是内容，不该被动）
    .replace(/\[([^\]]*)\]\(([^)]*)\)/g, (_m, label: string, url: string) => {
      return `[${label}](${stash(url)})`
    })
    .replace(/<\/?[A-Za-z][^>]*>/g, (m) => stash(m))
    .replace(/&[a-zA-Z]+;|&#\d+;/g, (m) => stash(m))

  // ② 逐行：统一列表标记 + 中英文加空格 + 去行尾空白
  let out = text
    .split('\n')
    .map(beautifyLine)
    .join('\n')

  // ③ 连续空行压成一个、去首尾空行。
  //    ⚠️ 代码块内部的空行天然被保护 —— 它已被整块替换成单行占位符，
  //    这正是「先挖再处理」的关键收益。
  out = out.replace(/\n{3,}/g, '\n\n').replace(/^\n+/, '').replace(/\n+$/, '')

  // ④ 放回。放回后可能又出现 3+ 换行（围栏内部），那是**代码**，不再压 ——
  //    压了就改了代码内容。
  return releaseAll(out)
}
