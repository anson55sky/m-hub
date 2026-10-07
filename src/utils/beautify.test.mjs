import { test } from 'node:test'
import assert from 'node:assert/strict'
import { beautifyMarkdown } from './beautify.ts'

/**
 * 一键美化（发布说明 ⑥）。
 *
 * 断言分三类：
 * ① **该做的**（列表符号 / 空行 / 中英文空格）
 * ② **绝不能碰的**（代码块 / 链接 URL / 图片）—— 这类一旦破坏，用户丢的是数据
 * ③ **幂等** —— 不幂等的美化比没有更糟
 */

/* ── ① 该做的 ───────────────────────────────────────────────────── */

test('列表符号统一为 -（缩进层级保留）', () => {
  assert.equal(beautifyMarkdown('* 甲\n+ 乙\n- 丙'), '- 甲\n- 乙\n- 丙')
  assert.equal(beautifyMarkdown('  * 嵌套'), '  - 嵌套', '缩进要保留')
})

test('有序列表分隔符统一为 .', () => {
  assert.equal(beautifyMarkdown('1) 甲\n2) 乙'), '1. 甲\n2. 乙')
})

test('中英文之间加空格', () => {
  assert.equal(beautifyMarkdown('这是Hello世界'), '这是 Hello 世界')
  assert.equal(beautifyMarkdown('用Vue写前端'), '用 Vue 写前端')
  assert.equal(beautifyMarkdown('共3个文件'), '共 3 个文件')
})

test('连续空行压成一个，首尾空行去掉', () => {
  assert.equal(beautifyMarkdown('a\n\n\n\nb'), 'a\n\nb')
  assert.equal(beautifyMarkdown('\n\na\n\n'), 'a')
})

test('行尾空白清掉', () => {
  assert.equal(beautifyMarkdown('a   \nb\t'), 'a\nb')
})

/* ── ② 绝不能碰 ─────────────────────────────────────────────────── */

/**
 * ⚠️ 代码里的空格是**语义**。`int x` 被改成 `intx` 会直接编译不过 ——
 * 这不是排版问题，是把用户的代码改坏了。
 */
test('行内代码与围栏代码块逐字不动', () => {
  const src = '说明`int x = 1`，如下：\n\n```cpp\nint y = 2;  // 注释\n\n\nint z = 3;\n```'
  const out = beautifyMarkdown(src)
  assert.ok(out.includes('`int x = 1`'), '行内代码原样')
  assert.ok(out.includes('int y = 2;  // 注释'), '围栏首行原样')
  assert.ok(out.includes('\n\n\nint z = 3;'), '围栏内部的连续空行不能压')
})

/**
 * ⚠️ 围栏里**必须有中英混排**，否则测不出「有没有被挖走」。
 *
 * 我第一版这条只放了 `int y = 2;  // 注释` —— 全是拉丁字符，
 * 于是「挖掉围栏」这个变异照样绿：CJK↔拉丁加空格对纯拉丁文本是恒等变换。
 * 断言写了、却守不住它守的东西（约定 73 同款）。
 */
test('围栏里的中英混排也不动', () => {
  const src = '```js\n// 计算value和总数\nconst 名 = "abc";\n```'
  assert.equal(beautifyMarkdown(src), src)
  // 行内代码同理：必须有 CJK 才能测出来
  assert.equal(beautifyMarkdown('见`计算value`完'), '见`计算value`完')
})

/**
 * ⚠️ 纯拉丁的 URL 测不出来 —— `http://a.com/?x=1&y=2` 里没有任何 CJK↔拉丁
 * 相邻处，加空格规则对它是恒等变换，于是「不挖 URL」这个变异照样绿（实测）。
 * 必须用**含中文的 URL**（国内站点很常见）才测得出来。
 */
test('链接的 URL 不动（加空格会让链接失效）', () => {
  const out = beautifyMarkdown('看[文档](http://a.com/?x=1&y=2)吧')
  assert.ok(out.includes('(http://a.com/?x=1&y=2)'), `URL 被改了：${out}`)
  // 真正能区分的用例：URL 里有中文
  assert.equal(
    beautifyMarkdown('[文档](https://example.com/中文readme)'),
    '[文档](https://example.com/中文readme)',
  )
})

/**
 * 链接**文字**跟正文一起被规整 —— 同一段文字里不该有两种标准。
 *
 * ⚠️ 我第一版把断言写反了（期望文字原样不变），实测得到 `[Hello 世界](...)`。
 *   复核后确认**代码是对的、断言是错的**：文字是普通内容，用户看到的也是它，
 *   跟正文统一标准才对；真正不能碰的是 URL（下面那条）。
 *   而且这一改动是幂等的（`Hello 世界` 再跑一次不变），不会出现
 *   「点一次变一样、点两次变回去」。
 */
test('链接文字与正文同一标准', () => {
  assert.equal(beautifyMarkdown('[Hello世界](http://a.com)'), '[Hello 世界](http://a.com)')
  assert.equal(
    beautifyMarkdown(beautifyMarkdown('[Hello世界](http://a.com)')),
    '[Hello 世界](http://a.com)',
  )
})

test('图片整体不动', () => {
  const out = beautifyMarkdown('![说明文字](mhub-note.localhost/abc.png)')
  assert.ok(out.includes('![说明文字](mhub-note.localhost/abc.png)'), out)
})

