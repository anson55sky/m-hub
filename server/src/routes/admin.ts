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
import { rebuildRegistry } from '../lib/marketSign.ts'
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

  // 顺手把包内 manifest 存下来：清单里的 name/description/permissions 来自它，
  // 而包是分块存在 pkg_blobs 里的、读取要解 zip。审核这一步手里已经有那个包，
  // 此刻存下来，「上架」就只是纯数据库读 —— 不必每次上架都解一遍 zip。
  // 存不下（manifest 读不出）**不拦审核**：关卡已经过了，manifest 缺失是
  // 上架时会再报一次的另一件事，不该在这里让审核人白忙。
  let manifestJson: string | null = null
  try {
    const mf = await readZipEntry(bytes, entries, 'manifest.json', MAX_MANIFEST_BYTES)
    if (mf) {
      const text = new TextDecoder().decode(mf)
      JSON.parse(text) // 先确认能解析，别把半截 JSON 存进去
      manifestJson = text
    }
  } catch {
    /* 交给上架那一步报错 */
  }

  const body = await readJson(ctx.req)
  const note = typeof body.note === 'string' ? body.note.slice(0, 500) : ''
  await ctx.env.DB.prepare(
    `UPDATE submissions
        SET status = 'approved', review_note = ?1, updated_at = ?2,
            manifest_json = COALESCE(?3, manifest_json)
      WHERE id = ?4`,
  )
    .bind(note, Date.now(), manifestJson, id)
    .run()
  return json({
    ok: true,
    id,
    status: 'approved',
    // 明确告诉审核人**下一步**：状态改了不等于已上架
    next: '已通过审核。再点「上架」才会进市场清单（不需要再跑任何命令）。',
  })
}

/**
 * POST /api/v1/admin/submissions/:id/publish —— 把已审核通过的提交**上架**。
 *
 * ## 为什么要单独一个动作（2026-10-02）
 *
 * 原来「审核通过」与「上架」之间隔着一道**没有按钮**的步骤：必须有人在那台
 * 持有市场私钥的机器上跑 `npm run publish:approved`。用户报的是「重新发布
 * 新版本后市场还是旧版，而且没有上架按钮」—— 不是他不会用，是这条路径
 * 根本没有按钮可按。
 *
 * 私钥上传为 Pages 加密 secret 后，「上架」可以在运行期完成，于是这里
 * 就是那个按钮背后的动作：状态置 `published` + 重建并重签市场清单。
 *
 * ## 为什么不并进 approve
 *
 * 审核（人看内容）与上架（对所有用户生效）是两件不同的事，分开才谈得上
 * 「先看再放」。并成一个动作的话，审核台的「通过」就等于「立刻上线」，
 * 想再看一眼就没有回头路了。
 *
 * ## 失败处理：上架失败**不回滚**审核结果
 *
 * 签名失败（私钥没配）或 D1 写失败时，提交**已经**是 approved —— 那是审核的
 * 结论，与上架无关，不该被上架的失败连累。所以：状态照改，把错误如实返回，
 * 让管理员能重试。反过来（状态没改但假装成功）才是谎报。
 */
export async function publishSubmission(ctx: Ctx, params: Record<string, string>) {
  const admin = await requireAdmin(ctx)
  if (admin instanceof Response) return admin

  const id = Number(params.id)
  if (!Number.isInteger(id)) return fail(400, 'bad_request', 'id 不合法')

  const row = await ctx.env.DB.prepare(
    `SELECT status, manifest_json AS manifestJson FROM submissions WHERE id = ?1`,
  )
    .bind(id)
    .first<{ status: string; manifestJson: string | null }>()
  if (!row) return fail(404, 'not_found', '提交不存在')
  if (row.status !== 'approved') {
    return fail(409, 'not_approved', `只有 approved 能上架，这条是 ${row.status}`)
  }
  // ⚠️ 没有 manifest 就上架不了：清单的 name/description/permissions 全来自它。
  //   放一个空壳条目进市场比不发布更糟 —— 用户看到一个只有 id 和下载链接的条目。
  if (!row.manifestJson) {
    return fail(
      409,
      'manifest_missing',
      '这条提交没有存下包内 manifest，无法上架。请让作者用修正过的包重新提交。',
    )
  }

  await ctx.env.DB.prepare(
    `UPDATE submissions SET status = 'published', updated_at = ?1 WHERE id = ?2 AND status = 'approved'`,
  )
    .bind(Date.now(), id)
    .run()

  // 清单重建失败时如实报错：提交已经是 published（审核结论成立），
  // 但市场还是旧的 —— 管理员需要知道，且能重试。
  let reg: { extensions: number }
  try {
    reg = await rebuildRegistry(ctx)
  } catch (e) {
    return json(
      {
        ok: false,
        id,
        status: 'published',
        error: 'registry_rebuild_failed',
        message: `已标记上架，但市场清单没更新：${String((e as Error)?.message ?? e)}`,
      },
      502,
    )
  }
  return json({
    ok: true,
    id,
    status: 'published',
    next: `已上架，市场清单现有 ${reg.extensions} 个扩展。`,
  })
}

