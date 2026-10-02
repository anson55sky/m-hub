// 管理端门禁测试。
//
// ## 为什么这条最该测
//
// 越权漏洞的形状是「**少写一个 if**」—— 编译通过、类型通过、所有路由测试都过，
// 因为根本没有测试会去调用那个端点用非管理员身份。
//
// 本文件的每条断言都基于这个判据：**任何 handler 少了 requireAdmin，
// 它的「非管理员被拒」那条就会红。** 所以下面的测试是逐个 handler 列的，
// 新增 handler 时必须补一条 —— 否则新端点是裸的。

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const ADMIN_SRC = readFileSync(join(HERE, 'admin.ts'), 'utf8')
const HANDLE_SRC = readFileSync(join(HERE, '..', 'handle.ts'), 'utf8')

/**
 * 所有管理端 handler 的名字 —— **从源码推导，不手写列表**。
 *
 * ⚠️ 原来是一份手写的 `const HANDLERS = [...]`。那是本文件自己警告过的东西：
 *   「新增 handler 时必须补一条 —— 否则新端点是裸的」，可「必须」靠自觉，
 *   半年后没人记得。实测已经漏过一次：`submissionPackage` 不在列表里，
 *   它自己有门禁，纯属侥幸；下一个新 handler 就会真的裸奔。
 *
 * 推导方式：`export async function <name>(ctx: Ctx` —— 要求**签名里带 ctx**
 * 才能直接判它是不是 handler。`requireAdmin` **也**符合这个形状（它的参数
 * 就是 ctx: Ctx，返回门禁本身），所以它同样要排除，否则会断言
 * 「requireAdmin 必须调用 requireAdmin」这种套娃。
 *
 * ⚠️ 第一版我以为「`requireAdmin` 没有 ctx 参数、会被自然排除」—— 读代码才发现
 *   `requireAdmin(ctx: Ctx): Promise<...>` 是带 ctx 的，于是新增 handler 的那一刻
 *   测试立刻红。红得对：排除名单漏了它。
 */
const NOT_A_HANDLER = new Set(['requireAdmin'])
const HANDLERS = [
  ...new Set(
    [...ADMIN_SRC.matchAll(/export async function ([a-zA-Z0-9_]+)\(ctx: Ctx/g)]
      // 捕获组在 TS 6 的类型里是 `string | undefined`，这里先滤掉 undefined：
      // 宁可多一行，也别用 `!` 断言 —— 断言会让「正则写错导致全 undefined」
      // 这种真故障在类型层面消失，只剩运行时的假绿。
      .map((m) => m[1])
      .filter((n): n is string => typeof n === 'string'),
  ),
].filter((n) => !NOT_A_HANDLER.has(n)).sort()

test('每个管理端 handler 都必须先过 requireAdmin', () => {
  // 先确认推导本身没退化成空列表 —— 推导错了的话下面这个 for 一次都不跑、
  // 测试全绿，而它守的东西一个都没守（约定 73：假绿比红更糟）。
  assert.ok(HANDLERS.length >= 7, `只推导出 ${HANDLERS.length} 个 handler，列表解析多半坏了`)

  // 逐个切出函数体，检查第一件事是不是门禁。
  // 只检查「文件里出现过 requireAdmin」是不够的 —— 那等于没检查。
  for (const name of HANDLERS) {
    const i = ADMIN_SRC.indexOf(`export async function ${name}(`)
    assert.ok(i > -1, `找不到 handler ${name}`)
    const body = ADMIN_SRC.slice(i, i + 700)
    const first = body.indexOf('await requireAdmin(ctx)')
    assert.ok(
      first > -1,
      `${name} 没有调用 requireAdmin —— 这就是越权漏洞（少写一个 if，编译与类型检查都发现不了）`,
    )
    // 门禁之后必须 `if (admin instanceof Response) return admin`
    assert.match(
      body.slice(first, first + 200),
      /instanceof Response\) return \w+/,
      `${name} 拿到 requireAdmin 的结果后没有 return —— 门禁形同虚设（拿到的 Response 被忽略）`,
    )
  }
})

/**
 * 取某个函数的正文（花括号配平；签名跨行也不受影响）。
 *
 * ⚠️ 同时认  /  /  ——
 *    第一版只认 ，于是 （普通 function）
 *    取不到、两条测试假红。取样器漏一种形态比测试红更糟：
 *    它让人以为「这个函数不存在所以没法测」。
 */
