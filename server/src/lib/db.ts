// 数据访问层：**一份 SQL，两种运行时**。
//
// ## 为什么要抽这一层
//
// 原实现直接用 D1（`env.DB.prepare().bind().first()`）。PocketBay 不提供 D1
// （托管 PostgreSQL 仍是 beta，且它连的是另一个语法方言）。但**D1 底下就是
// SQLite**，`schema.sql` 用的 `unixepoch()` / `INSERT OR IGNORE` / 部分唯一索引
// 全是标准 SQLite —— 所以 SQL 语句**一行都不用改**，只有「怎么执行」要分家。
//
// 抽成接口后，`routes/*.ts` 完全不知道自己跑在 Workers 还是 Node 上。
//
// ## 判据：SQLite 语法够用吗
//
// 够。当前规模是「几十个用户、6 张表、每天几百次请求」。PostgreSQL 在这个量级
// 上换来的好处（并发写、复杂索引、JSON 能力）都用不上，而代价是：
// 托管库还在 beta、`schema.sql` 要重写成另一套方言、多一个「托管库开通失败」
// 的故障类别。**不过度设计**。

export type SqlValue = string | number | null | ArrayBuffer | Uint8Array

export interface Row {
  [k: string]: SqlValue
}

export interface RunResult {
  /** 受影响行数 */
  changes: number
  /** 自增主键（INSERT 后取 `lastInsertRowid`） */
  lastId: number
}

/**
 * 数据访问接口。
 *
 * 刻意**不用泛型默认值**（`first<T = Row>`）：D1 与 node:sqlite 的返回类型
 * 推断能力不同，宽松的默认值会掩盖「查出 0 行」和「查出 1 行」的区别。
 * 调用方必须显式写 `first<User>(...)`，让「拿到 null」这件事在类型上可见。
 */
export interface Db {
  /** 取第一行；无行则 `null` */
  first<T>(sql: string, ...params: SqlValue[]): Promise<T | null>
  /** 取全部行 */
  all<T>(sql: string, ...params: SqlValue[]): Promise<T[]>
  /** 执行写操作 */
  run(sql: string, ...params: SqlValue[]): Promise<RunResult>
  /** 执行不带参数的多语句脚本（建表用） */
  exec(sql: string): Promise<void>
  /** 关闭底层连接 */
  close(): void
}

/**
 * 把 D1 的 `?N` 位置参数风格归一。
 *
 * `routes/*.ts` 里两种写法都在用：`?1`（D1 惯用，且 `users.ts` 有用到）
 * 与裸 `?`。node:sqlite **两种都支持** `?N` 形式，但裸 `?` 混用时
 * 「`?1` 之后又出现裸 `?`」的编号是 SQLite 自己按出现顺序算的，容易错位。
 * 故这里不做转换，只在文档里要求**统一用 `?N`**。
 */
export const SQL_PARAMS_STYLE = '所有 SQL 统一用 ?N 位置参数，不要用裸 ?'
