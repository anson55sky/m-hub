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
} from '../lib/http.ts'
// 「能不能申请开发者」的唯一判定（与 me.ts 共用同一份，见 lib/developerGate.ts 注释）
import { canApplyDeveloper } from '../lib/developerGate.ts'
// 2026-10-01：包体原来**只算 sha256 就丢掉**（注释说推到 GitHub Releases，
// 那个脚本从来不存在）→ 审核通过了也发布不出去。详见 submit() 里那段注释。
import { putBlob } from '../lib/pkgStore.ts'
import { parseShotIds } from '../lib/shotIds.ts'
import { listZipEntries } from '../lib/zipdir.ts'
import { readZipEntry } from '../lib/zipread.ts'
import { runGate } from '../lib/gate.ts'

/**
 * 单包上限。
 *
 * 原来写的是 200MB，而 D1 分块方案的实际约束是**整库 10GB** 与每次提交的
 * 时长。留 64MB：足够装下任何合理扩展（含几张截图），
 * 又不会让一次上传跑太久触发 Functions 的 CPU 限制。
 */
const MAX_PACKAGE_BYTES = 64 * 1024 * 1024

/**
 * manifest.json 解压后的大小上限（防 zip bomb）。
 *
 * 与 `lib/gate.ts::MAX_MANIFEST_BYTES` 数值相同但**用途不同**：
 * 那边是「manifest 不该这么大」（规则），这边是「解压时最多吐这么多」（资源上限）。
 * 两者恰好取同一个数，**改一个不会自动改另一个** —— 故都在注释里点明。
 */
const MAX_MANIFEST_BYTES = 256 * 1024

// ---- 截图：按**文件头**判类型（不信扩展名、不信浏览器给的 Content-Type）----
//
// ⚠️ 这道校验以前没有，因为字节压根没被存下来 —— 也就没人担心它们被当图片发出去。
//   现在要存要发了，就得补上：Content-Type 由上传方声明，`multipart` 的
//   `File.type` 说它是 `image/png` 时字节完全可能是 HTML/JS。
//   一张 `text/html` 被原样回给浏览器，就是一个存储型 XSS。
const SHOT_TYPES: { ext: string; mime: string; test: (b: Uint8Array) => boolean }[] = [
  { ext: 'png', mime: 'image/png', test: (b) => b.length > 8 && b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47 },
  { ext: 'jpg', mime: 'image/jpeg', test: (b) => b.length > 3 && b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff },
  { ext: 'webp', mime: 'image/webp', test: (b) => b.length > 12 && b[0] === 0x52 && b[1] === 0x49 && b[2] === 0x46 && b[3] === 0x46 && b[8] === 0x57 && b[9] === 0x45 && b[10] === 0x42 && b[11] === 0x50 },
]

