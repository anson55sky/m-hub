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

/** 所有管理端 handler 的名字（改了一个就自动进列表） */
const HANDLERS = [
  'listDevApplications',
  'approveDevApplication',
  'rejectDevApplication',
  'listSubmissions',
  'approveSubmission',
  'rejectSubmission',
] as const

test('每个管理端 handler 都必须先过 requireAdmin', () => {
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