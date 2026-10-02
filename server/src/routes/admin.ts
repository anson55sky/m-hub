// 管理端：审核开发者申请、审核扩展提交。
//
// ## 为什么门禁放在 `requireAdmin` 而不是每个 handler 里各判一次
//
// 每个 handler 自己判 = 漏一个就是一个越权漏洞，而且**没有测试会发现**
// （少写一个 `if` 是编译期看不出来的）。集中在一处，漏的代价从「漏洞」
// 降到「多写一行 import」。
//
// ## 门禁是两层的，不是三层
//
// ① `currentUser` —— 有有效会话（401）
// ② `requireAdmin` —— `role === 'admin'`（403）
//
// ⚠️ 特别注意 **403 与 401 要分开**：401 = 没登录，403 = 登录了但不是管理员。
// 混成同一个码的话，客户端会把「你不是管理员」显示成「请先登录」——
// 用户会反复登录，而登录多少次都不会变。

import {
  fail,
  json,
  readJson,
  currentUser,
  unauthorized,
  timingSafeEqual,
  type Ctx,
  type Env,
} from '../lib/http.ts'
import { listZipEntries } from '../lib/zipdir.ts'
import { readZipEntry } from '../lib/zipread.ts'
import { runGate } from '../lib/gate.ts'
import { getBlob } from '../lib/pkgStore.ts'

/**
 * GET /api/v1/admin/submissions/:id/package —— 取包体原始字节。
 *
 * 存在的理由：`scripts/publish-approved.mjs` 需要拿到包才能上架，
 * 而它**刻意不直连 D1**（直连会绕过门禁与关卡）。
 *
 * ⚠️ 只对**已 approved** 的返回正文，其余一律 409。
 *   否则这个端点就是个「任意读用户上传内容」的洞 —— 而包是未审核的
 *   用户代码，即使只在本机 CLI 用，也不该从 pending_review 就能取。
 */
export async function submissionPackage(ctx: Ctx, params: Record<string, string>) {
  const admin = await requireAdmin(ctx)
  if (admin instanceof Response) return admin

  const id = Number(params.id)
  if (!Number.isInteger(id)) return fail(400, 'bad_request', 'id 不合法')
  const row = await ctx.env.DB.prepare(
    `SELECT pkg_sha256 AS pkgSha256, pkg_size AS pkgSize, status
       FROM submissions WHERE id = ?1`,
  )
    .bind(id)
    .first<{ pkgSha256: string | null; pkgSize: number | null; status: string }>()
  if (!row) return fail(404, 'not_found', '提交不存在')
  if (row.status !== 'approved') {
    return fail(409, 'not_approved', `该提交是 ${row.status}，只有 approved 能取包体`)
  }
  if (!row.pkgSha256 || !row.pkgSize) return fail(409, 'package_missing', '该提交没有包体')

  const bytes = await getBlob(ctx.env.DB, row.pkgSha256, row.pkgSize)
  if (!bytes) return fail(409, 'package_missing', '包体在库里找不到')

  return new Response(bytes as unknown as BodyInit, {
    headers: {
      'content-type': 'application/octet-stream',
      // ⚠️ 明确不缓存：包体按 sha256 变化，缓存错了就是「下到旧版本」。
      'cache-control': 'no-store',
    },
  })
}

/**
 * 机器凭据是否正确。
 *
 * ⚠️ **没配置时一律拒绝** —— 不能「没配就放行」，那种写法在
 *    「先上线、密钥后补」的过程中会让管理端裸奔。
 */
/** 审核时给关卡的 manifest 解压上限（与提交时同一量级） */
const MAX_MANIFEST_BYTES = 256 * 1024

function machineOk(env: Env, token: string | undefined): boolean {
  const expect = env.ADMIN_TOKEN
  if (!expect || !token) return false
  return timingSafeEqual(token, expect)
}

/** 必须是管理员。返回 `Response` 表示已经回过客户端，调用方直接 return。 */
export async function requireAdmin(ctx: Ctx): Promise<Awaited<ReturnType<typeof currentUser>> | Response> {
  // ① 机器凭据（CLI，见 scripts/review.mjs）
  //
  // ⚠️ 为什么不复用应用的会话 token：那个文件存在应用的私有目录里，
  //    让 CLI 去读等于让脚本伸手进另一个程序的家目录 —— 耦合、能读到用户
  //    身份、且用户重装应用就失效。机器凭据是**独立**的一条。
  if (machineOk(ctx.env, ctx.token)) {
    return {
      id: 0,
      github_id: null,
      username: 'cli',
      email: null,
      avatar: null,
      role: 'admin',
      is_developer: 1,
      developer_status: 'approved',
      invite_redeemed: 1,
    }
  }
  // ② 浏览器会话（将来的管理界面走这条）
  const user = await currentUser(ctx.env, ctx.token)
  if (!user) return unauthorized()
  if (user.role !== 'admin') {
    return fail(403, 'forbidden', '需要平台管理员权限')
  }
  return user
}

