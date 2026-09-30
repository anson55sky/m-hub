// 开发者申请 + 扩展发布提交。
//
// 两条产品口径必须与客户端一致，否则界面会说反话：
// · 约定 58：同一扩展只允许一条未走完流程的提交，**拦在打包之前**且写明原因。
//   服务端这里用**部分唯一索引**兜底（schema.sql::idx_sub_one_open），
//   不靠「先查一遍再插」——并发下会漏，而客户端已经按「有阻塞就拦住按钮」处理。
// · 约定 60：下架只写市场清单 `revoked`，**绝不 UPDATE submissions.status**。
//   否则提交记录会永远停在 published，客户端从记录里看不出「已下架」。
// · 约定 61：提交记录按**账号**全量列出，服务端**不支持**按 ext_id 过滤。
//   客户端因此不做过滤，而是给非本扩展的行加「其他扩展」标签。

import {
  currentUser,
  fail,
  json,
  readJson,
  unauthorized,
  type Ctx,
  type User,
} from '../lib/http'

/** 未走完流程的提交状态。必须与客户端可撤回白名单一致。 */
const OPEN_STATUSES = ['uploaded', 'pending_review', 'gate_failed'] as const

async function requireDeveloper(ctx: Ctx): Promise<User | Response> {
  const user = await currentUser(ctx.env, ctx.token)
  if (!user) return unauthorized()
  if (!user.is_developer) {
    return fail(403, 'not_developer', '需先通过开发者认证')
  }
  return user
}

// ------------------------------------------------------------ 开发者申请

/** POST /api/v1/dev/apply —— body `{ reason }` */
export async function apply(ctx: Ctx) {
  const user = await currentUser(ctx.env, ctx.token)
  if (!user) return unauthorized()
  const body = await readJson(ctx.req)

  const reason = typeof body.reason === 'string' ? body.reason.trim() : ''
  if (!reason) return fail(400, 'bad_request', '请填写申请理由')
  if (reason.length > 2000) return fail(400, 'bad_request', '申请理由过长')

  // 兑换邀请码是申请的前置（约定 53：兑换后才发额度、才可申请开发者）
  if (!user.invite_redeemed) {
    return fail(403, 'invite_required', '需先兑换邀请码')
  }
  if (user.developer_status === 'approved') {
    return fail(409, 'already_developer', '你已是开发者')
  }

  try {
    await ctx.env.DB.prepare(
      `INSERT INTO dev_applications (user_id, reason) VALUES (?1, ?2)`,
    )
      .bind(user.id, reason)
      .run()
  } catch (e) {
    // 撞上部分唯一索引 = 已有一条 pending。这是**正常状态**不是 500：
    // 重复申请不该看起来像服务器故障。
    if (String(e).includes('UNIQUE')) {
      return fail(409, 'already_pending', '你已有一份申请在审核中')
    }
    throw e
  }
  await ctx.env.DB.prepare(
    `UPDATE users SET developer_status = 'pending' WHERE id = ?1`,
  )
    .bind(user.id)
    .run()

  return json({ ok: true, status: 'pending' })
}

/** GET /api/v1/dev/apply */
export async function applyStatus(ctx: Ctx) {
  const user = await currentUser(ctx.env, ctx.token)
  if (!user) return unauthorized()
  const row = await ctx.env.DB.prepare(
    `SELECT id, status, reason, review_note AS reviewNote, created_at AS createdAt
       FROM dev_applications WHERE user_id = ?1 ORDER BY created_at DESC LIMIT 1`,
  )
    .bind(user.id)
    .first<Record<string, unknown>>()

  return json({
    status: user.developer_status,
    is_developer: !!user.is_developer,
    can_apply_developer: user.invite_redeemed && user.developer_status === 'none',
    application: row ?? null,
  })
}

// ------------------------------------------------------------ 扩展发布

/**
 * POST /api/v1/dev/submissions —— multipart 上传（约定 51）。
 *
 * ⚠️ **同名字段必须写成 `key[]`**：Hono 的 `parseBody()` 默认只保留同名键的
 * **最后一个**值，不带 `[]` 时传 6 张截图只到 1 张、且**服务端不报错**。
 * 客户端契约测试 `publisher.rs::upload_sends_screenshots_with_array_field_name`
 * 锁的就是这条。本实现是手写解析，行为等价：同名多值按数组收。
 */
