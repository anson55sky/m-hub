#!/usr/bin/env node
// 生产以 `node --experimental-strip-types` 直接跑 .ts，而该模式是
// **strip-only**：只擦掉类型标注，**不做任何代码变换**。于是有一批 TypeScript
// 语法在本地 `tsc` 下完全正常、在生产却直接 SyntaxError。
//
// ## 为什么要有这个守卫
//
// 这类错误的形状特别恶劣：本地 `npm test`（node --test 走的是另一条加载路径，
// 对这些语法更宽松）、`tsc --noEmit`、`npm run build` **全绿**，
// 只有真正 `npm start` 才炸 —— 也就是只有部署到生产那一刻。
//
// 触发过一次：`d1shape.ts` 写了 `constructor(private db: Db)`（参数属性），
// 本地一切正常，起服务时 `ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX`。
//
// ## 语法黑名单
//
// · 参数属性（`constructor(private x: T)`）—— 需要代码变换才能变成字段
// · `enum` —— 需要变成运行时的对象
// · `namespace` —— 需要变成闭包
// · 装饰器（`@foo`）—— 需要变换
//
// 其它 strip-only 限制（`declare` 的具体规则、类型断言在 `.ts` 里的表现等）
// 由 Node 自己在启动时报错，这里的目标是**在构建期拦住最常踩的几类**。

import { readFileSync, readdirSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const SRC = join(ROOT, 'src')

function walk(dir) {
  const out = []
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) out.push(...walk(p))
    else if (p.endsWith('.ts')) out.push(p)
  }
  return out
}

const problems = []

/** 每条规则：正则 + 说明。注释里的匹配要排除，否则文档会误报。 */
const RULES = [
  {
    name: '参数属性',
    why: 'constructor(private x: T) 需要代码变换才能变成字段；strip-only 直接报 ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX',
    re: /constructor\s*\([^)]*\b(private|public|protected|readonly)\s+\w+\s*:/,
  },
  {
    name: 'enum',
    why: 'enum 需要变成运行时对象；strip-only 不支持',
    re: /^\s*(export\s+)?(const\s+)?enum\s+\w+/m,
  },
  {
    name: 'namespace',
    why: 'namespace 需要变成闭包；strip-only 不支持',
    re: /^\s*(export\s+)?namespace\s+\w+/m,
  },
  {
    name: '装饰器',
    why: '装饰器需要代码变换；strip-only 不支持',
    re: /^\s*@[A-Za-z_$][\w$]*\s*(\(|$)/m,
  },
]

for (const file of walk(SRC)) {
  const raw = readFileSync(file, 'utf8')
  // 粗略剥掉块注释与行注释，避免文档/注释里的示例触发误报。
  // ⚠️ 故意**不**追求精确：宁可漏报也不能因为误报把守卫变成噪声而被忽略。
  const code = raw
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .split('\n')
    .map((l) => l.replace(/^\s*\/\/.*$/, ''))
    .join('\n')

  for (const rule of RULES) {
    const m = rule.re.exec(code)
    if (m) {
      const line = code.slice(0, m.index).split('\n').length
      problems.push(
        `${file.replace(ROOT + '/', '')}:${line} 用了「${rule.name}」—— ${rule.why}`,
      )
    }
  }
}

if (problems.length) {
  console.error('✗ 以下语法在生产的 strip-only 模式下会 SyntaxError：\n')
  for (const p of problems) console.error(`  · ${p}`)
  console.error(
    '\n  本地 tsc / node --test / vite 全都不会发现这个问题，只有 npm start 才炸。\n' +
      '  改法：参数属性 → 显式字段；enum → const 对象 + as const；namespace → 模块。',
  )
  process.exit(1)
}

console.log('✓ 源码可被 --experimental-strip-types 直接执行（无参数属性 / enum / namespace / 装饰器）')