test('HTML 标签与实体不动', () => {
  assert.ok(beautifyMarkdown('<span class="a">x</span>').includes('<span class="a">'))
  assert.ok(beautifyMarkdown('a&nbsp;b').includes('a&nbsp;b'))
})

/* ── 排版惯例的例外 ──────────────────────────────────────────────── */

test('全角标点两侧不加空格（标点自带间距）', () => {
  assert.equal(beautifyMarkdown('你好，world'), '你好，world')
  assert.equal(beautifyMarkdown('world（测试）'), 'world（测试）')
})

/**
 * ⚠️ 数字+单位加空格是**错的**：`1080 p` 是个不存在的东西。
 * 拿不准的一律维持原样 —— 漏加只是不整齐，加错是把词拆了。
 */
test('单位/版本/缩写保持紧贴', () => {
  // ⚠️ 「已经多了空格」才是这条规则真正起作用的场景。
  //    只测 `1080p`（本来就紧贴）的话，两种实现结果相同 —— 断言守不住。
  assert.equal(beautifyMarkdown('1080 p 视频'), '1080p 视频', '修掉多余的空格')
  assert.equal(beautifyMarkdown('30 fps'), '30fps')
  assert.equal(beautifyMarkdown('1080p 视频'), '1080p 视频')
  assert.equal(beautifyMarkdown('30fps 的帧'), '30fps 的帧')
  assert.equal(beautifyMarkdown('调用 API 接口'), '调用 API 接口')
  assert.equal(beautifyMarkdown('用 Vue3 写'), '用 Vue3 写')
})

/**
 * 块引用标记后的多余空白只有这里能测出来 ——
 * `LEAD_MARK` 里 `>\s?` 的 `?` 最多吃**一个**空白，而列表/标题的 `\s+` 是贪婪的
 * （多余空白已由 `unifyListMarker` 规范掉）。所以 `-  项目A` 那条测的是
 * 列表标记规范化，不是这个函数。
 */
test('块引用标记后的多余空白被规范成一个', () => {
  assert.equal(beautifyMarkdown('>   引用C'), '> 引用 C')
})

test('行首标记后不加空格（否则列表/标题不渲染）', () => {
  assert.equal(beautifyMarkdown('- 项目A'), '- 项目 A')
  assert.equal(beautifyMarkdown('# 标题B'), '# 标题 B')
  assert.equal(beautifyMarkdown('> 引用C'), '> 引用 C')
  assert.equal(beautifyMarkdown('1. 条目D'), '1. 条目 D')
})

/* ── ③ 幂等 ─────────────────────────────────────────────────────── */

/**
 * ⚠️ 不幂等的美化是灾难：用户会发现「点一次变一样、点两次又变回去」，
 * 那比不提供这个按钮更糟。
 */
test('幂等：对同一段文本跑两次结果完全相同', () => {
  const inputs = [
    '这是Hello世界',
    '* 甲\n+ 乙',
    'a\n\n\n\nb',
    '这是`int x`测试\n\n```\nint y = 1\n```',
    '看[文档](http://a.com/?x=1&y=2)吧',
    '你好，world 和 1080p 视频',
    '- 项目A\n  * 嵌套B\n\n1) 甲',
    '![图](a.png)说明Hello',
    '',
    '   ',
  ]
  for (const input of inputs) {
    const once = beautifyMarkdown(input)
    assert.equal(beautifyMarkdown(once), once, `输入：${JSON.stringify(input)}`)
  }
})

test('空输入与纯空白不炸', () => {
  assert.equal(beautifyMarkdown(''), '')
  assert.equal(beautifyMarkdown('   \n  \n'), '')
})

/**
 * ⚠️ 这条是围栏规则**唯一**真正负责的情形。
 *
 * 挖取链里围栏规则排在行内代码规则**之前**，但实测发现：**闭合**围栏其实
 * 被行内代码规则顺带覆盖了 —— 正则 `(`+)(?:[^`]|…)*\1` 会贪婪吃掉开头 3 个
 * 反引号、把中间全当非反引号、再回匹配结尾 3 个，整块围栏被当成一个行内代码。
 * 所以「删掉围栏规则」这个变异在闭合围栏上**测不出来**（实测绿）。
 *
 * 真正只有围栏规则能救的是**未闭合**的围栏（用户打字打到一半）：
 * 行内代码规则要求有配对的收尾反引号，匹配不上。
 */
test('未闭合的围栏代码块也受保护（这条才真的在守围栏规则）', () => {
  // ⚠️ 内容里必须有 CJK，否则纯拉丁文本上两种实现结果相同，断言守不住
  const src = '前面\n```js\n// 计算value和总数\nconst 名 = 1'
  assert.equal(beautifyMarkdown(src), src)
})

/* ── 变异验证（改源码跑，已实测五条全红） ──────────────────────────
   ① 去掉代码块挖除            → 「行内代码与围栏逐字不动」红
   ② 去掉链接 URL 挖除         → 「链接的 URL 不动」红
   ③ 把紧贴后缀改成加空格      → 「单位/版本/缩写保持紧贴」红
   ④ 空行压缩去掉              → 「连续空行压成一个」红
   ⑤ 行首标记后也加空格        → 「行首标记后不加空格」红
   ⑥ 只挖一层（releaseAll 单趟）→ 「围栏内部连续空行」红
 */
