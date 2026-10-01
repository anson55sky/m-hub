// 扩展包体的分块存储（D1）与读取。
//
// ## 为什么分块
//
// 我想实测 D1 的单值上限，**但测不出来**：`wrangler d1 execute` 把 SQL 当命令行
// 参数传，实测 1KB 能写、64KB 就 `code: 7500` —— 那是 CLI 的参数长度限制，
// 不是 D1 的限制。真正的写入走 Function 里的 `env.DB` 绑定，不经命令行，
// 所以那个上限**无法用 CLI 探测**。
//
// 既然不能确认，就不依赖它：128KB 一块，任何可能的上限都够。
//
// ## 为什么不直接用 R2
//
// R2 需要先在 Cloudflare 控制台手工启用（还要绑支付方式），是用户操作。
// 本轮先把链路跑通，且**所有 IO 都收在这一个文件里** ——
// 将来换 R2 只需改 `putBlob` / `getBlob` 两个函数，调用方无感。
//
// ## 分片逻辑是纯函数
//
// `sliceChunks` 与 `joinChunks` 不碰 DB，可离线回归。**这很重要**：
// 分块最容易出的错是「末尾块算错」「空包」「正好整除」这类边界，
// 而它们只有跨过块边界才暴露 —— 用 128KB 的假数据测是可行的，不必真造 128KB。

export const CHUNK_SIZE = 128 * 1024

/**
 * 把字节切成定长块。
 *
 * ⚠️ 这里曾有两处「防御」代码，验过之后删掉了 —— 它们都是**死的**：
 * · `if (bytes.length === 0) return []`：循环条件 `off < 0` 本就不成立，
 *   空输入自然得到 0 块。删掉守卫行为不变。
 * · `Math.min(off + size, bytes.length)`：`subarray` 自身就会把结束下标夹到
 *   数组末尾（实测 `subarray(8, 131072)` 返回长度 2，不抛错）。
 *
 * 留着它们的唯一坏处是**误导**：下一个人会以为这里有边界要处理，
 * 于是不敢简化、也不敢改块大小。死代码比没有代码更贵。
 * （这两条都是靠约定 73 的变异测试发现的 —— 变异后测试**全绿**，
 *  说明断言压根没碰到那两行。）
 */
export function sliceChunks(bytes: Uint8Array, size = CHUNK_SIZE): Uint8Array[] {
  const out: Uint8Array[] = []
  for (let off = 0; off < bytes.length; off += size) {
    out.push(bytes.subarray(off, off + size))
  }
  return out
}

/** 拼回原始字节。`total` 用来丢弃尾部的空块（防御库里多出来的块）。 */
export function joinChunks(chunks: Uint8Array[], total: number): Uint8Array {
  const out = new Uint8Array(total)
  let off = 0
  for (const c of chunks) {
    if (off >= total) break
    const n = Math.min(c.length, total - off)
    out.set(c.subarray(0, n), off)
    off += n
  }
  return out
}

/** 小端十六进制，避免依赖 Buffer（Workers 运行时没有） */
export function toHex(bytes: Uint8Array): string {
  let s = ''
  for (const b of bytes) s += b.toString(16).padStart(2, '0')
  return s
}

async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const d = await crypto.subtle.digest('SHA-256', bytes as unknown as BufferSource)
  return toHex(new Uint8Array(d))
}

/**
 * 存一个包，返回 `sha256`。内容寻址：同样的字节重复存不会占用新空间。
 *
 * 幂等：同 sha256 再存一次会先清掉旧块，避免「上次写到一半」留下的残块
 * 混进来（那会让读回来的字节既不是旧版本也不是新版本）。
 */
export async function putBlob(db: D1Database, bytes: Uint8Array): Promise<string> {
  const sha = await sha256Hex(bytes)
  await db.prepare(`DELETE FROM pkg_blobs WHERE sha256 = ?1`).bind(sha).run()
  const chunks = sliceChunks(bytes)
  for (let i = 0; i < chunks.length; i++) {
    await db.prepare(`INSERT INTO pkg_blobs (sha256, idx, data) VALUES (?1, ?2, ?3)`)
      .bind(sha, i, chunks[i])
      .run()
  }
  return sha
}

/** 读回一个包；不存在返回 null。`total` 是原始总长度（分块后必须知道）。 */
export async function getBlob(db: D1Database, sha: string, total: number): Promise<Uint8Array | null> {
  const rows = await db.prepare(`SELECT data FROM pkg_blobs WHERE sha256 = ?1 ORDER BY idx`)
    .bind(sha)
    .all<{ data: ArrayBuffer | Uint8Array | number[] }>()
  if (!rows.results || rows.results.length === 0) return null
  const chunks = rows.results.map((r) =>
    r.data instanceof Uint8Array ? r.data : new Uint8Array(r.data as ArrayBuffer),
  )
  return joinChunks(chunks, total)
}

/** 包是否已在库里（发布脚本要先查：包丢了就不能上架） */
export async function hasBlob(db: D1Database, sha: string, total: number): Promise<boolean> {
  const n = sliceChunks(new Uint8Array(total)).length
  const row = await db.prepare(`SELECT COUNT(*) AS c FROM pkg_blobs WHERE sha256 = ?1`)
    .bind(sha)
    .first<{ c: number }>()
  return !!row && row.c === n
}