/** GET /api/v1/admin/dev-applications —— 开发者申请队列 */
export async function listDevApplications(ctx: Ctx) {
  const admin = await requireAdmin(ctx)
  if (admin instanceof Response) return admin
  const rows = await ctx.env.DB.prepare(
    `SELECT d.id, d.status, d.reason, d.review_note AS reviewNote,
            d.created_at AS createdAt, u.username, u.developer_status AS developerStatus
       FROM dev_applications d JOIN users u ON u.id = d.user_id
      ORDER BY d.created_at ASC`,
  )
    .all<Record<string, unknown>>()
  return json({ items: rows.results ?? [] })
}

/** POST /api/v1/admin/dev-applications/:id/approve —— body `{ note? }` */
export async function approveDevApplication(ctx: Ctx, params: Record<string, string>) {
  const admin = await requireAdmin(ctx)
  if (admin instanceof Response) return admin

  const id = Number(params.id)
  if (!Number.isInteger(id)) return fail(400, 'bad_request', 'id 不合法')
  const body = await readJson(ctx.req)
  const note = typeof body.note === 'string' ? body.note.slice(0, 500) : ''

  // ⚠️ 只允许从 pending 转。已 approved 的再批一次会让 updated_at 乱跳、
  //   也会把「已发布」的语义搞乱（约定 60 强调不要随手改 status）。
  const app = await ctx.env.DB.prepare(
    `SELECT d.id, d.status, u.id AS userId
       FROM dev_applications d JOIN users u ON u.id = d.user_id
      WHERE d.id = ?1`,
  )
    .bind(id)
    .first<{ id: number; status: string; userId: number }>()
  if (!app) return fail(404, 'not_found', '申请不存在')
  if (app.status !== 'pending') {
    return fail(409, 'already_reviewed', `该申请已是 ${app.status} 状态`)
  }

  // 关键：**users.developer_status 与 is_developer 都要改**。
  // 门禁查的是 `developer_status === 'approved'`（canApplyDeveloper 的反面），
  // 而 requireDeveloper 查的是 `is_developer`。只改一个的结果是
  // 「界面显示已通过，但发布扩展仍被拒」—— 症状与没通过一模一样。
  await ctx.env.DB.prepare(
    `UPDATE users SET developer_status = 'approved', is_developer = 1 WHERE id = ?1`,
  )
    .bind(app.userId)
    .run()
  await ctx.env.DB.prepare(
    `UPDATE dev_applications SET status = 'approved', review_note = ?1 WHERE id = ?2`,
  )
    .bind(note, id)
    .run()
  return json({ ok: true, id, status: 'approved' })
}

/** POST /api/v1/admin/dev-applications/:id/reject —— body `{ note }`（必填） */
export async function rejectDevApplication(ctx: Ctx, params: Record<string, string>) {
  const admin = await requireAdmin(ctx)
  if (admin instanceof Response) return admin

  const id = Number(params.id)
  if (!Number.isInteger(id)) return fail(400, 'bad_request', 'id 不合法')
  const body = await readJson(ctx.req)
  const note = typeof body.note === 'string' ? body.note.trim() : ''
  // 驳回**必须**有理由：申请人要知道改什么。只给「已驳回」等于让他瞎猜。
  if (!note) return fail(400, 'bad_request', '驳回必须填写理由（申请人需要知道改什么）')

  const r = await ctx.env.DB.prepare(
    `UPDATE dev_applications SET status = 'rejected', review_note = ?1 WHERE id = ?2 AND status = 'pending'`,
  )
    .bind(note, id)
    .run()
  if (!r.meta.changes) return fail(409, 'already_reviewed', '申请不存在或已审核')
  return json({ ok: true, id, status: 'rejected' })
}

/** GET /api/v1/admin/submissions —— 扩展提交队列（含包体是否还在） */
export async function listSubmissions(ctx: Ctx) {
  const admin = await requireAdmin(ctx)
  if (admin instanceof Response) return admin
  const rows = await ctx.env.DB.prepare(
    `SELECT s.id, s.ext_id AS extId, s.version, s.status, s.review_note AS reviewNote,
            s.pkg_sha256 AS pkgSha256, s.pkg_size AS pkgSize, s.created_at AS createdAt,
            u.username
       FROM submissions s JOIN users u ON u.id = s.user_id
      ORDER BY s.created_at ASC`,
  )
    .all<Record<string, unknown>>()
  // ⚠️ 顺带报「包体在不在」。清单里有一列 `hasPackage`：
  //   包丢了的话这条永远发不出去（2026-10-01 才补上的存储），
  //   而审核页若不提示，审核人会通过一条**根本发不出去**的提交。
  const items = await Promise.all(
    (rows.results ?? []).map(async (r) => ({
      ...r,
      hasPackage: !!(r.pkgSha256 && r.pkgSize) && (await getBlob(ctx.env.DB, String(r.pkgSha256), Number(r.pkgSize))) !== null,
    })),
  )
  return json({ items })
}