function fnBody(name: string, src = ADMIN_SRC): string {
  const i = Math.max(
    ...['export async function ', 'export function ', 'function ', 'async function ']
      .map((k) => src.indexOf(`${k}${name}(`))
      .filter((x) => x > -1),
  )
  assert.ok(i > -1, `找不到 ${name}`)
  const open = src.indexOf('{', src.indexOf(')', i))
  let depth = 0
  for (let k = open; k < src.length; k++) {
    if (src[k] === '{') depth++
    else if (src[k] === '}') {
      depth--
      if (depth === 0) return src.slice(open, k)
    }
  }
  throw new Error(`${name} 花括号不配平`)
}

test('requireAdmin 分开 401 与 403（混起来客户端会一直让用户重新登录）', () => {
  const body = fnBody('requireAdmin')
  assert.match(body, /unauthorized\(\)/, '未登录应回 401')
  assert.match(body, /'forbidden'/, '已登录但非管理员应回 403')
  assert.ok(
    body.indexOf('unauthorized()') < body.indexOf("'forbidden'"),
    '先判会话（401）再判角色（403）',
  )
})

test('门禁依据是 role=admin（不是 is_developer）', () => {
  // ⚠️ 这两个**不是一回事**：`is_developer` 是「能发布扩展」，
  //    `role='admin'` 才是「能审核别人的东西」。用错的话每个开发者都能审核 ——
  //    那等于没有审核。
  // ⚠️ 机器凭据分支返回的对象里**必须**带 role:'admin'（它不查库）。
  const body = fnBody('requireAdmin')
  assert.match(body, /user\.role !== 'admin'/, '会话路径必须查 role')
  assert.match(
    body,
    /role: 'admin'/,
    '机器凭据分支返回的对象缺 role=admin —— 它绕过查库，必须自带这个身份',
  )
  assert.ok(
    !/user\.is_developer|!user\.is_developer/.test(body),
    'requireAdmin 里不该出现 is_developer —— 它是「能发布扩展」，不是「能审核」',
  )
})

test('机器凭据：未配置 ADMIN_TOKEN 时一律拒绝（不能「没配就放行」）', () => {
  // 「先上线、密钥后补」的过程中最危险的一种写法就是 `if (!token) return true`。
  const body = fnBody('machineOk')
  assert.match(body, /if \(!expect \|\| !token\) return false/, '没配置或没给 token 必须拒绝')
  assert.ok(!/return true/.test(body.split('return timingSafeEqual')[0] ?? ''), '前置判断里不许有放行分支')
})