/** 按文件头判截图类型；认不出返回 null（**不猜**：猜错就是把任意字节当图片发出去） */
export function sniffShot(bytes: Uint8Array): { ext: string; mime: string } | null {
  for (const t of SHOT_TYPES) if (t.test(bytes)) return { ext: t.ext, mime: t.mime }
  return null
}

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
    // 复用共享判定（见该函数的注释：这里曾与 me.ts 各算一份）
    can_apply_developer: canApplyDeveloper(user),
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
  /** 作者署名（2026-10-04）：显示在市场卡片与详情页。空 = 不署名 */
  const author = String(form.get('author') ?? '')
    .trim()
    // 上限 64 字：它在市场卡片上是一行，更长必然被截断，而截断的署名没人认得出
    .slice(0, 64)
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
  if (bytes.byteLength === 0) return fail(400, 'bad_request', '扩展包是空的')
  if (bytes.byteLength > MAX_PACKAGE_BYTES) {
    return fail(400, 'package_too_large', `扩展包超过 ${MAX_PACKAGE_BYTES / 1048576}MB`)
  }

  // ⚠️ 这里原来只算 sha256 就把字节丢掉，注释说「包不进 D1，真实落地由
  // `npm run publish:package` 推到 GitHub Releases」——**那个脚本从来不存在**
  // （package.json 里没有这一条）。于是包在提交那一刻就没了：
  // **审核通过了也发布不出去**，因为清单里的 downloadUrl 指向不存在的字节。
  //
  // 这是比「缺审核接口」更根本的缺口：没有存储就没有上架。
  // 现在真的存进 D1（分块 + 内容寻址，见 lib/pkgStore.ts）。
  const sha256 = await putBlob(ctx.env.DB, bytes)

  // 包内文件清单：关卡判断「有没有禁的扩展名 / 是否漏了 manifest.json」、
  // 审核页展示「这个包会往用户机器上放什么」，都要它。
  //
  // 解析失败**不**阻断提交（那是关卡该判的事），只是关卡会拿不到清单。
  // ⚠️ `listZipEntries` 是**同步**的（只读中央目录，不解压），
  //    所以不能用 `.catch()` —— 得显式 try。
  let pkgFiles: ReturnType<typeof listZipEntries> | null = null
  try {
    pkgFiles = listZipEntries(bytes)
  } catch (e) {
    // 记进 review_note 是**错的**（那字段给审核人看，且会被后续审核覆盖）；
    // 这里只留日志，包本身照收。
    console.warn('[m-hub] 包目录解析失败，关卡将无法判定条目:', String(e))
  }

  // ---- 关卡：提交时就跑，不通过**不入库** ----
  //
  // 为什么在提交时而不是审核时跑：
  // · 提交者**立刻**拿到可照着改的具体原因（约定 51 的前端会显示 review_note）
  // · 审核人看到的队列里全是「已过关卡」的包，不会出现「一眼就有的问题」
  //
  // 但**审核时仍要再跑一次**（admin 端会做）—— 包在库里可能被改，
  // 且关卡规则以后会变；提交时过的，不代表今天还过。
  const gate = await runGate(
    pkgFiles ?? [],
    (p) => readZipEntry(bytes, pkgFiles ?? [], p, MAX_MANIFEST_BYTES),
    extId,
  )
  const status = gate.ok ? 'pending_review' : 'gate_failed'
  const note = gate.ok ? null : gate.problems.slice(0, 5).join('；')

  const ins = await ctx.env.DB.prepare(
    `INSERT INTO submissions (user_id, ext_id, version, status, review_note, author, pkg_sha256, pkg_size, pkg_files)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)`,
  )
    .bind(
      user.id,
      extId,
      version,
      status,
      note,
      author,
      sha256,
      bytes.byteLength,
      pkgFiles ? JSON.stringify(pkgFiles) : null,
    )
    .run()
  const subId = Number(ins.meta.last_row_id)

  await ctx.env.DB.prepare(
    `INSERT INTO submission_assets (submission_id, filename, size, sha256, storage_key)
     VALUES (?1, ?2, ?3, ?4, ?5)`,
  )
    .bind(subId, pkg.name || `${extId}-${version}.xhpack`, bytes.byteLength, sha256,
         `pending/${user.id}/${extId}/${version}`)
    .run()

  // ⚠️ `gate` 字段**不是可选的**：客户端 `publisher.rs` 读
