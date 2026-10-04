// 市场清单的**运行期**端点：签名在服务端做，字节存在 D1。
//
// ## 为什么清单从「静态文件」变成「Function 应答」（2026-10-02）
//
// 原来是构建期脚本写的静态文件 + 签名的 `.sig`。改成运行期签名之后：
//
//   · **上架有了按钮**：审核通过 → 点「上架」→ 清单当场更新，不需要任何人
//     在某台机器上跑命令（用户报的就是「没有上架按钮、市场还是旧版」）。
//   · **两端字节恒一致**：字节与签名存在 D1 的**同一行**，两个端点读同一个值。
//
// ## 为什么必须存 D1、不能每次请求现签
//
// 客户端取的是**两个独立请求**（`/registry` 与 `/registry.sig`）再验签。
// 现签的话两次请求可能落在不同的 D1 快照 / 不同的边缘节点，只要中间恰好
// 有一次重建，两边字节就对不上 → 客户端报「验签失败」。
// 症状与「被篡改」一模一样，但原因在无害的一侧 —— 这种歧义最费时间。
//
// ## 首次访问还没有记录时
//
// 回 503 而不是回一份空清单：空清单会让客户端把市场**清空**（用户看着像
// 「扩展全没了」），而真实原因是「还没上架过任何东西」。两者必须可区分。
import { json, type Ctx } from '../lib/http.ts'
import { loadRegistryRecord } from '../lib/marketSign.ts'
import { getBlob } from '../lib/pkgStore.ts'

/** GET /api/v1/market/registry —— 已签名的清单字节（原文，不重新序列化） */
export async function marketRegistry(ctx: Ctx): Promise<Response> {
  const rec = await loadRegistryRecord(ctx)
  if (!rec) {
    return json(
      {
        error: 'registry_not_ready',
        message: '市场清单还没有生成过。请先在审核台把某个扩展「上架」。',
      },
      503,
    )
  }
  return new Response(rec.bytes, {
    headers: {
      'content-type': 'application/json; charset=utf-8',
      // 清单变了客户端必须立刻看到，且绝不能被 CDN/中间层重压缩 ——
      // 签名覆盖的是**精确字节**，gzip 重压缩会改变它。
      'cache-control': 'no-store',
    },
  })
}

/** GET /api/v1/market/registry.sig —— 与上面那份字节配对的 base64 签名 */
export async function marketRegistrySig(ctx: Ctx): Promise<Response> {
  const rec = await loadRegistryRecord(ctx)
  if (!rec) {
    return json(
      { error: 'registry_not_ready', message: '市场清单还没有生成过。' },
      503,
    )
  }
  return new Response(rec.sig, {
    headers: {
      'content-type': 'text/plain; charset=utf-8',
      'cache-control': 'no-store',
    },
  })
}
/**
 * GET /api/v1/market/shot/:id —— 发布弹窗上传的截图（2026-10-04）。
 *
 * ## 这个端点补的是一个洞，不是一个新功能
 *
 * 作者在发布弹窗选图 → 客户端 multipart 上传 → 服务端校验大小与数量 →
 * **然后把字节丢掉了**。市场清单里的 `screenshots` 一直读的是扩展 manifest 里
 * 作者自己写的 URL，于是「上传了 3 张图」与服务端回的 `screenshots: 3` 都是真的，
 * 那 3 个字节却从来没有被存下、也没有任何地方读得到它们。
 *
 * ## 为什么按「资产行 id」而不是 sha 取
 *
 * ① **上线前不可见**（约定 51）：查询带了 `submissions.status='published'`，
 *   所以未上架提交的截图拿不到公开地址 —— 哪怕别人猜到了 id。
 *   用 sha 当路径的话这个约束就得另写一套反查，而 sha 是内容的散列、
 *   万一泄漏就是永久可读（内容寻址在这里成了缺点）。
 * ② 顺带拿到 `size`（分块存储拼回需要总长）与文件名，**不需要**额外的元数据表。
 *
 * ## Content-Type 由我们判，不看上传时声明的那个
 *
 * `multipart` 的 `File.type` 由客户端给。提交时已经按**文件头**判过一次并落了
 * 合法扩展名，这里按扩展名回 `content-type` —— 即便判断被人绕过，
 * 也不会把 `text/html` 当图片发出去。
 */
export async function marketShot(ctx: Ctx, idStr: string): Promise<Response> {
  const id = Number(idStr)
  // ⚠️ 先按形状挡掉再查库：`Number('')` 是 0、`Number('1abc')` 是 NaN，
  //   而 `/api/v1/market/shot/abc` 会命中这条路由 —— 不挡就是一次全表扫。
  if (!Number.isInteger(id) || id <= 0) {
    return json({ error: 'not_found', message: '没有这张截图' }, 404)
  }
  const row = await ctx.env.DB.prepare(
    `SELECT a.filename AS filename, a.size AS size, a.sha256 AS sha256
       FROM submission_assets a
       JOIN submissions s ON s.id = a.submission_id
      WHERE a.id = ?1 AND a.asset_kind = 'shot' AND s.status = 'published'`,
  )
    .bind(id)
    .first<{ filename: string; size: number; sha256: string }>()
  if (!row) {
    // ⚠️ 已删除/未上架一律 404，**不区分**：区分开等于告诉外界
    // 「这个 id 存在但还没上架」，那是另一种信息泄漏。
    return json({ error: 'not_found', message: '没有这张截图' }, 404)
  }
  const bytes = await getBlob(ctx.env.DB, row.sha256, row.size)
  if (!bytes) {
    // 资产行在、字节没了：只能是把 blob 删了。这要说出来（500 而不是 404），
    // 否则作者看到的是「图裂了」，而真正的原因在服务端。
    return json({ error: 'shot_blob_missing', message: '截图数据缺失，请重新上传' }, 500)
  }
  const ext = row.filename.split('.').pop() ?? 'png'
  const mime =
    ext === 'jpg' ? 'image/jpeg' : ext === 'webp' ? 'image/webp' : 'image/png'
  return new Response(bytes as unknown as BodyInit, {
    headers: {
      'content-type': mime,
      // 内容寻址：同样的 sha 永远是同样的字节，可以长缓存。
      // ⚠️ 但**不带 nosniff 之外的校验** —— 这里必须是 immutable 而不是 max-age，
      //   因为「已上架」这个条件会变（扩展被下架后就不该再从 CDN 命中）。
      'cache-control': 'public, max-age=3600',
      'x-content-type-options': 'nosniff',
    },
  })
}
