/**
 * 守卫：扩展 iframe 白屏看门狗必须先重试一次，才允许判死
 *
 * ## 为什么需要它
 *
 * 看门狗超时的语义是「入口 HTML 已返回，但 iframe 在超时内没有任何桥消息」。
 * 判定本身没错，错的是**只有一条出路**：直接判死。
 *
 * 实测存在一种**会自愈**的挂起 —— iframe 发起 `mhub-ext://` 协议请求时，扩展反向
 * 代理 / 协议处理器还没就位，这次请求既不返回**也不报错**（连 iframe 的 `error`
 * 事件都不触发）。于是一个本可自愈的加载被永久判成白屏。
 *
 * 而界面上没有自救入口：空白态那颗「返回」只 `emit('close')`，不会重新导航 iframe。
 * 所以用户看到的是一个彻底死掉的格子，只能重启应用 —— 实测 5 次启动里中了 3 次。
 *
 * 代价是这种失败**只在冷启动那几秒出现**：日志里 `扩展入口就绪` 是 INFO、一切正常，
 * 只有 8 秒后一行 ERROR。所以它很容易被当成「偶发，重启就好」而长期留着。
 *
 * ## 判据（四条都要满足）
 *
 *  ① 重试次数上限存在且 ≥ 1 —— 0 等于没修。
 *  ② 重试分支在「判死并写 error」**之前**，且中间有 return：否则重试只是白走一遍。
 *  ③ 重试真的**重新导航**（回到 about:blank 再赋回真实 URL），不是只清了 error。
 *  ④ 重试尊重 `loadGeneration`：期间若已发生新的 load()，重放旧 URL 会把 iframe
 *     打到过期入口上。这一条是最容易写漏的 —— 它在正常路径上完全看不出来。
 *
 * 守卫不求值，只认源码结构。但这几条都是「结构上必须成立」的不变量：
 * 删掉任何一条，这个 bug 就静默回来，而没有任何测试会红。
 */
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const SRC = join(root, 'src/composables/useExtensionFrame.ts')

const problems = []
const src = readFileSync(SRC, 'utf8')

/** 去掉行注释与块注释：注释里的 `{}` / `return` 会让结构判断失真 */
function stripComments(s) {
  return s.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '')
}

/** 从 `sig` 处起做花括号配对，取出函数体（跳过字符串/注释里的括号，避免被注释里的 `{}` 带偏） */
function bodyFrom(sig) {
  const at = src.indexOf(sig)
  if (at < 0) return null
  const open = src.indexOf('{', at + sig.length)
  if (open < 0) return null
  let depth = 0
  for (let i = open; i < src.length; i++) {
    const c = src[i]
    if (c === '{') depth++
    else if (c === '}') {
      depth--
      if (depth === 0) return src.slice(open, i + 1)
    }
  }
  return null
}

const arm = bodyFrom('function armWatchdog')
const load = bodyFrom('async function load')
if (!arm) problems.push('找不到 armWatchdog 函数体：看门狗逻辑被挪走或改名了')
if (!load) problems.push('找不到 load 函数体')

// ① 重试上限存在且 ≥ 1
const retriesDecl = src.match(/const\s+EXT_LOAD_RETRIES\s*=\s*(\d+)/)
if (!retriesDecl) problems.push('缺少 EXT_LOAD_RETRIES 常量：重试次数没有单一来源')
else if (Number(retriesDecl[1]) < 1) {
  problems.push(`EXT_LOAD_RETRIES = ${retriesDecl[1]}，等于不重试 —— 这个 bug 会原样回来`)
}

if (arm) {
  // ② 重试分支必须早于判死，且有 return
  const retryAt = arm.search(/watchdogRetries\s*<\s*EXT_LOAD_RETRIES/)
  const fatalAt = arm.search(/error\.value\s*=\s*['"`]扩展加载失败/)
  if (retryAt < 0) problems.push('armWatchdog 里没有「重试次数未用尽」的分支：超时会直接判死')
  if (fatalAt < 0) problems.push('armWatchdog 里找不到判死时写 error 的语句')
  if (retryAt >= 0 && fatalAt >= 0) {
    if (retryAt > fatalAt) {
      problems.push('重试分支写在判死之后：等于永远走不到重试')
    } else {
      // 必须精确校验「重试分支自己的最后一句是 return」。
      // 早先用「retryAt..fatalAt 之间出现过 return 就算过」—— 那太松：
      // 分支里 `if (!el) return` 那种守卫 return 会冒充，删掉真正的收尾 return
      // 反而检查通过。而漏掉这个 return 的后果正是重试后又掉进判死。
      const retryBody = bodyFrom('watchdogRetries < EXT_LOAD_RETRIES')
      if (!retryBody) {
        problems.push('取不到重试分支的函数体：分支结构变了，请同步本守卫')
      } else {
        // 去掉这个 if 块**自己的**收尾大括号，再看剩下部分的最后一句。
        // 不能直接对整段做 /return$/：`[^;]*` 会把结尾的 `}` 一并吃掉，于是
        // 无论有没有 return 都会匹配上 —— 第一版就是这么写的，删掉 return 照样通过。
        const inner = stripComments(retryBody).replace(/\}\s*$/, '').trimEnd()
        if (!/\breturn\b[^;}]*;?$/.test(inner)) {
          problems.push(
            '重试分支的最后一句不是 return：控制流会继续往下掉进判死，重试形同虚设',
          )
        }
      }
    }
  }

  // ③ 真的重新导航
  if (!/src\s*=\s*['"`]about:blank['"`]/.test(arm)) {
    problems.push('重试时没有先回到 about:blank：直接重赋同一 URL 不会触发重新加载')
  }
  if (!/src\s*=\s*lastEntryUrl/.test(arm)) {
    problems.push('重试时没有把 src 赋回真实入口 URL：重试等于什么都没做')
  }

  // ④ 尊重 generation
  if (!/generation\s*!==\s*loadGeneration/.test(arm)) {
    problems.push(
      '重试回调没有校验 generation !== loadGeneration：期间若发生新的 load()，' +
        '会把 iframe 导航到上一代入口上',
    )
  }
  if (!/disposed/.test(arm)) {
    problems.push('重试回调没有检查 disposed：组件卸载后仍会导航已废弃的 iframe')
  }
}

if (load) {
  // 重试计数必须在每次 load() 时归零，否则第二次 load 直接被判死
  if (!/watchdogRetries\s*=\s*0/.test(load)) {
    problems.push('load() 里没有重置 watchdogRetries：重试额度会在第二次加载时已耗尽')
  }
  // lastEntryUrl 必须在 load() 里被赋值，否则重试无 URL 可放
  if (!/lastEntryUrl\s*=/.test(load)) {
    problems.push('load() 里没有给 lastEntryUrl 赋值：重试时会因为没有 URL 而空转')
  }
}

if (problems.length) {
  console.error('扩展 iframe 看门狗守卫未通过：')
  for (const p of problems) console.error(`  ✗ ${p}`)
  process.exit(1)
}
console.log('✓ 扩展 iframe 看门狗：超时会先重试一次，重试尊重 generation，重试计数按 load 归零')
