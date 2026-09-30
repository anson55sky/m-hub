// Router 的判定逻辑。
//
// 为什么单独测它：`resolve()` 返回 404 / 405 / handler 三种结果，而它们
// 对应**三种不同的病因**却长得几乎一样（都是「请求失败了」）：
//   404 → 路径拼错 / 没注册
//   405 → 路径对、方法错（`r.add('GET', …)` 写成 POST）
// 前者要改路径，后者要改方法。只回 404 的话，方法写错会伪装成
// 「服务端没这个接口」，排查方向直接跑偏（约定 52 记的就是这类）。

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { Router } from './http.ts'
import { MOUNTED, DYNAMIC, STATIC, OPENAI_COMPAT } from './paths.ts'

const noop = () => new Response('ok')

/** 建一个跟 index.ts 同形的路由器（只测匹配，不测业务）。 */
function sample() {
  const r = new Router()
  r.add('GET', '/me', noop)
  r.add('POST', '/api/v1/me/redeem', noop)
  // 同一个路径同时有 GET 与 POST —— `/api/v1/dev/apply` 就是这个形状
  r.add('POST', '/api/v1/dev/apply', noop)
  r.add('GET', '/api/v1/dev/apply', noop)
  r.add('GET', '/api/v1/dev/submissions/:id', noop)
  r.add('POST', '/api/v1/dev/submissions/:id/withdraw', noop)
  return r
}

test('命中注册项时给出 handler 与路径参数', () => {
  const hit = sample().resolve('GET', '/api/v1/dev/submissions/42')
  assert.equal(hit.kind, 'ok')
  // 判别联合让这里能安全收窄，不需要 as 强转
  assert.equal(hit.kind === 'ok' && hit.params.id, '42')
})

test('参数段被 percent 解码（扩展 id 形态是 slug，但别把解码漏了）', () => {
  const hit = sample().resolve('GET', '/api/v1/dev/submissions/a%2Fb')
  assert.equal(hit.kind === 'ok' && hit.params.id, 'a/b')
})

test('路径不存在 → not_found（不是 method_not_allowed）', () => {
  assert.equal(sample().resolve('GET', '/api/v1/nope').kind, 'not_found')
  // 段数不同也算路径不存在：少一段 / 多一段都不是「方法不对」
  assert.equal(sample().resolve('GET', '/me/extra').kind, 'not_found')
  assert.equal(sample().resolve('GET', '/').kind, 'not_found')
})

test('路径存在但方法不对 → method_not_allowed 并带 allow（这是「方法写错」的唯一信号）', () => {
  // `/me` 只注册了 GET
  const bad = sample().resolve('POST', '/me')
  assert.equal(bad.kind, 'method_not_allowed')
  // Allow 头的内容要能直接告诉排查者「该用哪个方法」
  assert.deepEqual(bad.kind === 'method_not_allowed' && bad.allow, ['GET'])
  // 同一个路径两种方法都注册时，任一方法都命中，不该误报 405
  const r = sample()
  assert.equal(r.resolve('GET', '/api/v1/dev/apply').kind, 'ok')
  assert.equal(r.resolve('POST', '/api/v1/dev/apply').kind, 'ok')
})

test('中间的双斜杠不匹配（拼 URL 的 bug 要响亮地 404，不能猜成别的端点）', () => {
  // 曾经用 filter(Boolean) 把 `//` 折叠掉，于是这条被当成 id='withdraw'
  // 命中了 withdraw 路由 —— 一处拼接 bug 被静默匹配成**另一个接口**
  assert.equal(sample().resolve('GET', '/api/v1/dev/submissions//withdraw').kind, 'not_found')
  assert.equal(sample().resolve('GET', '//me').kind, 'not_found')
})

test('单个尾斜杠从宽：/me/ ≡ /me（常规写法，且这套路由里不可能歧义）', () => {
  const r = new Router()
  r.add('GET', '/me', noop)
  assert.equal(r.resolve('GET', '/me/').kind, 'ok')
  // 但两个尾斜杠就该不匹配
  assert.equal(r.resolve('GET', '/me//').kind, 'not_found')
})

test('MOUNTED === DYNAMIC 的键集合（防「加了表忘了挂」）', () => {
  const dyn = Object.keys(DYNAMIC)
  assert.deepEqual([...MOUNTED].sort(), [...dyn].sort())
})

test('静态清单路径与 OpenAI 兼容面都在表里，且格式对', () => {
  // 约定 46：市场清单 v2、升级清单 v1 —— 两份清单各有一套版本号，别弄反
  assert.equal(STATIC.marketRegistry, '/api/v1/market/registry')
  assert.equal(STATIC.appUpdate, '/api/v1/app/update')
  // 约定 33：平台对话是 /v1/chat/completions，**没有** GET /v1/models
  assert.equal(OPENAI_COMPAT.post_chat_completions, '/v1/chat/completions')
  assert.ok(!Object.values(OPENAI_COMPAT).some((p) => p.endsWith('/models')))
})

test('所有路径都以 / 开头且不含尾斜杠（尾斜杠会让静态资源匹配落空）', () => {
  const all = [
    ...Object.values(STATIC),
    ...Object.values(DYNAMIC),
    ...Object.values(OPENAI_COMPAT),
  ]
  for (const p of all) {
    assert.ok(p.startsWith('/'), `路径须以 / 开头: ${p}`)
    assert.ok(!p.endsWith('/'), `路径不得以 / 结尾: ${p}`)
  }
})

test('/me 走根路径，不在 /api/v1 下（约定 52 的坑）', () => {
  assert.ok(DYNAMIC.get_me.endsWith('/me'))
  // 拓宽成 string[]：as const 让值成了字面量联合，`includes('/api/v1/me')`
  // 会被编译器直接拒掉 —— 那**恰好**说明表里没有它，但报的是类型错而不是
  // 一条能读懂的断言失败。显式拓宽，让失败信息是人话。
  const paths = Object.values(DYNAMIC) as string[]
  assert.ok(
    !paths.includes('/api/v1/me'),
    '服务端不能有 /api/v1/me —— 客户端只取根路径 /me（约定 52）',
  )
})
