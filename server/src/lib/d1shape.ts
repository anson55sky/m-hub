// 把 D1 的调用形状包成「看起来还是 D1」，从而让 24 处 `prepare().bind()` 调用点
// **一行都不用改**。
//
// ## 为什么不直接改调用点
//
// 24 处调用点、跨 6 个文件，全是 `await env.DB.prepare(sql).bind(a, b).run()` 这种
// 链式形状。逐个改成 `db.run(sql, a, b)` 看着机械，实则是 24 处**同时**改动 ——
// 而这次移植的唯一目的是「换个运行时」，不是「改数据访问风格」。
// 保持调用点不动，diff 就只有「新增两个文件 + 改 Env 一行」，可审、可回滚。
//
// 代价是多一层薄包装（约 40 行）。这个代价换来的是**移植的 diff 小到能一眼看完**，
// 值得。

import type { Db, Row, RunResult, SqlValue } from './db.ts'

/** 链式查询构造器，形状与 D1 的 `D1PreparedStatement` 对齐。 */
class Stmt {
  // ⚠️ 显式字段而非参数属性：生产的 `--experimental-strip-types` 是
  // strip-only，参数属性需要代码变换才能变成字段。见 check-strip-only-safe.mjs。
  private db: Db
  private sql: string
  private params: SqlValue[]

  constructor(db: Db, sql: string, params: SqlValue[] = []) {
    this.db = db
    this.sql = sql
    this.params = params
  }

  /** 追加位置参数。**可以多次调用**，参数按调用顺序拼接（D1 同此行为）。 */
  bind(...params: SqlValue[]): Stmt {
    return new Stmt(this.db, this.sql, [...this.params, ...params])
  }

  async first<T>(): Promise<T | null> {
    return this.db.first<T>(this.sql, ...this.params)
  }

  async all<T>(): Promise<{ results: T[]; success: true }> {
    const results = await this.db.all<T>(this.sql, ...this.params)
    // D1 的 `.all()` 返回 `{results, success}`，调用点写的是 `rows.results`
    return { results, success: true }
  }

  async run(): Promise<{ success: true; meta: { changes: number; last_row_id: number } }> {
    const r = await this.db.run(this.sql, ...this.params)
    return { success: true, meta: { changes: r.changes, last_row_id: r.lastId } }
  }
}

/**
 * 包一层 D1 形状的 `DB`。
 *
 * ⚠️ **参数顺序**：`schema.sql` 与全部 SQL 都用 `?N` 编号形式。
 * `node:sqlite` 支持 `?N`，但**不支持**「`?1` 与裸 `?` 混用」——
 * 那种情况下 SQLite 按裸 `?` 的出现顺序自行编号，与 D1 的行为不同。
 * 故本模块不转换参数，只强制约定：**所有 SQL 统一 `?N`**。
 * `assert_numbered_params` 就是守这条的，测试里逐条扫。
 */
export class D1Shape {
  // ⚠️ 刻意**不用** TypeScript 的参数属性语法（`constructor(private db: Db)`）：
  // 生产以 `node --experimental-strip-types` 直接跑 .ts，而该模式是
  // **strip-only**（只擦类型、不做代码变换），不支持参数属性 / enum / namespace。
  // 写成字段 + 显式赋值，代价是多两行，收益是「加了新语法不会在生产才炸」。
  // `scripts/check-strip-only-safe.mjs` 守着这条。
  private db: Db

  constructor(db: Db) {
    this.db = db
  }

  prepare(sql: string): Stmt {
    return new Stmt(this.db, sql)
  }
}

export type { Db, Row, RunResult, SqlValue }

/**
 * 扫出「用了裸 `?`」的 SQL。
 *
 * D1 允许裸 `?`（按出现顺序编号），而我们要求统一 `?N` —— 混用时
 * `node:sqlite` 的编号规则与 D1 不同，症状是**参数错位**：查询返回 0 行、
 * 或写进了错误的列。这类 bug 极难从现象反推，故做成可扫的纯函数。
 *
 * 实现上必须跳过 `?N` 本身与字符串字面量里的 `?`，否则 `'a?b'` 会被误报。
 */
export function assert_numbered_params(sql: string): string[] {
  const problems: string[] = []
  // 去掉字符串字面量，避免把 `'a?b'` / `LIKE '%?%'` 误判
  const stripped = sql.replace(/'[^']*'/g, "''")
  // 去掉 ?N 形式，剩下的裸 ? 才是问题
  const bare = stripped.replace(/\?\d+/g, '')
  if (bare.includes('?')) {
    problems.push(sql.replace(/\s+/g, ' ').trim().slice(0, 80))
  }
  return problems
}