export async function submit(ctx: Ctx) {
  const user = await requireDeveloper(ctx)
  if (user instanceof Response) return user

  const form = await ctx.req.formData().catch(() => null)
  if (!form) return fail(400, 'bad_request', '需要 multipart/form-data')

  const extId = String(form.get('ext_id') ?? '').trim()
  const version = String(form.get('version') ?? '').trim()
  if (!extId || !version) return fail(400, 'bad_request', '缺少 ext_id 或 version')
  if (!/^[a-z0-9][a-z0-9.-]*$/i.test(extId)) {
    return fail(400, 'bad_request', '扩展 id 格式不合法')
  }
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    // 约定 63：版本严格限三段纯数字，prerelease/前导零一律拒
    return fail(400, 'bad_request', '版本号必须是 x.y.z 三段纯数字')
  }

  // workers-types 对 FormData.get 的 File 返回值建模不全（声明成 string），
  // 故显式取 unknown 再判 File，不用 as 断言把类型骗过去
  const pkgRaw = form.get('package') as unknown
  if (!(pkgRaw instanceof File)) return fail(400, 'bad_request', '缺少扩展包文件')
  const pkg: File = pkgRaw

  // 阻塞判定：同扩展已有未走完流程的提交 → 明确报出是哪一条、什么状态
  const open = await ctx.env.DB.prepare(
    `SELECT id, version, status FROM submissions
      WHERE user_id = ?1 AND ext_id = ?2 AND status IN ('uploaded','pending_review','gate_failed')
      ORDER BY created_at DESC LIMIT 1`,
  )
    .bind(user.id, extId)
    .first<{ id: number; version: string; status: string }>()
  if (open) {
    return fail(409, 'blocked_by_open_submission', `${open.id}`, {
      message: `该扩展已有未走完流程的提交：v${open.version}（${open.status}）。请先撤回它，或等结果。`,
      blocking: { id: open.id, version: open.version, status: open.status },
    })
  }

  // 版本必须严格大于已发布版本（约定 63：服务端是权威兜底）
  const published = await ctx.env.DB.prepare(
    `SELECT version FROM submissions
      WHERE user_id = ?1 AND ext_id = ?2 AND status = 'published'
      ORDER BY created_at DESC LIMIT 1`,
  )
    .bind(user.id, extId)
    .first<{ version: string }>()
  if (published && compareSemver(version, published.version) <= 0) {
    return fail(
      400,
      'version_not_incremented',
      `新版本必须严格大于已发布版本 v${published.version}`,
    )
  }

  const bytes = new Uint8Array(await pkg.arrayBuffer())
  // D1 单值上限 1MB，故包**不进 D1**；这里只记元数据与存放位置。
  // 真实落地由 `npm run publish:package` 推到 GitHub Releases（见 README）。
  if (bytes.byteLength > 200 * 1024 * 1024) {
    return fail(400, 'package_too_large', '扩展包超过 200MB')
  }
  const sha256 = await sha256Hex(bytes)

  const ins = await ctx.env.DB.prepare(
    `INSERT INTO submissions (user_id, ext_id, version, status)
     VALUES (?1, ?2, ?3, 'pending_review')`,
  )
    .bind(user.id, extId, version)
    .run()
  const subId = Number(ins.meta.last_row_id)

  await ctx.env.DB.prepare(
    `INSERT INTO submission_assets (submission_id, filename, size, sha256, storage_key)
     VALUES (?1, ?2, ?3, ?4, ?5)`,
  )
    .bind(subId, pkg.name || `${extId}-${version}.xhpack`, bytes.byteLength, sha256,
         `pending/${user.id}/${extId}/${version}`)
    .run()

  // 截图：同名多值必须收成数组（见上方 ⚠️）
  // ⚠️ 必须收成**数组**：同名多值只取最后一个是 FormData 的经典坑（约定 51）。
  // workers-types 的 `FormData.getAll` 只声明了返回 `string[]`（File 建模不全），
  // 故这里显式取 unknown 再筛，避免断言刷类型。
  const shots = (form.getAll('screenshots[]') as unknown[]).filter(
    (v): v is File => v instanceof File,
  )
  if (shots.length > 5) return fail(400, 'bad_request', '截图最多 5 张')
  for (const s of shots) {
    if (s.size > 2 * 1024 * 1024) return fail(400, 'screenshot_too_large', '单张截图超过 2MB')
  }

  return json({ ok: true, id: subId, version, status: 'pending_review', screenshots: shots.length })
}

