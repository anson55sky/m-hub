#!/usr/bin/env node
// 客户端 ↔ 服务端**响应字段名**的对账（只管「原样透传」的那一类命令）。
//
// ## 为什么要这个脚本
//
// `check-api-spec-conformance.mjs` 守的是**路径**。但**响应字段名**没有这一层，
// 两端各自的测试都是绿的：
//
//   · 服务端写 SQL 时显式别名（`ext_id AS extId`），
//   · 客户端在 `src/api/tauri.ts` 里手写一份 TS interface 声明「服务端会给我什么」。
//
// 这份声明是**第二份拷贝，且从来没有被任何东西验证过**——本脚本第一版就是
// 这个样子（见下「两次返工」）。
//
// ## 它造成的实际事故（2026-10-01，用户报上来是三个互不相干的现象）
//
// 客户端声明的 `DevSubmissionRow` / `DevSubmissionList` 与服务端真实返回
// **只有 `id` / `version` / `status` / `total` / `quota` 对得上**，其余全是 `undefined`：
//
//   | 客户端读 | 服务端发 | 界面上表现 |
//   |---|---|---|
//   | `submissions` | `items` | `?? []` → 「账号下还没有提交记录」（明明有 2 条） |
//   | `ext_id` | `extId` | 「其他扩展 ·」标签出现在每一条上 |
//   | `review_note` | `reviewNote` | 「驳回原因」永不显示 |
//   | `created_at` | `createdAt` | 时间显示不出来 |
//   | `gate_report` | （服务端没这列） | 展开的「关卡逐项结论」永远是空面板 |
//
// 而 `blockerFor` 里 `s.ext_id === extId` 恒不成立 → **待审版本再也拦不住**，
// 用户能重复提交、撞服务端唯一索引才报错。三个现象同一根因。
//
// ## 判据
//
// 服务端「真正发出去的字段名」只有两个来源，机械可抽：
//
//   ① SQL 别名：`ext_id AS extId` → 响应里叫 `extId`
//   ② `json({ … })` 字面量的顶层键
//
// **只在下面这个精确集合上断言**：Rust 侧**返回原始 `serde_json::Value`**
// （即原样透传服务端 JSON，字段名一字不改）的命令。
//
// ## 两次返工，都是「守卫红得没有意义」
//
//   ① 第一版断言「所有 `invoke<T>` 的 T 都得对账」→ 17 处假阳性：
//      `AppConfig.theme_mode` / `SystemInfo.cpuUsage` / `ClipboardInfo.paused`
//      打的是**本地 Rust**，与服务端毫无关系。`invoke` 本身**不是**「打服务端」的标志。
//   ② 第二版用「函数体里有 `account::get_json`」判定 —— 太靠后，
//      且把返回自有 serde 结构体的命令也算进来了（那些的字段名由
//      `#[serde(rename_all="camelCase")]` 决定，与服务端词表无关）。
//
// 现在改成三层机械推导，**没有一张人工对照表**（表是会悄悄漂移的东西，
// 而它守的恰恰就是漂移）：
//
//   Rust：命令返回 `Result<Value, String>` 且函数体打服务端 → 命令名集合
//   TS：  `invoke<T>('命令名'` → T
//   守卫：T 是具名 interface 时，其**顶层**字段名必须都在服务端词表里
//
// ## 它明确抓不到什么（别误以为覆盖了）
//
//   · **返回自有 serde 结构体的命令**（`account_status` / `dev_submit` / …）。
//     那些字段名由 Rust 的 `#[serde(...)]` 决定，要对账得连 serde 规则一起解析，
//     本脚本不做。**别把它当「已覆盖」。**
//   · **嵌套对象内部的字段名**。只看 interface 顶层；
//     `{ quota: { drafts_remaining } }` 里的键写错查不出。
//   · **同名的漏检（实测漏了 4/15，必须知道）**。词表是「服务端**任何地方**发出过的
//     名字」，所以只要**别处**恰好有同名字段，就查不出这一处接口上的错。
//     实测：`DevSubmissionRow.created_at` / `review_note` 没被抓到 ——
//     因为 `deviceTokens` 的 JSON 里就有 `created_at`、别的 SELECT 里有裸列
//     `review_note`。HEAD 原版 15 个坏字段里只抓到 11 个。
//     堵这个洞需要「接口 ↔ 路由」的精确映射，那是人工表、会漂移，故不做。
//     **结论：它是「抓出大部分漂移的粗筛」，不是「保证字段名正确」的证明。**
//   · **响应的值/语义**（类型对不对、状态码对不对）。那要靠联调。
//   · **请求体字段名**。请求是蛇形、响应是驼峰，这个不对称本身是对的
//     （约定 47 记的是请求侧），由 api_spec.rs 的测试守着。
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const TAURI_TS = join(ROOT, 'src/api/tauri.ts')
const RUST_SRC = join(ROOT, 'src-tauri/src')
const SERVER_SRC = join(ROOT, 'server/src')