/**
 * POST /api/v1/admin/submissions/:id/approve
 *
 * **这里只把状态改成 `approved`，不直接上架。** 上架必须走
 * `npm run publish:approved`（读 D1 → 重签清单 → 部署 Pages）——
 * 因为市场清单是**签过名的静态文件**，运行期改它会让验签失败。
 * 详见 lib/paths.ts 与 scripts/publish-approved.mjs。
 */
export async function approveSubmission(ctx: Ctx, params: Record<string, string>) {
  const admin = await requireAdmin(ctx)
  if (admin instanceof Response) return admin

  const id = Number(params.id)
  if (!Number.isInteger(id)) return fail(400, 'bad_request', 'id 不合法')
  const row = await ctx.env.DB.prepare(
    `SELECT id, ext_id AS extId, version, status, pkg_sha256 AS pkgSha256, pkg_size AS pkgSize
       FROM submissions WHERE id = ?1`,
  )
    .bind(id)
    .first<{ id: number; extId: string; version: string; status: string; pkgSha256: string | null; pkgSize: number | null }>()
  if (!row) return fail(404, 'not_found', '提交不存在')
  if (row.status !== 'pending_review') {
    return fail(409, 'already_reviewed', `该提交已是 ${row.status} 状态`)
  }

  // ---- 审核时**重跑**关卡（不信任提交时的结果） ----
  // 三条理由：包在库里可能被换；关卡规则以后会变；
  // 提交时那次是作者进程里跑的，审核是另一个信任边界。
  if (!row.pkgSha256 || !row.pkgSize) {
    return fail(409, 'package_missing', '该提交没有包体（旧版本提交，那时还没存包）')
  }
  const bytes = await getBlob(ctx.env.DB, row.pkgSha256, row.pkgSize)
  if (!bytes) return fail(409, 'package_missing', '包体在库里找不到，无法审核')

  const entries = listZipEntries(bytes)
  const gate = await runGate(
    entries,
    (p) => readZipEntry(bytes, entries, p, MAX_MANIFEST_BYTES),
    row.extId,
  )
  if (!gate.ok) {
    await ctx.env.DB.prepare(
      `UPDATE submissions SET status = 'gate_failed', review_note = ?1 WHERE id = ?2`,
    )
      .bind(gate.problems.slice(0, 5).join('；'), id)
      .run()
    return fail(422, 'gate_failed', gate.problems[0] ?? '未通过关卡', { problems: gate.problems })
  }

  const body = await readJson(ctx.req)
  const note = typeof body.note === 'string' ? body.note.slice(0, 500) : ''
  await ctx.env.DB.prepare(
    `UPDATE submissions SET status = 'approved', review_note = ?1, updated_at = ?2 WHERE id = ?3`,
  )
    .bind(note, Date.now(), id)
    .run()
  return json({
    ok: true,
    id,
    status: 'approved',
    // 明确告诉审核人**下一步**：状态改了不等于已上架
    next: '已通过审核。仍需运行 `npm run publish:approved` 才会进市场清单并部署。',
  })
}

/** POST /api/v1/admin/submissions/:id/reject —— body `{ note }`（必填） */
export async function rejectSubmission(ctx: Ctx, params: Record<string, string>) {
  const admin = await requireAdmin(ctx)
  if (admin instanceof Response) return admin

  const id = Number(params.id)
  if (!Number.isInteger(id)) return fail(400, 'bad_request', 'id 不合法')
  const body = await readJson(ctx.req)
  const note = typeof body.note === 'string' ? body.note.trim() : ''
  if (!note) return fail(400, 'bad_request', '驳回必须填写理由（作者需要知道改什么）')

  const r = await ctx.env.DB.prepare(
    `UPDATE submissions SET status = 'rejected', review_note = ?1, updated_at = ?2
      WHERE id = ?3 AND status IN ('pending_review','gate_failed')`,
  )
    .bind(note, Date.now(), id)
    .run()
  if (!r.meta.changes) return fail(409, 'already_reviewed', '提交不存在或已审核')
  return json({ ok: true, id, status: 'rejected' })
}