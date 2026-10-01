// `/me` 响应形状 + 开发者准入判定的守卫。
//
// ## 事故背景（2026-10-01，真实）
//
// 客户端一直从 `/me` 读 `can_apply_developer` 决定显示「还没有申请」还是
// 「先兑换邀请码才能申请」，而 `publicUser()` **从来没返回过这个字段**。
// Rust 侧 `unwrap_or(false)` 把它兜成 `false` → 界面同时显示
// 「已兑换邀请码」与「先兑换邀请码才能申请」，**自相矛盾且零报错**
// （tsc 通过、部署通过、单测全绿，只有实机看得见）。
//
// 更气人的是：正确答案在 `applyStatus()` 里**一直算着**，只是没人调用。
// 两份真相、一份坏掉，被用的恰好是坏的那份。
//
// ## 这两条分别守什么
//
// · `canApplyDeveloper`：import **真代码**直接断言五种状态组合。
// · `publicUserFields`：断言**响应形状**（字段在不在）—— 判「值」需要
//   真实用户 fixture，那样会绕过真正的风险（本次正是「源码少写字段」，
//   而运行时一切正常）。
//
// ⚠️ 关于「读源码文本」的教训（守卫自己也踩了）：
// 我第一版守卫靠**解析源码**来「调用」`canApplyDeveloper` 的逻辑，
// 结果连错三次：函数签名跨行 → 截断点猜错；类型标注里的 `{...}` →
// 花括号配平提前结束；改成按分号截 → 又截过头。
// **把编译器的活抢过来做，必然出错。** 逻辑能 import 就 import；
// 只有「字段在不在」这类无法 import 的断言才该读文本。
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { canApplyDeveloper } from '../lib/developerGate.ts'

const ME_TS = join(dirname(fileURLToPath(import.meta.url)), 'me.ts')

test('canApplyDeveloper —— 五种状态组合', () => {
  // 已兑换 + 未申请 → 可申请（本次事故里界面该显示却显示成「不能」）
  assert.equal(canApplyDeveloper({ invite_redeemed: 1, developer_status: 'none' }), true)
  // 未兑换 → 不可
  assert.equal(canApplyDeveloper({ invite_redeemed: 0, developer_status: 'none' }), false)
  // 已申请过 → 不可（别让人重复申请）
  assert.equal(canApplyDeveloper({ invite_redeemed: 1, developer_status: 'pending' }), false)
  assert.equal(canApplyDeveloper({ invite_redeemed: 1, developer_status: 'approved' }), false)
  assert.equal(canApplyDeveloper({ invite_redeemed: 1, developer_status: 'rejected' }), false)
})

test('publicUserFields —— 客户端会读的字段一个都不能少', () => {
  const src = readFileSync(ME_TS, 'utf8')
  const start = src.indexOf('function publicUser')
  assert.ok(start > -1, 'me.ts 里找不到 publicUser —— 守卫需同步更新')
  // 找到 `return {` 之后那个对象字面量的范围（花括号配平在这里是安全的：
  // 前面是函数签名，签名里虽然也有对象类型标注，但我们要的 `return {` 更靠后）
  const ret = src.indexOf('return {', start)
  assert.ok(ret > -1, 'publicUser 里找不到 return {')
  let depth = 0
  let end = -1
  for (let i = src.indexOf('{', ret); i < src.length; i++) {
    if (src[i] === '{') depth++
    else if (src[i] === '}') {
      depth--
      if (depth === 0) {
        end = i
        break
      }
    }
  }
  assert.ok(end > -1, 'publicUser 的返回对象花括号不配平')
  const keys = new Set([...src.slice(ret, end).matchAll(/^\s+([a-z_]+):/gm)].map((m) => m[1]))

  // ⚠️ 这份清单**逐条对应** Rust `account.rs::status_from_me` 里 `me.get(...)`
  //    的每一个键。少一个 → Rust `unwrap_or(默认值)` 静默吞掉 → 界面说错话。
  const required = [
    'username',
    'email',
    'avatar',
    'role',
    'is_developer',
    'developer_status',
    'invite_redeemed',
    'can_apply_developer', // ← 本次事故的元凶：曾漏在这里
    'platform_ai_available',
    'platform_ai_reason',
  ]
  const missing = required.filter((f) => !keys.has(f))
  assert.deepEqual(
    missing,
    [],
    `publicUser 少了字段：${missing.join(', ')}\n` +
      `    客户端读它、却读到 undefined → Rust unwrap_or 兜成默认值 → 界面显示与事实相反。\n` +
      `    加字段时记得同步 account.rs::status_from_me 与 src/api/tauri.ts 的 AccountStatus。`,
  )
})