// ------------------------------------------------- ① 服务端「真正发出的字段名」词表

function walk(dir, pred, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) walk(p, pred, out)
    else if (pred(name)) out.push(p)
  }
  return out
}

const serverText = walk(SERVER_SRC, (n) => n.endsWith('.ts') && !/\.(test|spec)\.ts$/.test(n))
  .map((f) => readFileSync(f, 'utf-8'))
  .join('\n')

const vocab = new Set()
for (const m of serverText.matchAll(/\b[a-z_][a-z0-9_]*\s+AS\s+([A-Za-z_][A-Za-z0-9_]*)/gi)) {
  vocab.add(m[1])
}

/** 取一段文本里所有 `ident:` 形式的键（**任意深度**，跳过字符串与注释） */
function allObjectKeys(body) {
  const out = []
  let i = 0
  while (i < body.length) {
    const c = body[i]
    if (c === '/' && body[i + 1] === '/') {
      const nl = body.indexOf('\n', i)
      i = nl < 0 ? body.length : nl
      continue
    }
    if (c === '/' && body[i + 1] === '*') {
      i = (body.indexOf('*/', i) + 1) || body.length
      continue
    }
    if (c === '"' || c === "'" || c === '`') {
      const q = c
      i++
      while (i < body.length && body[i] !== q) {
        if (body[i] === '\\') i++
        i++
      }
      i++
      continue
    }
    if (/[A-Za-z_$]/.test(c) && !/[A-Za-z0-9_$]/.test(body[i - 1] ?? '')) {
      const m = body.slice(i).match(/^([A-Za-z_$][A-Za-z0-9_$]*)\s*:/)
      if (m) {
        out.push(m[1])
        i += m[0].length
        continue
      }
    }
    i++
  }
  return out
}

for (const m of serverText.matchAll(/\bjson\(\s*\{/g)) {
  const start = m.index + m[0].length
  let i = start
  let depth = 1
  while (i < serverText.length && depth > 0) {
    const c = serverText[i]
    if (c === '`') {
      i = serverText.indexOf('`', i + 1)
      if (i < 0) break
    } else if (c === '{') depth++
    else if (c === '}') depth--
    i++
  }
  // ⚠️ 收**任意深度**的键，不是只收顶层：服务端的列表路由普遍写成
  //    `json({ items: rows.map(r => ({ …真实字段… })) })`，
  //    只收顶层就只拿到 `items`，内层的 `device`/`expires_at`/`expired`…
  //    全部收不到 → 客户端照抄真实字段名反被判红（第一版就栽在这，
  //    报出 5 个根本不存在的错）。
  //    代价：词表里会混进嵌套字段名，于是「顶层字段名与某个嵌套字段撞名」
  //    这一种漏检抓不到。这是有意的取舍——宁可少抓一类，
  //    也不要一份天天误报的守卫（误报的守卫会被直接删掉）。
  const region = serverText.slice(start, i - 1)
  for (const k of allObjectKeys(region)) vocab.add(k)
  for (const k of shorthandKeys(region)) vocab.add(k)
}

/**
 * 对象字面量的**简写**属性：`{ items, total, page }`。
 *
 * ⚠️ 别把它并进 `allObjectKeys` 一起判：那会退化成「抓所有标识符」，
 *    `r.token.slice(0, 8)` 里的 `slice`、`Date.now()` 里的 `now` 全会被收进来，
 *    词表膨胀到失去意义。这里只在**紧跟 `{` 或 `,`、且后面紧跟 `,` `}` 或换行**
 * 的位置判一次 —— 那正是对象字面量里简写属性唯一可能出现的位置。
 *
 * 之所以必须有它：`mySubmissions` 的返回是 `json({ items, total, page, … })`，
 * `total`/`page` 全是简写，客户端当然要读它们 —— 漏了就把真字段判成假字段。
 */
function shorthandKeys(body) {
  const out = []
  for (let i = 0; i < body.length; i++) {
    if (body[i] !== '{' && body[i] !== ',') continue
    let k = i + 1
    while (k < body.length && /\s/.test(body[k])) k++
    const m = body.slice(k).match(/^([A-Za-z_$][A-Za-z0-9_$]*)(?=\s*[,}\n])/)
    if (m) out.push(m[1])
  }
  return out
}

/**
 * `SELECT a, b AS c, d FROM …` 里**没有别名**的裸列名也原样出现在响应里
 * （`json(row)` 直接把行序列化，`rows.results` 同理）。
 * `version` / `status` / `id` 都属于这一类 —— 它们没有 `AS`，
 * 只靠别名规则收不到，会被误判成「服务端不发」。
 */
for (const m of serverText.matchAll(/\bSELECT\s+([\s\S]*?)\s+FROM\s/gi)) {
  for (const col of m[1].split(',')) {
    const c = col.trim()
    if (!c) continue
    const parts = c.split(/\s+/)
    const last = parts[parts.length - 1].replace(/[^\w]/g, '')
    if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(last)) vocab.add(last)
  }
}

