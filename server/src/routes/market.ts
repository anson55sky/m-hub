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