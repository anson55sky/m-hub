// 构建期守卫：文档里引用的 `docs/**` 文件必须真实存在。
//
// ## 为什么要有这个守卫
//
// 2026-10-06 开源时发现：`AGENTS.md` 里有 **10 处**「详见 docs/xxx」，
// 而 `docs/` 目录**从未提交过、磁盘上也不存在**。同样的死链还散落在
// `CONTEXT.md` / `DESIGN.md` / 11 个源码文件的注释里。
//
// 死链的危害不是「链接点了没反应」，而是**它们看起来像有据可查**：
// 读的人（尤其是 agent）会以为「详细设计在 docs/adr/0008.md」，
// 于是**不去读 AGENTS.md 里其实已经写全的那一条** —— 于是以一份
// 不存在的文档为准去改代码。AGENTS.md 约定 44/45 的正文就是 0008/0005 的全文，
// 决策一条都没丢，丢的只是指路。
//
// 所以：**要么文件真在，要么别写。** 本守卫把这条钉住。
//
// 判定口径（有意收窄，误报比漏报更烦）：
// · 只查 `docs/` 开头的**路径型引用**（`docs/xxx.md` / `docs/adr/0008-…`）；
// · 排除测试夹具里的假路径（`C:/docs/report.pdf` 之类）与
//   `RELEASE_NOTES.md` 里的历史叙述（那记录的是「当时有个方案文档」）；
// · 允许**明确说明「该文件不存在」**的句子（例如「那份文档从未提交过」），
//   这类是有意留下的历史交代，不是死链。
//
// 变异验证：塞一条真死链必须变红。

import { readFileSync, existsSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join, relative } from 'node:path'
import { execFileSync } from 'node:child_process'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')

/** 扫哪些文件：全部被 git 跟踪的文本文件 */
const tracked = execFileSync('git', ['ls-files'], { cwd: root, encoding: 'utf8' })
  .split('\n')
  .filter(Boolean)
  .filter((f) => /\.(md|rs|ts|vue|mjs|jsonc|toml)$/.test(f))

const SKIP_FILES = new Set(['RELEASE_NOTES.md', 'scripts/check-doc-links.mjs'])

/**
 * 找出文件里所有 `docs/…` 形式的路径引用。
 *
 * ⚠️ 匹配到终止符为止（含 `-` `.` `/`），这样 `docs/adr/0008-extension-
 * content-origin-isolation.md` 能整段取到，而不是只取到 `docs/adr/0008`。
 */
const DOC_REF = /docs\/[A-Za-z0-9_./-]*[A-Za-z0-9_]/g

/**
 * 这句话是不是在**明确交代「该文档不存在」**。
 *
 * 形如「那份文档从未提交过」「原 `docs/adr/0008` 的全文」是有意留下的历史说明，
 * 删掉反而丢失信息（后来的人会重新踩一遍「为什么没有 docs」）。
 */
function isDeliberateAbsence(text) {
  // 形态一：明确说了「这份文档不存在/没提交过」
  if (/(从未提交过|从未存在|已不存在|已删除|不再存在|没有 docs|无 docs)/.test(text)) return true
  // 形态二：「原 `docs/adr/0008` 的全文」这种**指代已在别处**的说法
  //
  // ⚠️ 第一版写得过宽：`(原)?\s*`?docs\/[^`]*`?\s*(的全文|那份|)` 里
  //   「原」与所有空白都是**可选项**，于是 `详见 \`docs/adr/9999.md\`。`
  //   这种纯死链也命中 —— 守卫等于没写（变异测试当场抓出来）。
  // 现在要求「原」字**紧邻** docs/（允许中间有反引号与空白），且后面必须跟
  // 「的全文 / 那份」之一，两者缺一不可。
  return /原\s*`?docs\/[^`\s]*`?\s*(?:的全文|那份)/.test(text)
}

let failed = 0
const found = []

for (const rel of tracked) {
  if (SKIP_FILES.has(rel)) continue
  const abs = join(root, rel)
  let text
  try {
    if (statSync(abs).size > 4 * 1024 * 1024) continue
    text = readFileSync(abs, 'utf8')
  } catch {
    continue
  }
  if (!text.includes('docs/')) continue

  for (const m of text.matchAll(DOC_REF)) {
    const ref = m[0]
    // ⚠️ 正则只取到 `docs/…`，**看不到前面的盘符**。所以要回看命中位置之前的
    //   几个字符：`C:/docs/report.pdf` 里的 docs 段本身看不出是夹具假路径。
    const before = text.slice(Math.max(0, m.index - 4), m.index)
    // 取所在整句，判断是不是「刻意交代不存在」
    const start = Math.max(0, m.index - 160)
    const sentence = text.slice(start, m.index + ref.length + 160)
    if (isDeliberateAbsence(sentence)) continue
    // 测试夹具 / 被测产物里的**假路径**不是死链：`C:/docs/report.pdf` 是造数据用的
    // 样例文件名，`docs/LICENSE.txt` 是扩展包内一个真实存在的条目名。
    // 判据：这类引用几乎都带盘符前缀或位于「包内条目」语境，且路径含文件扩展名。
    if (/[A-Za-z]:\/$/.test(before)) continue
    if (ref.includes('LICENSE.txt')) continue
    // 路径型引用（带扩展名）才是死链的候选；裸 `docs/adr/0005` 是章节号式提法，
    // 也算 —— 它同样指向一个不存在的文件。
    if (existsSync(join(root, ref))) continue
    const line = text.slice(0, m.index).split('\n').length
    found.push({ file: rel, line, ref })
  }
}

if (found.length) {
  console.error(`  ✗ 发现 ${found.length} 处指向不存在文件的 docs/ 引用：\n`)
  for (const f of found) {
    console.error(`    ${f.file}:${f.line}  →  ${f.ref}`)
  }
  // ⚠️ 这段提示文本**故意不写任何 docs/ 开头的样例路径** ——
  //   本守卫会连自己一起扫（SKIP_FILES 里已排除本文件，但更稳的是别写出来）。
  console.error(
    '\n  三条出路，**不要为了消警告而新建文档目录**：\n' +
      '  ① 若内容已在别处（本工程绝大多数情况如此），把引用改成指向它 ——\n' +
      '     决策全文通常就在 AGENTS.md 的某条约定里（见下方每条给出的行号自行核对）；\n' +
      '  ② 若确实是新写的设计文档，就真的提交那个文件；\n' +
      '  ③ 若只是想记录「曾经有过这份文档」，把话改成明确交代它不存在\n' +
      '     （含「从未提交过」等字样会被本守卫放行）。\n\n' +
      '  判据：**一份没有被提交的文档等于不存在**，而指路牌会让人以为它存在。',
  )
  process.exit(1)
}
console.log(`  [doc-links] ${tracked.length} 个跟踪文件，docs/ 引用全部可达 ✓`)