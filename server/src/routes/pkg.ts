// 扩展包体的**运行期**分发：从 D1 的 blob 里流式发出上传的那份字节。
//
// ## 为什么不再重新打包（2026-10-02）
//
// 原来上架要做一次「解包 → 搬到 `manifests/<id>` → `seed-manifests` 重新 zip
// → 写 `public/packages/` → 部署」。于是**发出去的包**与**作者上传的包**是两份
// 不同的字节 —— 重新打包的 zip 压缩参数、条目顺序都可能不同。
//
// 后果被清单自检当场抓到：清单声明 `size: 7775`（作者上传的大小，来自
// `submissions.pkg_size`），而实际部署的文件是 **7853** 字节。
// 客户端安装时会校验 sha256 —— 对不上就是**安装失败**，
// 而清单与签名都合法、服务端全 200，只有真人点一次「安装」才暴露。
//
// 改成直接发 blob 之后，`pkg_sha256` / `pkg_size` 就是**所发字节**的
// sha256 与长度，构造上不可能再漂 —— 少了「重新打包」这一环，也少了
// 「清单说一个字节、文件是另一个字节」这一类问题。
//
// ## 为什么按 extId@version 查、不把 sha256 放进 URL
//
// 清单里已经有 sha256，客户端会自己校验；URL 里再带一份就成了第二份真相，
// 而它是可以被单独改坏的。按 extId@version 查更简单：唯一的真相是
// `submissions` 里那一行。
import { getBlob } from '../lib/pkgStore.ts'
import { fail, type Ctx } from '../lib/http.ts'

const MAX_PKG_BYTES = 64 * 1024 * 1024

/**
 * GET /packages/:extId/:version/:file
 *
 * `:file` 只是为了与既有 URL 形状一致（`{id}-{version}.xhpack`），**不参与匹配** ——
 * 取包只看 `:extId` 与 `:version`。让文件名可任意变化而不改变行为，
 * 这样清单里拼错文件名不会导致 404 之外更难排查的结果。
 */
export async function extensionPackage(ctx: Ctx, params: Record<string, string>): Promise<Response> {
  const extId = params.extId ?? ''
  const version = params.version ?? ''

  // ⚠️ id/version 都要校验形状：它们会拼进 SQL 的值（已用绑定参数，不存在注入），
  // 但非法形状会一路查到 404，把「参数写错」和「真的没这个扩展」混成同一件事。
  if (!/^[a-z0-9][a-z0-9._-]{0,63}$/i.test(extId) || !/^\d+\.\d+\.\d+$/.test(version)) {
    return fail(404, 'not_found', '扩展 id 或版本号格式不对')
  }

  const row = await ctx.env.DB.prepare(
    `SELECT pkg_sha256 AS pkgSha256, pkg_size AS pkgSize
       FROM submissions
      WHERE ext_id = ?1 AND version = ?2 AND status = 'published'
      LIMIT 1`,
  )
    .bind(extId, version)
    .first<{ pkgSha256: string | null; pkgSize: number | null }>()

  // 只有**已上架**的版本可下载。未上架的给 404 而不是 403：
  // 「这个版本还没上架」不该让下载者知道它的存在（403 等于承认它存在）。
  if (!row || !row.pkgSha256 || !row.pkgSize) {
    return fail(404, 'not_found', `没有这个扩展版本：${extId}@${version}`)
  }
  if (row.pkgSize > MAX_PKG_BYTES) {
    return fail(413, 'too_large', '包体超出上限')
  }

  const bytes = await getBlob(ctx.env.DB, row.pkgSha256, row.pkgSize)
  if (!bytes) {
    // ⚠️ 「清单说有、实际取不到」是最坏的状态：用户点安装得到 404，
    //   而清单与签名都合法。必须能被一眼认出来，而不是笼统的 404。
    return fail(500, 'package_missing', '包体在库里找不到（清单与实际内容不一致）')
  }

  return new Response(bytes as unknown as BodyInit, {
    headers: {
      'content-type': 'application/zip',
      'content-length': String(bytes.byteLength),
      // 内容寻址：URL 里的版本变了就是另一份字节，可以长缓存。
      // 同一个版本的内容永不改变（内容寻址的字节）。
      'cache-control': 'public, max-age=31536000, immutable',
      etag: `"${row.pkgSha256.slice(0, 16)}"`,
    },
  })
}