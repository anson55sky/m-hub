# schema.sql（SQLite）↔ schema.postgres.sql（PostgreSQL）差异

`schema.postgres.sql` 由 `schema.sql` **自动转换**而来（方言级替换，其余逐字保留，
含全部注释与业务约束）。转换点只有 4 类：

| # | SQLite | PostgreSQL | 处数 |
|---|---|---|---|
| 1 | `INTEGER PRIMARY KEY AUTOINCREMENT` | `SERIAL PRIMARY KEY` | 4 |
| 2 | `unixepoch()*1000` | `(EXTRACT(EPOCH FROM now())*1000)::bigint` | 5 |
| 3 | `PRAGMA foreign_keys = ON` | 无对应 → 靠应用层保证 | 1 |
| 4 | `INSERT OR IGNORE` | `INSERT … ON CONFLICT DO NOTHING` | 2 |

## 第 4 条为什么不在 schema 层转换

`ON CONFLICT DO NOTHING` 需要知道**冲突目标**（`ON CONFLICT (user_id, day)`），
那是语句级的信息，schema 里没有。所以子句写在 `src/routes/ai.ts` 的具体语句里。

## 第 3 条会不会丢外键约束

SQLite 的 `PRAGMA foreign_keys` 是**连接级开关**，而 PostgreSQL 支持
`ALTER TABLE … ADD CONSTRAINT`。本项目没转成真外键，理由：

- 所有写入方都是自己的代码（`routes/*.ts`），应用层已保证引用完整性
- 加真外键会在**数据导入顺序**上引入硬约束 —— 迁移 dump 时若子表先于父表到达，
  导入直接失败，而这是当前最可能出问题的一步

**若将来出现数据一致性问题，这里是第一个该补的地方。**

## ⚠️ 两份 schema 必须同步改

忘了同步的症状**很隐蔽**：PG 侧建表少了某列，应用照旧按旧结构查 ——
表现为某条接口一直返回空、或某字段是 `undefined`，而建表本身**不报错**。

## 占位符不在 schema 里

`?N` → `$N` 由 `src/lib/postgres.ts::to_pg` 在**运行时**转换，
所以 schema 里不含任何占位符。
