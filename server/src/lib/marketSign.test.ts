// 市场清单构建的纯逻辑测试。
//
// ## 判据：这些测试守的是什么
//
// 签名保证的是**完整性**，不是**正确性**。一份「字段全是 undefined、但签名
// 完全正确」的清单，客户端验签会**通过**，然后用户看到一个没有名字、没有
// 下载链接、点了 404 的空壳扩展 —— 而所有校验都显示绿色。
//
// 所以真正要守的是「构建出来的清单字段名与客户端期望的一致」，以及
// 「包体/manifest 缺失的条目不许混进去」。
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { buildRegistry, cmpVersion, registryBytes, type PublishedRow } from './marketSign.ts'

const BASE = 'https://m-hub-server.pages.dev'

function row(over: Partial<PublishedRow> = {}): PublishedRow {
  return {
    extId: 'local.calculator',
    version: '0.1.6',
    pkgSha256: 'a'.repeat(64),
    pkgSize: 7775,
    manifestJson: JSON.stringify({ id: 'local.calculator', name: '计算器', version: '0.1.6' }),
    username: 'anson55sky',
    ...over,
  }
}

test('cmpVersion 与客户端同口径（三段数字，缺位当 0）', () => {
  assert.equal(cmpVersion('0.1.10', '0.1.9'), 1, '10 > 9（不能按字符串比）')
  assert.equal(cmpVersion('0.2.0', '0.1.99'), 1)
  assert.equal(cmpVersion('1.0.0', '1.0'), 0, '缺位当 0，两者相等')
  assert.equal(cmpVersion('0.1.5', '0.1.5'), 0)
  assert.equal(cmpVersion('0.1.4', '0.1.5'), -1)
})

test('条目字段照抄包内 manifest，下载地址是绝对路径', () => {
  const r = buildRegistry([row()], { revoked: [], baseUrl: BASE })
  assert.equal(r.extensions.length, 1)
  const e = r.extensions[0]!
  assert.equal(e.id, 'local.calculator')
  assert.equal(e.name, '计算器')
  assert.equal(e.version, '0.1.6')
  assert.equal(e.downloadUrl, `${BASE}/packages/local.calculator/0.1.6/local.calculator-0.1.6.xhpack`)
  assert.equal(e.sha256, 'a'.repeat(64))
  assert.equal(e.size, 7775)
})

/**
 * ⚠️ 权限/截图必须是**字符串数组**，不能塞对象。
 * 客户端 `market.rs` 的 `MarketExtension` 是 `Vec<String>`；
 * 塞对象过去 serde 会整条解析失败 → 清单退回缓存 → 用户看到空市场。
 */
test('permissions / screenshots 只取字符串数组（塞对象会让客户端整条解析失败）', () => {
  const r = buildRegistry(
    [
      row({
        manifestJson: JSON.stringify({
          id: 'local.calculator',
          name: '计算器',
          version: '0.1.6',
          permissions: ['storage', { bad: 1 }, 'network'],
          screenshots: ['https://x/a.png', 42],
        }),
      }),
    ],
    { revoked: [], baseUrl: BASE },
  )
  const e = r.extensions[0]!
  assert.deepEqual(e.permissions, ['storage', 'network'], '非字符串元素必须被剔除')
  assert.deepEqual(e.screenshots, ['https://x/a.png'])
})

/**
 * 同一个扩展只上架**版本号最高**的一条。
 *
 * ⚠️ 判据用的是版本号，不是「哪条 updated_at 新」——
 * 先发 0.1.6、后来又补审了 0.1.4，按时间取会让清单退化成旧版本。
 */
test('同一扩展只保留版本号最高的一条（不按时间取）', () => {
  const r = buildRegistry(
    [
      row({ version: '0.1.6', manifestJson: JSON.stringify({ id: 'local.calculator', name: '新', version: '0.1.6' }) }),
      row({ version: '0.1.4', manifestJson: JSON.stringify({ id: 'local.calculator', name: '旧', version: '0.1.4' }) }),
    ],
    { revoked: [], baseUrl: BASE },
  )
  assert.equal(r.extensions.length, 1)
  assert.equal(r.extensions[0]!.version, '0.1.6')
  assert.equal(r.extensions[0]!.name, '新')
})

/**
 * 没有包体 / 没有 manifest 的条目**必须跳过**。
 *
 * ⚠️ 这两种「坏条目」都会给用户一个点了 404 或全空的卡片，
 * 而服务端一切正常 —— 是最难被发现的一类问题。
 * 注意顺序：包里没 manifest 的那条更早被滤掉（`!mf` 那条）。
 */