//    `gate.passed` 决定显示「已提交，等待人工审核」还是「机器关卡未通过」。
//    缺了它 → `unwrap_or(false)` → **明明提交成功了却说「关卡未通过」**，
//    而且下面没有任何条目可显示（症状正是「未通过」+ 空白）。
//    实机踩过：2026-10-01 用户看到「机器关卡未通过，请按下面提示修改后重新发布」
//    而下方一片空白，连重试三次，最后是版本号回写残留才暴露真正原因。
//
//    这是同一类 bug 的第三次：响应少一个字段，客户端静默兜成默认值，
//    于是界面说了一句**与事实相反**的话。见 test 的「形状」断言。
const gateItems = gate.problems.map((p, i) => ({
  id: `g${i}`,
  label: p,
  ok: false,
  detail: '',
}))
  const shots = (form.getAll('screenshots[]') as unknown[]).filter(
    (v): v is File => v instanceof File,
  )
  if (shots.length > 5) return fail(400, 'bad_request', '截图最多 5 张')
  for (const s of shots) {
    if (s.size > 2 * 1024 * 1024) return fail(400, 'screenshot_too_large', '单张截图超过 2MB')
  }

  // 「引用上一版的截图」（2026-10-04）：沿用上一版已上传的那几张。
  //
  // ⚠️ 存的是**资产行 id 列表**，不是重新上传字节 —— 截图按内容寻址存在
  //   `pkg_blobs` 里，同一张图沿用时**不占新空间、不重新上传**，
  //   所以「界面没改就不必重新选图」这句话是真的（而不是让用户重传一遍）。
  //
  // ⚠️ 两头都给了就报错、不静默取其一：界面同时显示「引用了 v0.1.7」和 3 张新图，
  //   说不上是哪几张在用 —— 这时候界面在说谎，比报错难查得多。
  const inheritFrom = String(form.get('inherit_shots_from') ?? '').trim()
  if (inheritFrom && shots.length) {
    return fail(
      400,
      'shots_conflict',
      '既上传了新截图又要求引用上一版，请二选一',
    )
  }
  if (inheritFrom && !/^\d+\.\d+\.\d+$/.test(inheritFrom)) {
    return fail(400, 'bad_request', '引用来源版本号必须是 x.y.z 三段纯数字')
  }


  // ---- 截图落库 ----
  //
  // ⚠️ 这是整条截图链路**第一次**真的把字节留下来。此前只校验不存，
  //   作者选的图没有任何归宿（见 schema.sql 的 shots_json 注释）。
  // 一行一张：资产行的 id 就是清单里的公开 URL 里的那个号。
  const shotIds: number[] = []
  if (shots.length) {
    for (const [i, shot] of shots.entries()) {
      const bytes = new Uint8Array(await shot.arrayBuffer())
      // ⚠️ 先读字节再判类型：不能靠 `File.type`，也不能靠文件名后缀
      const sniffed = sniffShot(bytes)
      if (!sniffed) {
        return fail(400, 'shot_not_image', `第 ${i + 1} 张截图不是 png/jpg/webp`)
      }
      const sha = await putBlob(ctx.env.DB, bytes)
      const ext = sniffed.ext
      const ins = await ctx.env.DB.prepare(
        `INSERT INTO submission_assets
           (submission_id, asset_kind, filename, size, sha256, storage_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)`,
      )
        .bind(
          subId,
          'shot',
          `shot-${i + 1}.${ext}`,
          bytes.byteLength,
          sha,
          `shots/${sha}`,
        )
        .run()
      shotIds.push(Number(ins.meta.last_row_id))
    }
  } else if (inheritFrom) {
    // ⚠️ 只能沿用**自己名下**同一扩展的那一版，否则就变成「猜一个 sha
    //   就能看到别人未上架的提交」。ext_id + user_id 一起限定。
    const src = await ctx.env.DB.prepare(
      `SELECT id, shots_json AS shotsJson FROM submissions
       WHERE user_id = ?1 AND ext_id = ?2 AND version = ?3`,
    )
      .bind(user.id, extId, inheritFrom)
      .first<{ id: number; shotsJson: string | null }>()
    const srcIds = parseShotIds(src?.shotsJson ?? null)
    if (srcIds.length === 0) {
      // 说清是「那版没有截图」而不是含糊的失败 —— 否则用户会以为功能坏了
      return fail(
        400,
        'inherit_source_empty',
        `v${inheritFrom} 没有可引用的截图，请自己选一次`,
      )
    }
    for (const sid of srcIds) {
      // 逐行复制资产记录，storage_key 与 sha 都指同一份字节（零新增存储）。
      // ⚠️ 只复制**属于那个提交**的行（带 sub_id 限定），不能只凭 id 取 ——
      //   否则可以用别人的资产 id 挂到自己的提交上。
      const ins = await ctx.env.DB.prepare(
        `INSERT INTO submission_assets
           (submission_id, asset_kind, filename, size, sha256, storage_key)
         SELECT ?1, asset_kind, filename, size, sha256, storage_key
           FROM submission_assets WHERE id = ?2 AND submission_id = ?3`,
      )
        .bind(subId, sid, src!.id)
        .run()
      // 行没插进去（源资产已被删）→ 借来的清单里有空洞，明确报错
      if (!ins.meta.changes) {
        return fail(
          400,
          'inherit_source_missing',
          `v${inheritFrom} 的截图已不存在，请自己选一次`,
        )
      }
      shotIds.push(Number(ins.meta.last_row_id))
    }
  }

  if (shotIds.length) {
    await ctx.env.DB.prepare(`UPDATE submissions SET shots_json = ?1 WHERE id = ?2`)
      .bind(JSON.stringify(shotIds), subId)
      .run()
  }

  // 截图：同名多值必须收成数组（见上方 ⚠️）
  // ⚠️ 必须收成**数组**：同名多值只取最后一个是 FormData 的经典坑（约定 51）。
  // workers-types 的 `FormData.getAll` 只声明了返回 `string[]`（File 建模不全），
  // 故这里显式取 unknown 再筛，避免断言刷类型。

return json({
  ok: true,
  id: subId,
  version,
  status,
  review_note: note,
  // ↓ 通过时也要给 items（可以为空数组）——前端 v-for 依赖它存在
  gate: { passed: gate.ok, items: gateItems },
  screenshots: shotIds.length,
})
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