/** GET /api/v1/dev/submissions —— 按账号全量，**不支持** ext_id 过滤（约定 61） */
export async function mySubmissions(ctx: Ctx) {
  const user = await requireDeveloper(ctx)
  if (user instanceof Response) return user

  const url = ctx.url
  const page = Math.max(1, Number(url.searchParams.get('page') ?? '1') || 1)
  const pageSize = Math.min(100, Math.max(1, Number(url.searchParams.get('page_size') ?? '20') || 20))
  const offset = (page - 1) * pageSize

  const rows = await ctx.env.DB.prepare(
    `SELECT id, ext_id AS extId, version, status, review_note AS reviewNote,
            created_at AS createdAt, updated_at AS updatedAt
       FROM submissions WHERE user_id = ?1
      ORDER BY created_at DESC LIMIT ?2 OFFSET ?3`,
  )
    .bind(user.id, pageSize, offset)
    .all<Record<string, unknown>>()

  const totalRow = await ctx.env.DB.prepare(
    `SELECT COUNT(*) AS total FROM submissions WHERE user_id = ?1`,
  )
    .bind(user.id)
    .first<{ total: number }>()

  const openCount = await ctx.env.DB.prepare(
    `SELECT COUNT(*) AS n FROM submissions
      WHERE user_id = ?1 AND status IN ('uploaded','pending_review','gate_failed')`,
  )
    .bind(user.id)
    .first<{ n: number }>()

  const total = totalRow?.total ?? 0
  return json({
    items: rows.results ?? [],
    total,
    page,
    page_size: pageSize,
    // 约定 61：配额是「剩余」语义，且**只回剩余不回上限**（上限是运营参数，
    // 不进开源仓）。故客户端界面必须把话说成「还可 N 条」而不能自解释成「共 N 条」。
    quota: {
      drafts_remaining: Math.max(0, 5 - (openCount?.n ?? 0)),
      daily_submits_remaining: 3,
      published_remaining: Math.max(0, 10 - await countPublished(ctx, user.id)),
    },
  })
}

async function countPublished(ctx: Ctx, userId: number): Promise<number> {
  // 约定 60 提醒：`published_remaining` 按 COUNT(DISTINCT ext_id) 算，
  // **被下架的扩展仍占名额**（服务端口径如此），界面文案别把它说成「当前在架数量」。
  const r = await ctx.env.DB.prepare(
    `SELECT COUNT(DISTINCT ext_id) AS n FROM submissions WHERE user_id = ?1 AND status = 'published'`,
  )
    .bind(userId)
    .first<{ n: number }>()
  return r?.n ?? 0
}

/** GET /api/v1/dev/submissions/:id */
export async function submissionDetail(ctx: Ctx, params: Record<string, string>) {
  const user = await requireDeveloper(ctx)
  if (user instanceof Response) return user
  const id = Number(params.id)
  if (!Number.isInteger(id)) return fail(400, 'bad_request', 'id 非法')

  // ⚠️ 必须带 user_id 条件：只按 id 查等于把别人的提交记录公开了
  const row = await ctx.env.DB.prepare(
    `SELECT id, ext_id AS extId, version, status, review_note AS reviewNote,
            created_at AS createdAt, updated_at AS updatedAt
       FROM submissions WHERE id = ?1 AND user_id = ?2`,
  )
    .bind(id, user.id)
    .first<Record<string, unknown>>()
  if (!row) return fail(404, 'not_found', '找不到该提交记录')
  return json(row)
}

/** POST /api/v1/dev/submissions/:id/withdraw */
export async function withdraw(ctx: Ctx, params: Record<string, string>) {
  const user = await requireDeveloper(ctx)
  if (user instanceof Response) return user
  const id = Number(params.id)
  if (!Number.isInteger(id)) return fail(400, 'bad_request', 'id 非法')

  const cur = await ctx.env.DB.prepare(
    `SELECT status, version FROM submissions WHERE id = ?1 AND user_id = ?2`,
  )
    .bind(id, user.id)
    .first<{ status: string; version: string }>()
  if (!cur) return fail(404, 'not_found', '找不到该提交记录')

  // 白名单必须与「阻塞集合」一致（约定 58：能撤回的才拦，拦住也一定能自己解开）
  if (!(OPEN_STATUSES as readonly string[]).includes(cur.status)) {
    return fail(
      409,
      'not_withdrawable',
      `状态 ${cur.status} 不可撤回（可撤回：${OPEN_STATUSES.join(' / ')}）`,
    )
  }

  await ctx.env.DB.prepare(
    `UPDATE submissions SET status = 'withdrawn', updated_at = ?2 WHERE id = ?1`,
  )
    .bind(id, Date.now())
    .run()
  return json({ ok: true })
}

// ------------------------------------------------------------ 小工具

/** 三段式数字版本比较。**不**用 semver 库：约定 63 明确拒 prerelease/前导零，
 *  所以只需比三段整数，且这与客户端 `validateNewVersion` 同口径。 */
function compareSemver(a: string, b: string): number {
  const pa = a.split('.').map(Number)
  const pb = b.split('.').map(Number)
  for (let i = 0; i < 3; i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0)
    if (d !== 0) return d < 0 ? -1 : 1
  }
  return 0
}

async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const d = await crypto.subtle.digest('SHA-256', bytes)
  return [...new Uint8Array(d)].map((b) => b.toString(16).padStart(2, '0')).join('')
}
