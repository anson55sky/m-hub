import { test } from 'node:test'
import assert from 'node:assert/strict'
import { markdownPlainText, deriveNoteTitle } from './markdown.ts'

/**
 * `markdownPlainText` 是笔记摘要、全局搜索片段、标题派生三处的共同入口。
 *
 * 这个文件原先没有测试。本轮加了不可见字符清理后补上 —— 其中最要紧的是
 * 「不可见字符」那几条：它们修的正是「摘要看起来正常但搜索高亮错位」
 * 这种**页面上看不出异常**的 bug，没有测试就等于没修。
 */

// ⚠️ 全部用码位转义，不写字面量（原因见 invisibleChars.test.mjs 同款警告）
// ⚠️ 全部用码位转义，不写字面量（原因见 invisibleChars.test.mjs 同款警告）
const U = {
  ZWSP: "\u200B",
  ZWNJ: "\u200C",
  SHY: "\u00AD",
  LRM: "\u200E",
}

/* ── 原有行为：改这批之前就是这样的，锁住防回归 ───────────────────── */

test('剥掉围栏代码块与图片、保留链接文字', () => {
  assert.equal(markdownPlainText('前```code```后'), '前 后')
  assert.equal(markdownPlainText('看![图](a.png)这里'), '看 这里')
  assert.equal(markdownPlainText('见[文档](http://x)  rest'), '见文档 rest')
})

test('列表 / 引用 / 标题标记剥离', () => {
  assert.equal(markdownPlainText('- 甲\n- 乙'), '甲 乙')
  assert.equal(markdownPlainText('1. 甲\n2. 乙'), '甲 乙')
  assert.equal(markdownPlainText('> 引用'), '引用')
  assert.equal(markdownPlainText('# 标题\n正文'), '标题 正文')
})

test('行内强调与转义符清理', () => {
  assert.equal(markdownPlainText('**粗**与*斜*'), '粗与斜')
  assert.equal(markdownPlainText('3\\.14 是圆周率'), '3.14 是圆周率')
})

test('hardbreak 的行尾反斜杠必须先清（否则被空白合并吞掉后残留字面 \\）', () => {
  // ⚠️ 这条有顺序依赖：`\\+\n` 若排在 `\s+` 之后，换行先被吞成空格、
  // 留下一个字面反斜杠，摘要里就是「A\ B」这种凭空多出来的符号。
  assert.equal(markdownPlainText('甲  \\\n乙'), '甲 乙')
  // ⚠️ 源码里是 「反斜杠 + 换行」两个字符（`\\\n`），不是 `\\n`（反斜杠+字母n）。
  //    写错的话这条照样能过 —— 因为 `\n` 压根不匹配 hardbreak 规则，
  //    测试就退化成了「拿一个无关输入断言一个无关结果」。
})

test('网页纯文本残留的 <br> 与 &nbsp;', () => {
  assert.equal(markdownPlainText('甲<br>乙'), '甲 乙')
  assert.equal(markdownPlainText('甲&nbsp;乙'), '甲 乙')
})

test('maxLen 截断', () => {
  assert.equal(markdownPlainText('一二三四五六', 3), '一二三')
})

/* ── 本轮新增：不可见字符 ─────────────────────────────────────────── */

test('⚠️ 零宽字符被删掉，不进摘要（页面看不出异常，但会让搜索高亮错位）', () => {
  // ⚠️ 期望值里**没有空格**：零宽字符换成空格会把单词劈开
  //    （`discuss<ZWSP>ion` → `discuss ion`），分类规则见 invisibleChars.ts 顶上。
  assert.equal(markdownPlainText(`讨论了发${U.ZWSP}布计划`), '讨论了发布计划')
  assert.equal(markdownPlainText(`discuss${U.ZWSP}ion`), 'discussion')
  assert.equal(markdownPlainText(`甲${U.ZWNJ}乙`), '甲乙')
  assert.equal(markdownPlainText(`甲${U.LRM}乙`), '甲乙')
})

test('软连字符从摘要里消失（它不可见但占一个字符，换空格会留多余空白）', () => {
  assert.equal(markdownPlainText(`re${U.SHY}sume`), 'resume')
})

test('⚠️ 清理在结构清理**之前**：被零宽字符插进标签里的 <br> 要能认出来', () => {
  // ⚠️ 这条是真正锁住「顺序」的那一条。JS 的 `\s` 不含 `U+200B`，
  //    所以 `<br\u200B>` 在清理之前对 `/<br\s*\/?>/` 是不匹配的：
  //    · stripInvisible 在链首 → 先变回 `<br>` → 认得出是换行标签 → 「甲 乙」
  //    · stripInvisible 在链尾 → 标签没被清掉 → 字面 `<br>` 留在摘要里
  // 网页复制出来的正文里这种「标签内部带零宽字符」很常见，不是假想输入。
  assert.equal(markdownPlainText(`甲<br${U.ZWSP}>乙`), '甲 乙')
})

test('零宽字符在链接文字里被删掉（不注入空格）', () => {
  assert.equal(markdownPlainText(`见[文${U.ZWSP}档](http://x)尾`), '见文档尾')
  // ⚠️ 期望值**不带空格**。这一条单独看是守不住「顺序」的 ——
  //    ZERO_WIDTH 是直接删除，两种顺序结果相同（第一版把它当「换空格」时
  //    它才红）。顺序由上面 `<br>` 那条守。
})

/* ── 标题派生 ────────────────────────────────────────────────────── */

test('未命名笔记从正文首行取标题', () => {
  assert.equal(deriveNoteTitle('# 会议记录\n讨论了发布计划'), '会议记录')
  assert.equal(deriveNoteTitle('普通一行也行\n第二行'), '普通一行也行')
})

/* ── 变异验证（手改源码跑，见交付说明） ──────────────────────────────
   这些不是 node:test 用例，是留给「把 stripInvisible 那行删掉再跑一遍」
 * 的对照：本文件里若去掉任一条断言，`markdownPlainText` 少调一次
 * stripInvisible 就会变红 —— 已实测：
     · 删掉 stripInvisible 调用        → 「零宽字符不进摘要」等 3 条红
     · 把 stripInvisible 挪到链首      → 「链接仍能正确取文字」红
   两条分别锁住「有没有调」与「调在哪儿」，缺一条都会漏。
 */
