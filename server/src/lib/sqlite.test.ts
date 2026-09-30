// `?N` → 裸 `?` 重编号的行为。
//
// ## 为什么这条最该被测
//
// node:sqlite **不支持** `?N`（实测报 `column index out of range`），
// 而本项目 24 处调用点全是 `?N` 形状。所以这个函数错了，症状是
// **参数错位**：查询返回 0 行、或把 A 的值写进 B 的列 ——
// 极难从现象反推，且在测试之外完全静默。
//
// 它是纯函数，故能完整覆盖。

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { renumber_params } from './sqlite.ts'

test('?N 按出现顺序改写成裸 ?，参数按传入顺序展开', () => {
  const r = renumber_params('SELECT * FROM t WHERE a = ?1 AND b = ?2', ['x', 'y'])
  assert.equal(r.sql, 'SELECT * FROM t WHERE a = ? AND b = ?')
  assert.deepEqual(r.params, ['x', 'y'])
})

test('乱序的 ?N 也按数字取参数，不按出现顺序', () => {
  // ?2 在前、?1 在后：参数应按**编号**取，与出现顺序无关
  const r = renumber_params('INSERT INTO t(a,b) VALUES(?2, ?1)', ['A', 'B'])
  assert.equal(r.sql, 'INSERT INTO t(a,b) VALUES(?, ?)')
  assert.deepEqual(r.params, ['B', 'A'])
})

test('字符串字面量里的 ?1 不是占位符', () => {
  // 漏掉这条会把 'a?1b' 里的内容当参数取走，并凭空多出一个 ? → 参数错位
  const r = renumber_params("SELECT * FROM t WHERE a = ?1 AND b LIKE '%?2%'", ['A'])
  assert.equal(r.sql, "SELECT * FROM t WHERE a = ? AND b LIKE '%?2%'")
  assert.deepEqual(r.params, ['A'])
})

test('字面量在改写后被原样放回', () => {
  const r = renumber_params("SELECT '?1' AS lit, ?1 AS v", ['V'])
  assert.ok(r.sql.includes("'?1'"), `字面量应原样保留: ${r.sql}`)
  assert.deepEqual(r.params, ['V'])
})

test('占位符越界时抛错，而不是静默绑错值', () => {
  // 静默绑错是这类 bug 最坏的形态：查询照跑，返回一个「看起来合理」的空结果
  assert.throws(
    () => renumber_params('SELECT * FROM t WHERE a = ?3', ['A']),
    /越界/,
  )
})

test('裸 ? 与 ?N 混用时报错（不猜绑定顺序）', () => {
  // D1 允许混用且按裸 ? 出现顺序编号；node:sqlite 的编号规则不同。
  // 混用时两种运行时的绑定顺序可能不一致 → 显式拒绝
  assert.throws(
    () => renumber_params('SELECT * FROM t WHERE a = ? AND b = ?1', ['A', 'B']),
    /混用/,
  )
})

test('无占位符时原样返回', () => {
  const r = renumber_params('SELECT 1', [])
  assert.equal(r.sql, 'SELECT 1')
  assert.deepEqual(r.params, [])
})

test('INSERT OR IGNORE / RETURNING / 部分唯一索引等 SQLite 方言原样通过', () => {
  // schema.sql 与 ai.ts 依赖的这些构造，改写不得破坏它们
  const cases = [
    'INSERT OR IGNORE INTO ai_quota (user_id, day, granted, used) VALUES(?1, ?2, ?3, 0)',
    'UPDATE ai_quota SET used = used + 1 WHERE user_id = ?1 AND day = ?2 AND used < granted RETURNING granted - used AS remaining',
    'DELETE FROM sessions WHERE user_id = ?1 AND token LIKE ?2',
    'UPDATE submissions SET status = ?2, updated_at = ?1 WHERE id = ?1',
  ]
  for (const sql of cases) {
    // ⚠️ 只数 `?N`，**不能**用「问号总数」—— 语句里还有字面量 0
    // （`VALUES(?1, ?2, ?3, 0)`），按问号总数会多传一个参数，
    // 而 [renumber_params] 的越界检查是**故意**要抛错的。
    // 这条断言本意是验「方言关键字不被改写」，不是验参数个数。
    const n = (sql.match(/\?\d+/g) ?? []).length
    const ps = Array.from({ length: n }, (_, i) => i)
    const r = renumber_params(sql, ps)
    assert.equal((r.sql.match(/\?/g) ?? []).length, n, `占位符数应变: ${sql}`)
    // 关键字不该被改写掉
    for (const kw of ['INSERT OR IGNORE', 'RETURNING', 'LIKE', 'updated_at']) {
      if (sql.includes(kw)) {
        assert.ok(r.sql.includes(kw), `关键字 ${kw} 被改写坏了: ${r.sql}`)
      }
    }
  }
  // `?N` 按**数字**取参数而非出现顺序（这正是本条最容易写错的地方：
  // 写断言时按出现顺序写，而实现按编号取，测试就红了 —— 但实现是对的）
  {
    const sql = 'UPDATE submissions SET status = ?2, updated_at = ?1 WHERE id = ?1'
    const r = renumber_params(sql, [111, 'withdrawn'])
    assert.equal(r.sql, 'UPDATE submissions SET status = ?, updated_at = ? WHERE id = ?')
    assert.deepEqual(r.params, ['withdrawn', 111, 111], '?1→params[0]、?2→params[1]')
  }
})

test('UNIQUE 子句里的常量不受影响', () => {
  const r = renumber_params('SELECT ?1 FROM s WHERE st = ?2', ['a', 'pending'])
  assert.equal(r.sql, 'SELECT ? FROM s WHERE st = ?')
  assert.deepEqual(r.params, ['a', 'pending'])
})