test('缺包体 / 缺 manifest 的条目不进清单（宁可不显示，也不给空壳）', () => {
  // ⚠️ 每条坏数据必须给**不同的 extId** —— buildRegistry 先按 extId 去重，
  // 同 id 的坏行会被去重吃掉，测试就变成了「什么都没测到」（第一版就栽在这：
  // 期望 1 条、实际拿到的是另一条同名扩展的条目）。
  const bad = (id: string, over: Partial<PublishedRow>) =>
    row({ extId: id, manifestJson: JSON.stringify({ id, name: id }), ...over })
  const r = buildRegistry(
    [
      bad('bad.no-sha', { pkgSha256: null }),
      bad('bad.no-size', { pkgSize: null }),
      bad('bad.no-manifest', { manifestJson: null }),
      bad('bad.bad-json', { manifestJson: '{ 这不是合法 JSON' }),
      bad('local.ok', {}),
    ],
    { revoked: [], baseUrl: BASE },
  )
  assert.deepEqual(
    r.extensions.map((e) => e.id),
    ['local.ok'],
    '只有那条完整的该留下',
  )
})

/**
 * 去重**先于**过滤：最高版本的那条坏了，就**整条不显示**，不退回旧版本。
 *
 * ⚠️ 这是一个刻意的取舍，不是疏漏：退到旧版本会让用户「莫名其妙装了一个更老的」，
 *   而他明明提交过更新的那个。宁可扩展暂时从市场消失（作者能在提交记录里看到
 *   「已上架」并排查），也不要给一个与事实不符的旧版本。
 */
test('最高版本那条坏了就整条不显示（不退回旧版本）', () => {
  const r = buildRegistry(
    [
      row({ extId: 'x.y', version: '0.1.4', pkgSha256: 'b'.repeat(64), manifestJson: JSON.stringify({ id: 'x.y', name: '旧', version: '0.1.4' }) }),
      row({ extId: 'x.y', version: '0.1.6', pkgSha256: null, manifestJson: JSON.stringify({ id: 'x.y', name: '新', version: '0.1.6' }) }),
    ],
    { revoked: [], baseUrl: BASE },
  )
  assert.equal(r.extensions.length, 0, '新版本坏掉时不该悄悄退回 0.1.4')
})

/**
 * manifest 里的字段名缺失时要有**可辨认的回落**，不能是 undefined。
 *
 * 判据：`name` 缺了回落到 id —— 用户在列表里还能认出这是哪个扩展；
 * 全空则是一张没有标题的卡片，比不显示更糟。
 */
test('manifest 字段缺失时回落到可辨认的值（不留 undefined）', () => {
  const r = buildRegistry([row({ manifestJson: JSON.stringify({ id: 'x.y' }) })], {
    revoked: [],
    baseUrl: BASE,
  })
  const e = r.extensions[0]!
  assert.equal(e.id, 'x.y')
  assert.equal(e.name, 'x.y', 'name 缺失回落到 id')
  assert.equal(e.version, '0.1.6', 'version 缺失回落到 submissions.version')
  assert.equal(e.description, undefined)
  assert.ok(!('description' in e) === false || e.description === undefined)
})

/**
 * `registryBytes` 的字节格式不是格式偏好 —— 它是**签名覆盖的对象**。
 *
 * 与 `seed-manifests.mjs` 的 `JSON.stringify(reg, null, 2)` + 尾随换行
 * 逐字一致；改了这里，两份清单的签名就不同。
 */
test('registryBytes = 2 空格缩进 + 尾随换行（与构建期脚本逐字一致）', () => {
  const r = buildRegistry([row()], { revoked: [], baseUrl: BASE })
  const text = new TextDecoder().decode(registryBytes(r))
  assert.ok(text.endsWith('}\n'), '必须以换行结尾')
  assert.match(text, /^\{\n {2}"schemaVersion": 2,/, '第一层缩进必须是 2 空格')
  // round-trip：解析回来必须还是同一个对象（没有丢字段）
  assert.deepEqual(JSON.parse(text), r)
})

/** 只追加字段、不抬 schemaVersion（约定 46：抬版本号会让老客户端的市场变空） */
test('schemaVersion 恒为 2，verified 是追加字段', () => {
  const r = buildRegistry([row()], { revoked: [], baseUrl: BASE })
  assert.equal(r.schemaVersion, 2)
  assert.equal(r.extensions[0]!.verified, true)
})

test('扩展按 id 排序，输出稳定（否则同一份数据两次构建字节不同）', () => {
  const mk = (id: string) => row({ extId: id, manifestJson: JSON.stringify({ id, name: id }) })
  const a = buildRegistry([mk('z.last'), mk('a.first')], { revoked: [], baseUrl: BASE })
  const b = buildRegistry([mk('a.first'), mk('z.last')], { revoked: [], baseUrl: BASE })
  assert.deepEqual(
    a.extensions.map((e) => e.id),
    ['a.first', 'z.last'],
  )
  // updatedAt 是时间戳，剥掉它再比字节
  const strip = (x: typeof a) => JSON.stringify({ ...x, updatedAt: '' })
  assert.equal(strip(a), strip(b), '同一份数据两次构建必须产生相同字节')
})