import { test } from 'node:test'
import assert from 'node:assert/strict'
import { errorText } from './errorText.ts'

/**
 * 这条链路的价值全在「界面不再显示一串错误代码」。
 * 每条用例都做过变异验证：删掉对应判断后必须变红。
 */

test('剥掉 CODE: 前缀 —— 发布说明那条「不再显示一串错误代码」就是这个', () => {
  assert.equal(
    errorText('QUOTA_EXHAUSTED: 今日平台额度已用完，明天再来'),
    '今天的 AI 额度已经用完了，明天再来（今日平台额度已用完，明天再来）',
  )
  // ⚠️ 没有收录的码也必须剥掉前缀：否则「一串错误代码」照样显示在界面上
  assert.equal(errorText('SOMETHING_NEW: 说明在这里'), '说明在这里')
})

test('已知码补上「下一步」（光有现象没有下一步，用户只会反复重试）', () => {
  const t = errorText('INVITE_REQUIRED: 需先兑换邀请码才能使用平台额度')
  assert.ok(t.includes('设置 → 账号'), `应指出去哪里兑换：${t}`)
})

test('⚠️ 变异：把收录表清空必须变红', () => {
  const t = errorText('INVITE_REQUIRED: 需先兑换邀请码')
  assert.ok(t.includes('设置 → 账号'), '收录表生效')
})

test('认不出的码原样保留 message —— 猜一个原因比不解释更糟', () => {
  assert.equal(errorText('WEIRD_NEW_CODE: 出了点问题'), '出了点问题')
  // 没有冒号的东西（本地异常、网络库的原生报错）不该被削掉任何内容
  assert.equal(errorText('boom'), 'boom')
  assert.equal(errorText('fetch failed'), 'fetch failed')
})

test('非字符串入参不炸（catch 到 undefined / Error / 对象都要能显示）', () => {
  assert.equal(errorText(undefined), 'undefined')
  assert.equal(errorText(null), 'null')
  // ⚠️ Error 实例必须取 .message：`String(e)` 会带出 'Error: ' 前缀，
  //   界面上就多一个「Error:」—— 与我们要消灭的错误代码是同一种毛病
  assert.equal(errorText(new Error('网络断了')), '网络断了')
  assert.equal(errorText(new TypeError('x is not a function')), 'x is not a function')
  assert.equal(errorText({ code: 1 }), '[object Object]')
})

test('⚠️ 只有全大写代码才当代码；普通句子里的冒号不能被吃掉', () => {
  // 这条最容易被「顺手把正则放宽」改坏：
  // 放宽成 `^\\w+:` 之后，「注意：点这里」会被削成「点这里」，界面上凭空少一句。
  assert.equal(errorText('注意：点这里可以兑换'), '注意：点这里可以兑换')
  assert.equal(errorText('IO_ERROR: 读文件失败'), '读写文件失败，检查权限后重试（读文件失败）')
  assert.equal(errorText('MADE_UP_CODE: 某条没收录的说明'), '某条没收录的说明')
})