// ------------------------------------------------- ② Rust：原样透传服务端 JSON 的命令

const rustText = walk(RUST_SRC, (n) => n.endsWith('.rs'))
  .map((f) => readFileSync(f, 'utf-8'))
  .join('\n')

const passthrough = new Set()
for (const m of rustText.matchAll(
  /pub(?:\(crate\))?\s+async\s+fn\s+([a-z0-9_]+)\s*\([^)]*\)\s*->\s*Result<\s*Value\s*,/g,
)) {
  const body = rustText.slice(m.index, m.index + 2600)
  if (/account::(get_json|post_json|get_text_json)|api_spec::/.test(body)) passthrough.add(m[1])
}

// ------------------------------------------------- ③ TS：命令 → 返回类型 → 顶层字段

const ts = readFileSync(TAURI_TS, 'utf-8')

/**
 * 按**深度 0** 把一段文本切成成员。
 *
 * ⚠️ 第一版只在 `;` 处断成员，而本文件的 interface 是**换行分隔、不带分号**的
 * —— 于是每个 interface 只被抽出**第一个**字段，第二个往后的全漏。
 * 表现是「守卫只报 2 处，而坏字段有 8 个」，而且报出来的偏偏是每段的开头。
 * 一个只看得见首字段的守卫比没有更糟：它让人以为覆盖了。
 *
 * 成员边界 = 深度 0 的 `;`，或深度 0 的换行**且**下一段确实像个新声明的开始
 * （标识符 / `?` / `[` / `{` / 引号）。前者管分号风格，后者管换行风格。
 * 字符串与注释在判断前先跳过，免得 `"a; b"` 里的分号把成员劈开。
 */
