// `Db` 的 SQLite 实现（Node 侧；PocketBay / 本地开发都用它）。
//
// ## 关键差异：node:sqlite **不支持** `?N` 编号参数
//
// 实测：`db.prepare('… VALUES(?1,?2)').run(a, b)` → `column index out of range`。
// node:sqlite 只认**裸 `?`**，按出现顺序绑定。
// 而 D1 惯用 `?N`（本项目 24 处调用点全是这个形状）。
//
// 所以这里做 **`?N` → 裸 `?` 的重编号改写**。这也是为什么数据层必须自己解析 SQL，
// 而不能直接把语句丢给底层驱动。
//
// ## 改写的正确性论证
//
// · 只改**占位符**，不动字面量与标识符 —— 故先剥掉单引号字符串再改
// · `?N` 与裸 `?` 混用时，裸 `?` 的编号由 SQLite 按出现顺序算；我们统一改写成
//   裸 `?` 后，顺序与**参数传入顺序**一致（`bind(a,b,c)` → 三个 `?` 从左到右）
// · 重复的 `?1`（同一参数用两次）改写后会变成两个 `?`、需要传两个值 ——
//   本项目没有这种写法，但 [renumber_params] 有测试钉住「同一 ?N 出现两次」
//   的行为，宁可改成显式重复参数也不猜。
//
// ## 为什么用 `node:sqlite` 而不是 better-sqlite3
//
// `node:sqlite` 是**内置**模块：无需编译原生代码、无需 `node-gyp`、无供应链风险。
// better-sqlite3 要下载预编译二进制或现场编译，在容器里遇到「构建节点磁盘不足」
// 这类平台故障时会变成纯运气问题。
// 代价：Node 22/23 上带 `ExperimentalWarning`。已确认其 API 面（RETURNING、
// 部分唯一索引、`unixepoch()`、`INSERT OR IGNORE`）与我们用到的完全一致。

import { DatabaseSync, type StatementSync } from 'node:sqlite'
import type { Db, Row, RunResult, SqlValue } from './db.ts'

/**
 * 把 SQL 里的 `?N` 占位符改写成裸 `?`，并返回**参数展开列表**。
 *
 * @returns `{ sql, params }` —— `params` 是按裸 `?` 出现顺序展开后的值列表，
 *          长度必然等于语句里裸 `?` 的个数。
 */
export function renumber_params(sql: string, params: SqlValue[]): { sql: string; params: SqlValue[] } {
  // 先剥掉字符串字面量：'a?1b' 里的 ?1 不是占位符
  const literals: string[] = []
  const masked = sql.replace(/'[^']*'/g, (m) => {
    literals.push(m)
    return `'\u0000${literals.length - 1}\u0000'`
  })

  const out: SqlValue[] = []
  const rewritten = masked.replace(/\?(\d+)/g, (_, n: string) => {
    const idx = Number(n)
    if (idx < 1 || idx > params.length) {
      // 参数个数与占位符对不上：宁可炸在这里，也不要静默绑错值
      throw new Error(`SQL 占位符 ?${idx} 越界（只传了 ${params.length} 个参数）: ${sql.slice(0, 80)}`)
    }
    out.push(params[idx - 1]!)
    return '?'
  })

  // 若语句里还有裸 `?`（未被 ?N 覆盖），它们按出现顺序对应「剩余参数」——
  // 这种混用极罕见，此处显式报错而不是猜（约定 47 的精神：含糊就吵）
  const bareCount = (rewritten.replace(/\?/g, ''), (rewritten.match(/\?/g) ?? []).length)
  if (bareCount !== out.length) {
    throw new Error(
      `SQL 混用了裸 ? 与 ?N（裸 ${bareCount} 个 / 展开 ${out.length} 个），` +
        `无法确定绑定顺序。请统一改用 ?N。`,
    )
  }

  // 把字符串字面量放回去
  const restored = rewritten.replace(/'\u0000(\d+)\u0000'/g, (_, i: string) => literals[Number(i)]!)
  return { sql: restored, params: out }
}

export class SqliteDb implements Db {
  private db: DatabaseSync
  private cache = new Map<string, StatementSync>()
  /** 串行化写操作：node:sqlite 是同步 API，并发 await 不会并行推进 */
  private queue: Promise<unknown> = Promise.resolve()

  constructor(path: string) {
    this.db = new DatabaseSync(path)
    // WAL：读写不互相阻塞。并发写在本项目量级下不成问题，但读不该被写卡住
    this.db.exec('PRAGMA journal_mode = WAL')
    this.db.exec('PRAGMA foreign_keys = ON')
    this.db.exec('PRAGMA busy_timeout = 5000')
  }

  private stmt(sql: string): StatementSync {
    let s = this.cache.get(sql)
    if (!s) {
      s = this.db.prepare(sql)
      this.cache.set(sql, s)
    }
    return s
  }

  private async serial<T>(fn: () => T | Promise<T>): Promise<T> {
    // 链式排队：保证写操作按调用顺序执行（node:sqlite 同步，但 async 函数
    // 之间仍会交错，不排队会出现「后写的先落」的次序错乱）
    const p = this.queue.then(fn, fn)
    this.queue = p.catch(() => {})
    return p
  }

  async first<T>(sql: string, ...params: SqlValue[]): Promise<T | null> {
    const { sql: q, params: ps } = renumber_params(sql, params)
    // node:sqlite 的 .get() 无行时返回 undefined；D1 的 .first() 返回 null。
    // 归一为 null，让上层 `row ?? null` 的写法在两个运行时下都成立。
    const row = this.stmt(q).get(...(ps as never[])) as T | undefined
    return row ?? null
  }

  async all<T>(sql: string, ...params: SqlValue[]): Promise<T[]> {
    const { sql: q, params: ps } = renumber_params(sql, params)
    return this.stmt(q).all(...(ps as never[])) as T[]
  }

  async run(sql: string, ...params: SqlValue[]): Promise<RunResult> {
    const { sql: q, params: ps } = renumber_params(sql, params)
    return this.serial(() => {
      const r = this.stmt(q).run(...(ps as never[]))
      return { changes: Number(r.changes), lastId: Number(r.lastInsertRowid) }
    })
  }

  async exec(sql: string): Promise<void> {
    // 建表脚本是**多条**语句（分号分隔），node:sqlite 的 prepare 只吃单条 → 用 exec
    await this.serial(() => {
      this.db.exec(sql)
    })
  }

  close(): void {
    // 预编译语句随连接一起释放（`DatabaseSync.close()` 会一并处理），
    // 故只需清缓存并关连接。⚠️ 别调 `StatementSync.finalize()` ——
    // 本版本（Node 22.x 的 node:sqlite）没有这个方法，调了直接抛 TypeError。
    this.cache.clear()
    this.db.close()
  }
}

export type { Db, Row, SqlValue }
