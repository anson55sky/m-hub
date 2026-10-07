import { test } from 'node:test'
import assert from 'node:assert/strict'
import { stripInvisible, hasInvisible } from './invisibleChars.ts'

/**
 * 隐形字符清理。全部做过变异验证。
 *
 * ⚠️ 用码位构造输入而不是打不可见字符 —— 测试文件里出现真正的 U+00A0 时，
 *   任何编辑器都可能把它显示成普通空格，于是「测试输入被悄悄改掉」而没人发现。
 *   （我第一次写这个文件就踩了：字面 U+00A0 让 Node 的 TS 解析器直接 SyntaxError。）
 */

// ⚠️ 用**码位转义**而不是字面量。
const U = {
  NBSP: "\u00A0", // ① 有可见宽度 → 换空格
  THIN: "\u2009", // ①
  IDEOGRAPHIC: "\u3000", // ①
  ZWSP: "\u200B", // ② 不可见 → 删掉
  ZWNJ: "\u200C", // ②
  LRM: "\u200E", // ②
  WORD_JOINER: "\u2060", // ②
  BOM: "\uFEFF", // ②
  ZWJ: "\u200D", // 不清
  SOFT_HYPHEN: "\u00AD", // ③ 不可见 → 删掉
}

test('① 有可见宽度的空白字符换成普通空格（不是删除）', () => {
  assert.equal(stripInvisible(`中文${U.NBSP}英文`), '中文 英文')
  assert.equal(stripInvisible(`全角${U.IDEOGRAPHIC}空格`), '全角 空格')
  // ⚠️ 必须是**空格**不是空串：删掉会得到「中文和」这种肉眼看不到缺口的伪连字，
  //    而原文那个间隔是存在过的。
  assert.notEqual(stripInvisible(`a${U.THIN}b`), 'ab')
})

test('② 零宽类直接删掉（不是换成空格）', () => {
  for (const [name, ch] of Object.entries({
    ZWSP: U.ZWSP,
    ZWNJ: U.ZWNJ,
    LRM: U.LRM,
    WORD_JOINER: U.WORD_JOINER,
    BOM: U.BOM,
  })) {
    assert.equal(stripInvisible(`a${ch}b`), 'ab', `${name} 应被直接删掉`)
  }
})

test('⚠️ 零宽字符不往文本里注入空格（网页防断词 ZWSP 会把单词劈开）', () => {
  // ⚠️ 这条是第一版的真 bug：`U+200B` 一律换成空格，于是
  //    `discuss<ZWSP>ion` → `discuss ion`（单词被拆）、`第<ZWSP>3` → `第 3`。
  //    用户看到的是「应用在正文里插了空格」，而正文一个字都没被改过。
  assert.equal(stripInvisible(`discuss${U.ZWSP}ion`), 'discussion')
  assert.equal(stripInvisible(`第${U.ZWSP}3节`), '第3节')
  assert.equal(stripInvisible(`发${U.ZWSP}布计划`), '发布计划')
})

/**
 * ⚠️ ZWJ（U+200D）必须**保留**：emoji 序列靠它组合
 *（`👨‍👩‍👧` = 3 个 emoji + 2 个 ZWJ）。删掉 emoji 就碎了。
 * 而 ZWNJ（U+200C）要清 —— 正文里它几乎只来自网页排版。
 */
test('ZWJ 保留、ZWNJ 清理（emoji 不能被拆散）', () => {
  const family = `👨${U.ZWJ}👩${U.ZWJ}👧`
  assert.equal(stripInvisible(family), family, '家庭 emoji 序列必须原样保留')
  assert.ok(stripInvisible(family).includes(U.ZWJ), 'ZWJ 不该被清')
  assert.equal(stripInvisible(`a${U.ZWNJ}b`), 'ab', 'ZWNJ 该清')
})

test('③ 软连字符直接删掉', () => {
  assert.equal(stripInvisible(`re${U.SOFT_HYPHEN}sume`), 'resume')
  assert.notEqual(stripInvisible(`a${U.SOFT_HYPHEN}b`), 'a b')
})

test('正常文本不受影响', () => {
  const s = '第一行\n第二行\t缩进  normal  spaces  中文English混排 # 标题 - 列表'
  assert.equal(stripInvisible(s), s)
  assert.equal(hasInvisible(s), false)
})

test('hasInvisible 可重复调用（正则带 /g，lastIndex 必须复位）', () => {
  const s = `a${U.ZWSP}b`
  // ⚠️ 带 /g 的正则有 lastIndex 状态：不复位的话第二次调用会返回 false（漏报）
  assert.equal(hasInvisible(s), true)
  assert.equal(hasInvisible(s), true)
  assert.equal(hasInvisible(s), true)
})

/* ── 变异验证（改源码跑，已实测四条全红） ──────────────────────────
   ① 把 ZWJ 也加进清理类              → 「ZWJ 保留」「emoji 不能被拆散」红
   ② 把 ZERO_WIDTH 改成换空格而非删除   → 「零宽类直接删掉」等 4 条红
   ③ 把 SPACED 改成删除而非换空格      → 「① 有可见宽度」红
   ④ 去掉 lastIndex 复位               → 「可重复调用」红
   ⑤ 把软连字符改成换空格              → 「③ 软连字符」红
 */
