// PostgreSQL 方言转换的行为。
//
// ## 为什么这条最该被测
//
// 托管库只提供 PostgreSQL（`platform_engines: ["postgresql"]`），而全部 SQL
// 用的是 SQLite 方言。这个函数错了，症状与 SQLite 版同款 —— **参数错位**：
// 查询返回 0 行、或把 A 的值写进 B 的列，且完全静默。
//
// 与 SQLite 版的 `renumber_params` 逐条对照测试：两个函数在「字面量保护」
// 与「越界抛错」上必须行为一致，否则换库时只有一边被验过。

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { to_pg } from './postgres.ts'

test('?N → $N，参数按编号取', () => {
  const r = to_pg('SELECT * FROM t WHERE a = ?1 AND b = ?2', ['x', 'y'])
  assert.equal(r.text, 'SELECT * FROM t WHERE a = $1 AND b = $2')
  assert.deepEqual(r.values, ['x', 'y'])
})

test('乱序 ?N 仍按编号取参数（与 SQLite 版同语义）', () => {
  const r = to_pg('INSERT INTO t(a,b) VALUES(?2, ?1)', ['A', 'B'])
  assert.equal(r.text, 'INSERT INTO t(a,b) VALUES($2, $1)')
  assert.deepEqual(r.values, ['B', 'A'])
})

test('字符串字面量里的 ?1 不是占位符', () => {
  const r = to_pg("SELECT * FROM t WHERE a = ?1 AND b LIKE '%?2%'", ['A'])
  assert.equal(r.text, "SELECT * FROM t WHERE a = $1 AND b LIKE '%?2%'")
  assert.deepEqual(r.values, ['A'])
})

test('字面量在改写后原样放回', () => {
  const r = to_pg("SELECT '?1' AS lit, ?1 AS v", ['V'])
  assert.ok(r.text.includes("'?1'"), `字面量应原样保留: ${r.text}`)
  assert.deepEqual(r.values, ['V'])
})

test('占位符越界抛错（与 SQLite 版一致）', () => {
  assert.throws(() => to_pg('SELECT * FROM t WHERE a = ?3', ['A']), /越界/)
})

test('裸 ? 抛错：PG 只接受 $N', () => {
  // PG 没有 `?` 占位符，传过去是语法错误。显式抛错比让驱动报错更清楚
  assert.throws(() => to_pg('SELECT * FROM t WHERE a = ? AND b = ?1', ['A', 'B']), /裸 \?/)
})

// ---------------------------------------------------------------- 方言转换

test('INSERT OR IGNORE → INSERT（ON CONFLICT 需在语句里显式写）', () => {
  const r = to_pg('INSERT OR IGNORE INTO q (a) VALUES (?1)', ['x'])
  assert.equal(r.text, 'INSERT INTO q (a) VALUES ($1)')
  // 子句数据层补不出来（要���冲突目标），故测试在此**固定**这个约束
  assert.ok(!r.text.includes('ON CONFLICT'), '数据层不应凭空生成 ON CONFLICT 子句')
})

test('unixepoch()*1000 → bigint 时间戳', () => {
  const r = to_pg("SELECT unixepoch() * 1000 AS t FROM x WHERE a = ?1", ['k'])
  assert.ok(
    r.text.includes('(EXTRACT(EPOCH FROM now())*1000)::bigint'),
    `时间戳表达式应被转换: ${r.text}`,
  )
  // ⚠️ 必须带 ::bigint —— PG 的 EXTRACT 返回 numeric，赋给 bigint 列会隐式转换，
  // 但反过来（列是 numeric 而表达式是 bigint）在某些版本会报错。
  assert.ok(!r.text.includes('unixepoch()'), '不应残留 SQLite 函数')
})

test('INTEGER PRIMARY KEY AUTOINCREMENT → SERIAL PRIMARY KEY', () => {
  const r = to_pg('CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT, a TEXT)', [])
  assert.equal(r.text, 'CREATE TABLE t (id SERIAL PRIMARY KEY, a TEXT)')
})

test('RETURNING 子句与条件递增原样保留（ai.ts 依赖）', () => {
  const sql = `UPDATE ai_quota SET used = used + 1
      WHERE user_id = ?1 AND day = ?2 AND used < granted
      RETURNING granted - used AS remaining`
  const r = to_pg(sql, [1, '2026-09-30'])
  assert.ok(r.text.includes('RETURNING granted - used AS remaining'))
  assert.ok(r.text.includes('used < granted'), '条件不得被改写掉')
  assert.deepEqual(r.values, [1, '2026-09-30'])
})

test('部分唯一索引的 WHERE 子句原样保留', () => {
  // schema.postgres.sql 依赖它做「同一扩展只允许一条未终结提交」
  const sql = 'CREATE UNIQUE INDEX IF NOT EXISTS idx ON submissions(user_id, ext_id) WHERE status IN (1,2)'
  const r = to_pg(sql, [])
  assert.ok(r.text.includes('WHERE status IN (1,2)'))
})

test('ON CONFLICT 子句不被二次改写', () => {
  const sql = 'INSERT INTO q (a) VALUES (?1) ON CONFLICT (a) DO NOTHING'
  const r = to_pg(sql, ['x'])
  assert.equal(r.text, 'INSERT INTO q (a) VALUES ($1) ON CONFLICT (a) DO NOTHING')
})