/** POST /api/v1/admin/market/rebuild —— 只重建清单，不改任何状态。 */
export async function rebuildMarket(ctx: Ctx) {
  const admin = await requireAdmin(ctx)
  if (admin instanceof Response) return admin
  try {
    const reg = await rebuildRegistry(ctx)
    return json({ ok: true, ...reg })
  } catch (e) {
    return fail(502, 'registry_rebuild_failed', String((e as Error)?.message ?? e))
  }
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

/**
 * DELETE /api/v1/admin/submissions/:id —— 删除一条**已终结**的提交。
 *
 * ## 为什么需要它（2026-10-02 用户反馈）
 *
 * 提交一旦走完流程就永远留在作者侧的列表里：撤回只对未终结状态开放，
 * 驳回会把状态改成 `rejected` 但**不删行**。于是连续发 5 个版本后，
 * 作者的「提交记录」里堆着 5 条 approved，界面上还把它们算成
 * 「5 条未走完流程、占满待处理额度」—— 与顶栏「待处理还可 5 条」直接矛盾。
 *
 * 所以「删除旧版」这个诉求不能靠撤回（撤回不了已 approved 的），
 * 也不该靠驳回（那是审核动作，会给作者留一条「你被拒绝过」的记录）。
 * 需要一个独立的**管理员删除**，语义干净：这条提交不存在了。
 *
 * ## 三条安全边界（都必须成立，否则就是数据损坏漏洞）
 *
 * ① **只能删已终结的**：`published` / `rejected` / `withdrawn`。
 *    未终结的三态（`uploaded` / `pending_review` / `gate_failed`）删掉等于
 *    让作者凭空少一条待办 —— 而且 `idx_sub_one_open` 那个部分唯一索引
 *    会因此放行，于是**绕过**「同一扩展同时只允许一条未走完流程」的产品口径。
 *    `WHERE status IN (…)` 让这个约束落在 SQL 里，不靠应用层 if（约定 70 的判据）。
 *
 * ② **包体 blob 一并清掉，但只清没人引用的**：`pkg_blobs` 是**内容寻址**的
 *    （约定：同一份字节只存一份），所以别的提交很可能指向同一个 sha256。
 *    直接 `DELETE WHERE sha256 = ?` 会把别人的包体也删掉 → 那些提交审核通过后
 *    发布不出来（`package_missing`）。故必须先确认没有别的行还在引用它。
 *
 * ③ **已在市场清单里的版本删不得**：那会让清单指向一个不存在的字节。
 *    `published` 的删除靠「先下架（写 revoked + 重签清单）再删」两步走，
 *    不在本接口的职责内 —— 所以本接口**不含** `published`，
 *    下架走 `/api/v1/admin/market/:id/revoke`。
 */
export async function deleteSubmission(ctx: Ctx, params: Record<string, string>) {
  const admin = await requireAdmin(ctx)
  if (admin instanceof Response) return admin

  const id = Number(params.id)
  if (!Number.isInteger(id)) return fail(400, 'bad_request', 'id 不合法')

  // 先读出来：既要拿 sha256 做引用检查，也要把 status 写进错误信息
  const row = await ctx.env.DB.prepare(
    `SELECT status, pkg_sha256 AS pkgSha256 FROM submissions WHERE id = ?1`,
  )
    .bind(id)
    .first<{ status: string; pkgSha256: string | null }>()
  if (!row) return fail(404, 'not_found', '提交不存在')

  if (row.status === 'published') {
    return fail(
      409,
      'still_published',
      '这条已上架，删不掉。先在市场把它下架（会写 revoked 并重签清单），再删这条记录。',
    )
  }
  if (!(DELETABLE_STATUSES as readonly string[]).includes(row.status)) {
    return fail(
      409,
      'not_deletable',
      `只能删已走完流程的提交（${DELETABLE_STATUSES.join(' / ')}），` +
        `这条是 ${row.status}。走完流程前请走撤回。`,
    )
  }

  // ⚠️ 状态白名单**必须出现在 DELETE 自己的 WHERE 里**，不能只靠上面那段 JS 判断。
  //   两次请求之间状态可能被别人改掉（作者撤回 / 另一个管理员操作），
  //   只在 JS 里判就会删掉一条此刻正处于未终结状态的提交 —— 而那等于给该扩展
  //   解开了 `idx_sub_one_open` 的部分唯一索引，绕过「同一扩展同时只允许
  //   一个待审版本」的产品口径。故 SQL 复述一遍白名单：即便状态在这中间
  //   变了，这一行也删不掉它（changes = 0 → 走下面的 409）。
  const placeholders = DELETABLE_STATUSES.map((_, i) => `?${i + 2}`).join(', ')
  const del = await ctx.env.DB.prepare(
    `DELETE FROM submissions WHERE id = ?1 AND status IN (${placeholders})`,
  )
    .bind(id, ...DELETABLE_STATUSES)
    .run()
  if (!del.meta.changes) {
    return fail(409, 'not_deletable', '提交不存在，或它的状态已变（请刷新后重试）')
  }

  // ② 内容寻址：只有没有任何提交再引用它时才清 blob
  let blobFreed = false
  if (row.pkgSha256) {
    const stillUsed = await ctx.env.DB.prepare(
      `SELECT COUNT(*) AS n FROM submissions WHERE pkg_sha256 = ?1`,
    )
      .bind(row.pkgSha256)
      .first<{ n: number }>()
    if ((stillUsed?.n ?? 0) === 0) {
      await ctx.env.DB.prepare(`DELETE FROM pkg_blobs WHERE sha256 = ?1`)
        .bind(row.pkgSha256)
        .run()
      blobFreed = true
    }
  }

  return json({ ok: true, id, deleted: row.status, blobFreed })
}

/**
 * GET /api/v1/admin/health —— **只读**探活端点，用于确认新部署是否已生效。
 *
 * ## 为什么需要它
 *
 * `scripts/publish-approved.mjs` 部署完要立刻标记提交为 published，
 * 但 `wrangler pages deploy` 报 "Deployment complete" 只代表**上传完成**，
 * 边缘节点还在跑旧 worker。第一版拿 `POST .../mark-published` 当探针，
 * 结果拿到 405；改对之后又发现**更糟**：那次探针在旧代码上「失败」是对的，
 * 可一旦新旧顺序反过来（新代码先到），探针就**真的把某条提交标成 published** ——
 * 一次纯探活动作改了业务数据。
 *
 * 探活端点必须是**无副作用**的。这条端点只回一个常量字符串，
 * 旧版本里根本没有这条路由（404），新版本回 200 —— 判据干净且零副作用。
 */
export async function adminHealth(ctx: Ctx): Promise<Response> {
  // 也要过门禁：它在 /admin 前缀下，不该成为一个不需要鉴权的公开端点
  const admin = await requireAdmin(ctx)
  if (admin instanceof Response) return admin
  return json({ ok: true, marker: 'admin-delete-v1' })
}

/** 允许管理员删除的状态。⚠️ **不含** `published`（见上面③） */
const DELETABLE_STATUSES = ['approved', 'rejected', 'withdrawn'] as const

/**
 * POST /api/v1/admin/submissions/:id/mark-published —— 标记为「已上架」。
 *
 * ## 为什么需要它
 *
 * 状态机本来缺这一环：`approved` →（构建期发布）→ 清单里有了这个版本，
 * 但**提交行永远停在 `approved``**。后果有两个方向：
 *
 *   · 作者侧那条记录永远显示「已通过」，约定 60 的「已下架是客户端派生状态，
 *     判据是 `status === 'published'` 命中清单 `revoked`」整段不触发；
 *   · `deleteSubmission` 按「`published` 不许删」保护在架版本 ——
 *     而状态到不了 published，那条保护就是**死代码**，在架版本可以被直接删掉。
 *
 * 只由 `scripts/publish-approved.mjs` 在**部署成功后**调用。
 *
 * ## 为什么只允许 approved → published（单向）
 *
 * 下架不回写 `submissions.status`（约定 60 的口径：那会破坏
 * 「已下架是客户端派生状态」这条设计）。所以 published 是**单向终点**，
 * 不接受从 published 改回任何状态 —— 否则就等于在本接口里偷偷实现下架，
 * 而下架必须走「写 revoked + 重签清单」两步。
 */
export async function markSubmissionPublished(ctx: Ctx, params: Record<string, string>) {
  const admin = await requireAdmin(ctx)
  if (admin instanceof Response) return admin

  const id = Number(params.id)
  if (!Number.isInteger(id)) return fail(400, 'bad_request', 'id 不合法')

  const r = await ctx.env.DB.prepare(
    `UPDATE submissions SET status = 'published', updated_at = ?1
      WHERE id = ?2 AND status = 'approved'`,
  )
    .bind(Date.now(), id)
    .run()
  if (!r.meta.changes) {
    // 已经是 published 时**不算错** —— 重跑发布脚本是正常操作，幂等。
    const row = await ctx.env.DB.prepare(`SELECT status FROM submissions WHERE id = ?1`)
      .bind(id)
      .first<{ status: string }>()
    if (!row) return fail(404, 'not_found', '提交不存在')
    if (row.status === 'published') return json({ ok: true, id, status: 'published' })
    return fail(409, 'not_approved', `只有 approved 能标记为已上架，这条是 ${row.status}`)
  }
  return json({ ok: true, id, status: 'published' })
}