// wrangler 配置自检。
//
// ## 为什么需要它
//
// 这个项目里 **wrangler 配置的错误全部是静默的或延后到部署才炸**的，
// 本轮实打实踩了三次：
//
// ① **D1 的 `database_id` 被截断**（28 字符而非 36）—— 我读命令输出时被截了尾，
//    直接抄进配置。症状是部署时 `Error 8000022: Invalid database UUID`，
//    前面所有测试（含 tsc 与全部单测）都绿。
// ② **`tsconfig.include` 只有 `src/**/*.ts`** —— `functions/` 完全不被类型检查，
//    症状是「`tsc --noEmit` 全绿」与「wrangler 报 Could not resolve」同时发生。
// ③ **Pages 与 Workers 的键互斥** —— `main` / `observability` 出现在 Pages 配置里
//    会直接报错，而且是部署时才报。
//
// 这三类都不是「测试忘了写」，而是**测试照不到的地方**：
// 配置文件的正确性不体现在任何断言里。故单列一个守卫。
//
// 用法：node scripts/check-wrangler-config.mjs

import { readFileSync, existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const SERVER_ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const PAGES = join(SERVER_ROOT, 'wrangler.jsonc')
const WORKER = join(SERVER_ROOT, 'wrangler.worker.jsonc')

const problems = []
const fail = (m) => problems.push(m)

/** 去掉 `//` 注释后按 JSON 解析（两份配置都是 JSONC） */
function readJsonc(p) {
  const raw = readFileSync(p, 'utf8')
  return JSON.parse(raw.replace(/^\s*\/\/.*$/gm, ''))
}

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

// ---------------------------------------------------------------- 1. Pages 配置
if (!existsSync(PAGES)) {
  fail('缺 wrangler.jsonc（Pages 配置）')
} else {
  const cfg = readJsonc(PAGES)

  // Pages 与 Workers 是**互斥**的两种产物，键混用会直接拒绝部署。
  // 这些是 Pages 明确不支持的键（实测：报 "Configuration file cannot contain both"）。
  for (const k of ['main', 'observability', 'workers_dev']) {
    if (k in cfg) {
      fail(
        `wrangler.jsonc（Pages）里不该有 \`${k}\`。\n` +
          `    Pages 与 Workers 是互斥产物，混用会直接拒绝部署。\n` +
          `    \`${k}\` 属于 Workers 那份（wrangler.worker.jsonc）。`,
      )
    }
  }

  if (!cfg.pages_build_output_dir) {
    fail(
      'wrangler.jsonc（Pages）缺 `pages_build_output_dir`。\n' +
        '    没有它 wrangler 不知道静态资源在哪 —— 而 Pages 是静态优先的那一方。',
    )
  } else {
    const dir = join(SERVER_ROOT, cfg.pages_build_output_dir)
    if (!existsSync(dir)) {
      fail(`pages_build_output_dir 指向的目录不存在：${cfg.pages_build_output_dir}`)
    }
  }
}

// ---------------------------------------------------------------- 2. Functions 入口目录
const FUNCTIONS = join(SERVER_ROOT, 'functions')
if (!existsSync(FUNCTIONS)) {
  fail('缺 functions/ 目录 —— Pages Functions 的入口。\n    没有它，/me 与 /v1/* 会 404（/api/v1/* 则由静态资产兜底，看起来「正常」）。')
}

// ---------------------------------------------------------------- 3. D1 绑定（两份配置都要对）
for (const [name, p] of [['Pages', PAGES], ['Workers', WORKER]]) {
  if (!existsSync(p)) continue
  const cfg = readJsonc(p)
  const dbs = cfg.d1_databases || []
  if (dbs.length === 0) {
    fail(`wrangler${name === 'Workers' ? '.worker' : ''}.jsonc 没有 d1_databases —— 接口会一律 401/空数据。`)
    continue
  }
  for (const db of dbs) {
    if (!db.binding) fail(`${name}：d1_databases 缺 binding`)
    if (!db.database_id || !UUID_RE.test(db.database_id)) {
      // ⚠️ 这条正是本轮踩的坑：UUID 被抄成 28 字符。
      //    症状是部署时 Error 8000022，而 tsc 与全部单测都绿。
      fail(
        `${name}：database_id 不是合法 UUID（长度 ${(db.database_id || '').length}）：${db.database_id}\n` +
          `    必须是 36 字符的 8-4-4-4-12。用 \`wrangler d1 list --json\` 核对，别从终端输出手抄。`,
      )
    }
    if (!db.database_name) fail(`${name}：d1_databases 缺 database_name`)
  }
}

// ---------------------------------------------------------------- 4. 机密不得进 vars
// `vars` 随函数代码**公开下发**，不是环境机密。
const SECRETS = ['GITHUB_CLIENT_ID', 'RESEND_API_KEY', 'OPENAI_API_KEY', 'INVITE_CODE']
for (const [name, p] of [['Pages', PAGES], ['Workers', WORKER]]) {
  if (!existsSync(p)) continue
  const cfg = readJsonc(p)
  for (const k of Object.keys(cfg.vars || {})) {
    if (SECRETS.includes(k)) {
      fail(
        `${name}：\`vars.${k}\` 是机密，不能写进配置。\n` +
          `    vars 随函数代码公开下发（不是环境机密）。用 \`wrangler pages secret put ${k}\`。`,
      )
    }
  }
}

// ---------------------------------------------------------------- 输出
if (problems.length) {
  console.error('\n✗ wrangler 配置自检未通过：\n')
  for (const p of problems) console.error(`  · ${p}\n`)
  process.exit(1)
}
console.log('  ✓ wrangler 配置自检通过：Pages/Workers 键互斥、输出目录与 functions/ 就位、D1 UUID 合法、机密未入 vars')