test('机器凭据用定长比较（不用 ===，那会因提前返回泄露前缀）', () => {
  assert.match(fnBody('machineOk'), /timingSafeEqual\(/, '应用 timingSafeEqual 而不是 ===')
})

test('驳回必须填理由（只给「已驳回」等于让申请人瞎猜）', () => {
  for (const fn of ['rejectDevApplication', 'rejectSubmission']) {
    const i = ADMIN_SRC.indexOf(`export async function ${fn}(`)
    const body = ADMIN_SRC.slice(i, i + 1200)
    assert.match(body, /if \(!note\)/, `${fn} 没校验理由为空`)
    assert.match(body, /bad_request/, `${fn} 应回 400 而不是静默通过`)
  }
})

test('开发者申请通过时 users 表两个字段都要改', () => {
  // ⚠️ 只改 developer_status 的症状与「没通过」一模一样：
  //    界面显示已通过，但 requireDeveloper（查 is_developer）仍拒发布。
  const i = ADMIN_SRC.indexOf('export async function approveDevApplication')
  const body = ADMIN_SRC.slice(i, i + 2000)
  assert.match(body, /developer_status = 'approved'/, 'developer_status 没改')
  assert.match(body, /is_developer = 1/, 'is_developer 没改 —— 发布扩展仍会被拒')
})

test('审核状态流转只允许从 pending 出发（防止重复审核乱跳 updated_at）', () => {
  for (const fn of ['approveDevApplication', 'approveSubmission']) {
    const i = ADMIN_SRC.indexOf(`export async function ${fn}(`)
    const body = ADMIN_SRC.slice(i, i + 2000)
    assert.match(
      body,
      /already_reviewed|status !== 'pending_review'/,
      `${fn} 没拦「非 pending 的状态」`,
    )
  }
})

test('审核扩展时重跑关卡，不信任提交时的结果', () => {
  const i = ADMIN_SRC.indexOf('export async function approveSubmission')
  const body = ADMIN_SRC.slice(i, i + 3000)
  assert.match(body, /runGate\(/, '审核时没跑关卡')
  assert.match(body, /gate_failed/, '关卡不过应把状态改成 gate_failed')
  assert.match(body, /package_missing/, '包体缺失要单独报错（不能当过关）')
})

test('取包体只对 approved 开放（否则这是个「读未审核用户代码」的洞）', () => {
  const body = fnBody('submissionPackage')
  assert.match(body, /status !== 'approved'/, '非 approved 必须拦')
  assert.match(body, /not_approved/, '要有专门错误码，让发布脚本能分辨「没批准」与「没这条」')
  assert.match(body, /no-store/, '包体按 sha256 变化，缓存错了就是「下到旧版本」')
})

test('管理端路由都注册了，且没混进 paths.ts::DYNAMIC', () => {
  // 注册漏了 → 端点 404；混进 DYNAMIC → 契约守卫会要求 api_spec.rs 也有，
  // 而客户端永远不会调它。
  for (const p of [
    '/api/v1/admin/dev-applications',
    '/api/v1/admin/dev-applications/:id/approve',
    '/api/v1/admin/dev-applications/:id/reject',
    '/api/v1/admin/submissions',
    '/api/v1/admin/submissions/:id/approve',
    '/api/v1/admin/submissions/:id/reject',
    '/api/v1/admin/submissions/:id',
    '/api/v1/admin/submissions/:id/mark-published',
    // ⚠️ 这条最容易漏：漏了的话「审核通过 → 发布」会在最后一步 404，
    //    而前面所有环节都显示成功。
    '/api/v1/admin/submissions/:id/package',
  ]) {
    assert.ok(HANDLE_SRC.includes(`'${p}'`), `handle.ts 没注册 ${p}`)
  }
  const pathsSrc = readFileSync(join(HERE, '..', 'lib', 'paths.ts'), 'utf8')
  assert.ok(
    !pathsSrc.includes('/api/v1/admin/'),
    'paths.ts::DYNAMIC 里不该有 admin 路径 —— 它是客户端契约，管理端没有客户端',
  )
})
// ---------------------------------------------------------------- 删除已终结的提交

/**
 * 删除只对**已终结**的提交开放。
 *
 * ⚠️ 这条断言守的是「应用层 if 忘了写 → 未终结的提交被删掉」。
 * 后果不是数据脏一条那么简单：submissions 上有
 * `idx_sub_one_open` 这个**部分唯一索引**（同一扩展同时只允许一条
 * uploaded/pending_review/gate_failed），删掉一条等于给该扩展**解锁** ——
 * 于是「同一扩展同时只允许一个待审版本」这条产品口径被绕过。
 * 所以约束必须落在 SQL 的 `WHERE status IN (...)` 里，不能只在 JS 里判。
 */
test('删除只允许已终结状态，且约束必须写进 SQL（不只是 JS 里的 if）', () => {
  const body = fnBody('deleteSubmission')
  // 两种合法形态都要认：`IN ('approved', …)` 逐字写死，或
  // `IN (${placeholders})` 由常量数组插值生成（本实现是后者）。
  // ⚠️ 第一版只认逐字形式，于是本实现明明已经把白名单写进 SQL，测试却报
  //   「没有 status IN」—— 与「取样器漏了一种形态」同类：红得没有意义。
  assert.match(
    body,
    /status IN \([\s\S]{0,120}?\)/,
    "删除的 SQL 里必须有 `status IN (...)` 白名单 —— 靠 JS 的 if 兜不住并发",
  )
  assert.ok(
    /DELETABLE_STATUSES/.test(body),
    '删除走 DELETABLE_STATUSES 常量，别把状态列表内联到 SQL 里再各写一份',
  )
  // 白名单本身不得含 published（已在架的删了会让清单指向不存在的字节）
  const list = ADMIN_SRC.slice(
    ADMIN_SRC.indexOf('DELETABLE_STATUSES = ['),
    ADMIN_SRC.indexOf(']', ADMIN_SRC.indexOf('DELETABLE_STATUSES = [')),
  )
  assert.ok(
    !list.includes('published'),
    'DELETABLE_STATUSES 里不能有 published：已上架的必须先下架再删',
  )
  assert.ok(
    !list.includes('pending_review') && !list.includes('gate_failed'),
    'DELETABLE_STATUSES 里不能有未终结状态：删掉它们会绕过 idx_sub_one_open',
  )
})

/**
 * 包体是**内容寻址**的（同一份字节只存一份），所以删提交时不能直接
 * `DELETE FROM pkg_blobs WHERE sha256 = ?` —— 别��提交指向同一个 sha256，
 * 删了它们的包体，那些提交审核通过后就发布不出来（package_missing）。
 *
 * 故必须先确认「没有任何提交还引用它」。
 */
test('删提交时包体只在无引用时才清（内容寻址，删了会连坐别人）', () => {
  const body = fnBody('deleteSubmission')
  assert.match(body, /COUNT\(\*\) AS n FROM submissions WHERE pkg_sha256/, '删 blob 前必须查引用数')
  assert.match(body, /stillUsed\?\.n \?\? 0\) === 0/, '引用数为 0 才清 blob')
  assert.ok(
    body.indexOf('stillUsed') < body.indexOf('DELETE FROM pkg_blobs'),
    '必须先查引用再删 blob —— 顺序反了等于永远删不掉或永远连坐',
  )
})

/** 路由已注册（少注册 = 最后一步 404，而前面每一步都显示成功） */
test('DELETE /api/v1/admin/submissions/:id 已注册', () => {
  assert.ok(
    HANDLE_SRC.includes("r.add('DELETE', '/api/v1/admin/submissions/:id'"),
    'handle.ts 没注册 DELETE 路由 —— 审核与删除都会在最后一步 404',
  )
  const http = readFileSync(join(HERE, '..', 'lib', 'http.ts'), 'utf8')
  assert.match(
    http,
    /type Method = 'GET' \| 'POST' \| 'DELETE'/,
    "Method 联合类型里没有 DELETE —— 路由表会把它当成不支持的方法",
  )
})

/**
 * `approved → published` 是**单向**的。
 *
 * ⚠️ 若允许从 published 改回别的状态，就等于在本接口里偷偷实现了「下架」——
 *   而下架必须走「写 revoked + 重签清单」两步（约定 60），只改状态的话
 *   客户端会把一个仍在清单里的版本判成已上架/已下架，说一句与事实相反的话。
 */
test('published 是单向终点：SQL 的 WHERE 只接受 approved', () => {
  const body = fnBody('markSubmissionPublished')
  assert.match(
    body,
    /AND status = 'approved'/,
    "UPDATE 必须带 `AND status = 'approved'` —— 否则就允许从 published 改回去",
  )
  assert.ok(
    !/SET status = '(withdrawn|rejected|pending_review|gate_failed)'/.test(body),
    '本接口不许把状态改成任何未终结状态',
  )
})

/**
 * `published` 必须**可达**，否则「在架版本不许删」是死代码。
 *
 * ⚠️ 这条守的是一个真事故：删除接口按「status 不是 published 才允许删」保护在架版本，
 *   而 `publish-approved.mjs` 从不把状态改成 published —— 于是正在市场里服役的
 *   版本可以被直接删掉，而那段保护读起来像是有防护的。
 */
test('published 状态可达：发布脚本必须在部署成功后标记它', () => {
  // HERE = src/routes，所以要到仓库根下的 scripts/ 得退**两级**。
  // 第一版只退一级（src/scripts/）→ ENOENT。而 ENOENT 让断言直接失败时，
  // 看起来像「实现错了」，实际是取样器路径写错（约定 73 的老熟人）。
  const pub = readFileSync(join(HERE, '..', '..', 'scripts', 'publish-approved.mjs'), 'utf8')
  // ⚠️ 必须匹配**完整路径**：`/mark-published/` 太松 ——
  //   把调用改成 `/mark-published-XX`（一个不存在的接口）它照样匹配，
  //   实测变异时才发现。而「调了个不存在的接口」正好是本条要防的故障。
  // ⚠️ 第二版补了尾部反引号锚点：只写 `\/mark-published` 的话，
  //   `/mark-published-typo` 是它的**前缀**，照样匹配 —— 变异时又抓了一次。
  //   判据必须锚到完整 token，否则它守的是一个真子串。
  assert.match(
    pub,
    /\/api\/v1\/admin\/submissions\/\$\{[^}]+\}\/mark-published`/,
    'publish-approved.mjs 从不把提交标记为 published → published 状态不可达 → 删除保护是死代码',
  )
  // 顺序：部署之后才改状态。反了的话部署失败而状态已改，作者侧会说「已上架」
  // 而用户根本装不到 —— 与事实相反的谎报。
  // ⚠️ 必须匹配**真正的部署调用**（execFileSync 的 argv），不能匹配文案：
  //   脚本里那句「清单改动要等 pages deploy 才生效」也含 'pages deploy'，
  //   拿它定位会得出「部署在标记之前」的正确结论 —— 但那是因为碰巧。
  //   一旦文案改写，判据就悄悄换了指向（约定 74：数据源换了口径，
  //   匹配的代码没跟着换 —— 这里反过来：判据本身换了口径）。
  const deployAt = pub.indexOf("'pages', 'deploy'")
  const markAt = pub.indexOf('markPublished(approved)')
  assert.ok(deployAt > -1, "没找到 wrangler 的部署调用（找的是 argv ['pages','deploy']）")
  assert.ok(markAt > -1, '没找到 markPublished(approved) 调用')
  assert.ok(
    deployAt < markAt,
    '必须先部署成功、再标记 published；反过来会在部署失败时说「已上架」',
  )
})

/**
 * 只能标记**真正进了市场清单**的那些提交。
 *
 * ⚠️ 实测踩过：同一扩展连着批了 v0.1.5 与 v0.1.6，发布脚本把**两条**都标成
 * `published`，而清单里只有 v0.1.6。结果 v0.1.5 在作者侧显示「已上架」
 * ——一句与事实相反的话；更要命的是它因此被「在架版本不许删」这条保护
 * 挡住，作者想删旧版却删不掉（正是用户报的「如何删除旧版」）。
 *
 * 所以判据是「上架集合」，不是「approved 集合」。
 */
test('只标记真正上架的那条：同一扩展多条 approved 时按版本取最高', () => {
  const pub = readFileSync(join(HERE, '..', '..', 'scripts', 'publish-approved.mjs'), 'utf8')

  assert.match(
    pub,
    /const latest = new Map\(\)/,
    '上架集合必须按扩展去重后取最高版本，不能直接遍历候选',
  )
  assert.ok(
    /markPublished\(approved\)/.test(pub),
    '标记 published 的必须是最终那批（已按版本去重）',
  )
  // ⚠️ 「要上架的」必须是**最新版里还没 published 的那些**，不能把已上架的也
  //   重发一遍 —— `GET .../package` 只对 approved 发包，重发会拿到 409。
  assert.match(
    pub,
    /\[\.\.\.latest\.values\(\)\]\.filter\(\(s\) => s\.status === 'approved'\)/,
    "上架集合必须是 latest 里 status==='approved' 的那些（已上架的不重发）",
  )
  assert.ok(
    /function cmpVersion/.test(pub),
    '版本比较函数不见了 —— 去重逻辑依赖它',
  )

  // 覆盖顺序不得再依赖接口的 ORDER BY：那是巧合，不是保证
  const unpackAt = pub.indexOf('const xhpack = join(STAGE')
  const loopAt = pub.lastIndexOf('for (const s of approved)', unpackAt)
  assert.ok(
    loopAt > -1 && loopAt < unpackAt,
    '解包循环必须遍历去重后的上架集合；遍历原始候选会让旧版本覆盖新版本',
  )
})

/**
 * 候选集必须含 `published`，否则「最新的发完 → 旧的没法发」形成死锁。
 *
 * ⚠️ 实测链条：v0.1.5 / v0.1.6 都 approved → 发布挑最高（v0.1.6）并标记
 *   published → 下次跑只看 approved，候选只剩 v0.1.5 → 清单被**旧版本顶掉**。
 *   而用户想删旧版时又因为「published 不许删」被挡住（用户报的就是这个）。
 */
test('发布候选集 = approved ∪ published（只取 approved 会把清单退回旧版本）', () => {
  const pub = readFileSync(join(HERE, '..', '..', 'scripts', 'publish-approved.mjs'), 'utf8')
  assert.match(
    pub,
    /s\.status === 'approved' \|\| s\.status === 'published'/,
    "候选集漏了 'published' —— 最新一条发完后它就不进候选集，比版本号时只剩更早的 approved，清单会退回旧版本",
  )
})
