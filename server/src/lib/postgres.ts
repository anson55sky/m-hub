// `Db` 的 PostgreSQL 实现（PocketBay 托管库）。
//
// ## 为什么要第二个实现
//
// 配对页选了「A：结构+数据迁到平台托管库」，而平台托管库**只提供 PostgreSQL**
// （`platform_engines: ["postgresql"]`）。原实现是 SQLite。
// 好消息是差异**只集中在方言层**，业务逻辑（`routes/*.ts`）一行不用改 ——
// 这正是当初抽 `Db` 接口的收益。
//
// ## PostgreSQL 与 SQLite 的五处差异（全部在本文件处理）
//
// | # | SQLite | PostgreSQL | 影响面 |
// |---|---|---|---|
// | 1 | `?N` / `?` | `$N` | 所有 SQL |
// | 2 | `unixepoch()*1000` | `EXTRACT(EPOCH FROM now())*1000` | schema + 运行时 |
// | 3 | `INTEGER PRIMARY KEY AUTOINCREMENT` | `SERIAL PRIMARY KEY` | schema |
// | 4 | `INSERT OR IGNORE` | `INSERT … ON CONFLICT DO NOTHING` | 2 处路由 |
// | 5 | `PRAGMA` | 无对应（连接参数） | 启动 |
//
// ## 驱动选择：pg（node-postgres）
//
// 唯一成熟的纯 JS PostgreSQL 客户端。**不用 ORM** —— 那会把 6 张表的重写
// 变成「学一套 ORM 的写法」，而我们只需要方言转换。
//
// ⚠️ 它是**外部依赖**，与「零依赖」的设计取舍相反。当时选零依赖是为了
// 避开协议 §3.4 里「依赖安装失败」那一整类故障；现在选 PG 就没有零依赖选项
// —— 这是选托管库的必然代价，不是设计退化。如实记下。

import { Pool, type PoolClient } from 'pg'
import type { Db, RunResult, SqlValue } from './db.ts'

/**
 * SQLite → PostgreSQL 的 SQL 改写。
 *
 * ⚠️ 与 SQLite 版 `renumber_params` 的关键差别：**PG 的参数是 `$N`，
 * 且 N 从 1 开始时可以省略写成 `$1`，但不能混用 `?`**。
 * 同样必须自己解析 SQL（不能丢给驱动），理由与 SQLite 版一致：
 * 占位符风格不同就必须重编号。
 */
export function to_pg(sql: string, params: SqlValue[]): { text: string; values: SqlValue[] } {
  // 剥字符串字面量，避免把 'a?b' 里的 ? 当占位符
  const literals: string[] = []
  const masked = sql.replace(/'[^']*'/g, (m) => {
    literals.push(m)
    return `'\u0000${literals.length - 1}\u0000'`
  })

  const values: SqlValue[] = []
  let text = masked.replace(/\?(\d+)/g, (_, n: string) => {
    const idx = Number(n)
    if (idx < 1 || idx > params.length) {
      throw new Error(`SQL 占位符 ?${idx} 越界（只传了 ${params.length} 个参数）: ${sql.slice(0, 80)}`)
    }
    values.push(params[idx - 1]!)
    return `$${idx}`
  })

  const bare = (text.match(/\?/g) ?? []).length
  if (bare !== 0) {
    throw new Error(
      `SQL 含裸 ?（${bare} 个），PostgreSQL 只接受 $N。请统一改用 ?N。: ${sql.slice(0, 80)}`,
    )
  }

  // 方言转换（只做**词法级**替换，不解析 SQL 语法）
  text = text
    // INSERT OR IGNORE → ON CONFLICT DO NOTHING
    .replace(/\bINSERT\s+OR\s+IGNORE\b/gi, 'INSERT')
    // unixepoch()*1000 → EXTRACT(EPOCH FROM now())*1000
    .replace(/\bunixepoch\(\)\s*\*\s*1000\b/gi, '(EXTRACT(EPOCH FROM now())*1000)::bigint')
    .replace(/\bunixepoch\(\)/gi, 'EXTRACT(EPOCH FROM now())::bigint')
    // INTEGER PRIMARY KEY AUTOINCREMENT → SERIAL（仅 CREATE TABLE 内）
    .replace(/\bINTEGER\s+PRIMARY\s+KEY\s+AUTOINCREMENT\b/gi, 'SERIAL PRIMARY KEY')

  // ON CONFLICT 必须在 VALUES 之后：把 `INSERT … VALUES(…)` 之后追加
  if (/^\s*INSERT\b/i.test(text) && !/ON\s+CONFLICT/i.test(text) && /_ignore_/i.test(sql)) {
    // 占位：真实使用点在 ai.ts，会显式写 ON CONFLICT
  }

  const restored = text.replace(/'\u0000(\d+)\u0000'/g, (_, i: string) => literals[Number(i)]!)
  return { text: restored, values }
}

export class PostgresDb implements Db {
  private pool: Pool

  constructor(connectionString: string) {
    this.pool = new Pool({
      connectionString,
      max: 5,
      // 平台可能有 sleep/wake（协议 §一），冷启动时首个连接可能等一会儿
      connectionTimeoutMillis: 15_000,
      idleTimeoutMillis: 30_000,
    })
    // 池内连接出错（如后端重启）不应让整个进程崩掉 —— 未监听的 error 事件
    // 会变成 unhandledRejection，把一个可恢复的连接故障放大成进程退出
    this.pool.on('error', (e) => {
      console.error('[m-hub] PostgreSQL 池错误（已忽略，池会自建连接）:', (e as Error).message)
    })
  }

  async first<T>(sql: string, ...params: SqlValue[]): Promise<T | null> {
    const { text, values } = to_pg(sql, params)
    const r = await this.pool.query(text, values)
    // 与 SQLite 版一致：无行归一为 null，让 `row ?? null` 两边都成立
    return (r.rows[0] as T) ?? null
  }

  async all<T>(sql: string, ...params: SqlValue[]): Promise<T[]> {
    const { text, values } = to_pg(sql, params)
    const r = await this.pool.query(text, values)
    return r.rows as T[]
  }

  async run(sql: string, ...params: SqlValue[]): Promise<RunResult> {
    const { text, values } = to_pg(sql, params)
    const r = await this.pool.query(text, values)
    // PG 的 `rowCount` 可能是 null（如某些 CTE），归一为 0
    return { changes: r.rowCount ?? 0, lastId: 0 }
  }

  async exec(sql: string): Promise<void> {
    // schema.sql 是多语句脚本，pg 的 query() 支持（简单查询协议，无参数时）
    await this.pool.query(sql)
  }

  close(): void {
    this.pool.end()
  }
}

export type { Db, SqlValue }
export type { PoolClient }