function splitMembers(body) {
  const members = []
  let d = 0
  let seg = ''
  let i = 0
  const push = () => {
    if (seg.trim()) members.push(seg)
    seg = ''
  }
  const nextMeaningful = (from) => {
    let k = from
    while (k < body.length) {
      const c = body[k]
      if (c === '/' && body[k + 1] === '/') {
        k = body.indexOf('\n', k)
        if (k < 0) return ''
        continue
      }
      if (c === '/' && body[k + 1] === '*') {
        k = body.indexOf('*/', k) + 1
        continue
      }
      if (!/\s/.test(c)) return c
      k++
    }
    return ''
  }
  while (i < body.length) {
    const c = body[i]
    if (c === '"' || c === "'" || c === '`') {
      const q = c
      seg += c
      i++
      while (i < body.length && body[i] !== q) {
        if (body[i] === '\\') seg += body[i]
        seg += body[i]
        i++
      }
      seg += body[i] ?? ''
      i++
      continue
    }
    if (c === '{' || c === '(' || c === '[' || c === '<') d++
    else if (c === '}' || c === ')' || c === ']' || c === '>') d--
    seg += c
    i++
    if (d === 0 && c === ';') push()
    else if (d === 0 && c === '\n' && seg.trim()) {
      const nxt = nextMeaningful(i)
      if (/[A-Za-z_$?[]/.test(nxt)) push()
    }
  }
  push()
  return members
}

/** `export interface X { … }` → 名字 → { fields, decl, line } */
function interfaces(src) {
  const out = new Map()
  for (const m of src.matchAll(
    /export\s+interface\s+([A-Za-z0-9_]+)\s*(?:extends\s+[A-Za-z0-9_, <>]+)?\s*\{/g,
  )) {
    const start = m.index + m[0].length
    let i = start
    let depth = 1
    while (i < src.length && depth > 0) {
      if (src[i] === '{') depth++
      else if (src[i] === '}') depth--
      i++
    }
    const body = src.slice(start, i - 1)
    const fields = []
    for (const member of splitMembers(body)) {
      // 去掉行首注释再取字段名（注释里出现的标识符不是字段）
      const cleaned = member
        .split('\n')
        .filter((l) => !/^\s*\/\//.test(l))
        .join('\n')
      const fm = cleaned.match(/([A-Za-z_$][A-Za-z0-9_$]*)\s*\??\s*:/)
      if (!fm) continue
      const at = body.indexOf(member)
      fields.push({
        name: fm[1],
        line: src.slice(0, start + Math.max(0, at)).split('\n').length,
      })
    }
    out.set(m[1], { fields, decl: body, line: src.slice(0, m.index).split('\n').length })
  }
  return out
}

const ifaces = interfaces(ts)

/** 命令名 → invoke 的类型实参 */
const retTypeOf = new Map()
for (const m of ts.matchAll(/invoke<([\s\S]*?)>\(\s*'([a-z0-9_]+)'/g)) {
  const type = m[1].replace(/\s+/g, ' ')
  retTypeOf.set(m[2], type)
}

/**
 * 从类型表达式出发，取出**可达的 interface 闭包**。
 *
 * ⚠️ 必须传递。第一版只查 `invoke<T>` 的 T 本身，于是
 * `DevSubmissionList { items: DevSubmissionRow[] }` 里那层**数组**把
 * `DevSubmissionRow` 挡在外面 —— 8 个坏字段只抓到 2 个（`submissions` 与
 * `market`），`ext_id`/`review_note`/`created_at`/`size`/`runtime`/… 全漏。
 * 半盲的守卫比没有更糟：它让人以为这一类已经覆盖。
 *
 * 沿字段类型递归：`Record<string, X>` 的 `string` 不算 interface 名，
 * 内置泛型（`Array`/`Promise`/`Record`…）本来就不在 `ifaces` 里，
 * 所以只认「出现在类型表达式里、且确实是本文件定义的 interface」。
 */
function reachable(typeExpr) {
  const seen = new Set()
  const queue = [typeExpr]
  while (queue.length) {
    const t = queue.pop()
    for (const id of t.match(/[A-Za-z_][A-Za-z0-9_]*/g) ?? []) {
      if (seen.has(id)) continue
      const iface = ifaces.get(id)
      if (!iface) continue
      seen.add(id)
      queue.push(iface.decl)
    }
  }
  return seen
}

// ------------------------------------------------- 对账

const problems = []
const checked = new Set()

for (const cmd of passthrough) {
  const type = retTypeOf.get(cmd)
  if (!type) {
    problems.push(
      `Rust 有透传命令 '${cmd}'，但 src/api/tauri.ts 里没有任何 invoke 调它 —— 命令封装漏了。`,
    )
    continue
  }
  for (const name of reachable(type)) {
    checked.add(name)
    const iface = ifaces.get(name)
    for (const f of iface.fields) {
      if (!vocab.has(f.name)) {
        problems.push(
          `${name}.${f.name}（tauri.ts:${f.line}，命令 ${cmd}）—— 服务端没有任何地方发出 '${f.name}'。\n` +
            `    服务端响应字段名只有两个来源：SQL 的 \`AS\` 别名、\`json({…})\` 的顶层键。\n` +
            `    若这是新字段：先在服务端发出它。若这是改过的字段：把客户端改回服务端真名。`,
        )
      }
    }
  }
}

if (problems.length) {
  console.error(`\n响应字段名对账失败：${problems.length} 处客户端声明的字段服务端根本不发\n`)
  for (const p of problems) console.error(`  ✗ ${p}\n`)
  console.error(
    `  症状不会是「报错」，而是界面说了一句与事实相反的话：\n` +
      `  读不到的字段被 \`?? []\` 兜成空列表 →「没有提交记录」；\n` +
      `  拦截条件 \`s.extId === extId\` 恒不成立 → 待审版本再也拦不住。\n` +
      `  真实事故：2026-10-01 审核完 APP 不显示审核状态 / 重新进入要求重新发布 / 提示版本号非法。\n`,
  )
  process.exit(1)
}
console.log(
  `  ✓ 响应字段名对账：${checked.size} 个响应类型 / ${passthrough.size} 个 Rust 侧候选 / ` +
    `${vocab.size} 个服务端字段名，无漂移`,
)