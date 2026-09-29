/**
 * 守卫：每一个让通知窗变可见的函数，都必须自己判免打扰
 *
 * ## 为什么需要它
 *
 * 免打扰是「不弹窗」。但通知窗变可见不只发生在推送那一刻（`show_notice`）——
 * 前端每张卡片入场后都会回调 `notice_layout` 量高，而那条路径末尾**自带 `show()`**。
 *
 * 所以只在 `show_notice` 里加一句判断是不够的：一张**静默卡片**会走
 * 「入库 → 渲染卡片 → 量高 → `notice_layout` → `show()`」，把窗口弹出来，
 * 免打扰当场失效。这个 bug 我写的时候真的差点犯，是读 `notice_layout` 的实现
 * 才发现的 —— 而它没有任何报错、没有崩溃，只是「免打扰看起来不生效」，
 * 用户多半归因成「提醒偶尔不响」而不会去查设置。
 *
 * ## 为什么把 `silent` 做成必填参数，而不是让调用方自觉
 *
 * 最开始 `show_no_activate` / `anchor_default_and_show` 是「无脑显示、不判断」的
 * 纯工具函数，判据在调用方。那样「每个显示函数里都有免打扰判断」这条不变量
 * **在结构上就不成立**，任何检查都只能退化成「调用方是不是都判了」——
 * 而那恰恰是「加一处新调用点就静默失配」的东西。
 * 改成让这两个函数自己收下 `silent` 并自己判之后，不变量变成
 * 「**每一个**让窗口可见的函数里都有判断」，可机械校验，代价只是多穿一个布尔值。
 *
 * ## 判据（两条都要满足）
 *
 *  ① 函数**自己拿到**判定：体内有 `dnd::active()`（顶层决策点），
 *     或签名带 `silent: bool`（底层 helper 收下传进来的判定）。
 *     守卫不会求值，所以只能确认来源 —— 但这一条能挡住 `let silent = false;` 冒充。
 *  ② 按 `#[cfg(...)]` 分段看：每段里若有显示调用，该段内必须自己判 `silent`。
 *
 * ② 是必须的：把 macOS 分支的 `if !silent {` 去掉、只留 Windows 分支的，守卫会报
 * 「已门禁」—— 它从 Windows 那条 `if` 一路找到了 macOS 那个裸 `show()`。
 * 而 macOS 上跑的恰恰是**没门禁的那一支**。`#[cfg]` 两块各写一遍时只改一块就
 * 静默失配，这是移植期最常见的一类漏。
 *
 * ⚠️ 分段正则必须吃下**嵌套**条件（`#[cfg(not(target_os = "windows"))]`）。
 * `\[cfg\(([^)]*)\)\]` 匹配不到它（`[^)]*` 停在 `not(` 的括号上），
 * 于是 macOS 分支被归进上一段、蹭了别人的门禁。变异测试实测：退回那个正则，
 * 守卫立刻对 macOS 分支失明。
 *
 * ## 这个守卫**不**能查出的（已知边界，别指望它）
 *
 *  · **参数值**。`anchor_default_and_show(&win, false)` 与
 *    `anchor_default_and_show(&win, silent)` 在文本上只差一个词，这里看不出区别。
 *    （`anchor_default_and_show` 里那个 `false` 是有意的 —— 它开头已有卫语句。）
 *  · **表达式求值**。门禁变量若来自别的计算而非 `dnd::active()`，①只能确认到调用。
 *  · 真正的**控制流支配关系**。这里做的是「同一 cfg 段里有没有 if 判断」，
 *    而不是逐个显示调用回溯它的所有祖先分支。（曾试过写完整的花括号栈 +
 *    if/else 链继承来做支配分析，但 Rust 的宏与字符串字面量让括号计数不可靠，
 *    误报一片 —— 与其留一个会喊狼来的守卫，不如收窄到能站住的那部分。）
 *
 * 跑法：node scripts/check-notice-dnd-gate.mjs（prebuild 会跑）
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = readFileSync(join(root, "src-tauri/src/notify.rs"), "utf8");

/** 让通知窗变可见的调用 */
const SHOWERS = [
  /\bwin\.show\s*\(\s*\)/,
  /\bshow_no_activate\s*\(/,
  /\banchor_default_and_show\s*\(/,
];

/** 免打扰判断里的变量名 */
const GATE_VARS = ["silent", "dnd", "muted", "is_dnd"];

/** 去掉注释与字符串字面量，只留结构（格式串里的 `{title}` 不是代码块） */
function codeOnly(t) {
  return t
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/^\s*\/\/.*$/gm, "")
    .replace(/r#*"[\s\S]*?"#/g, '""')
    .replace(/r"[^"]*"/g, '""')
    .replace(/"(?:\\.|[^"\\\n])*"/g, '""')
    .replace(/'(?:\\.|[^'\\])'/g, "''");
}

/** 某函数名出现的所有函数体（同名可能有多份，如 #[cfg] 分支定义） */
const _bodies = new Map();
function bodiesOf(fnName) {
  const hit = _bodies.get(fnName);
  if (hit) return hit;
  const out = [];
  const sig = `fn ${fnName}`;
  let i = src.indexOf(sig);
  while (i >= 0) {
    const j = src.indexOf("\n}\n", i);
    out.push(codeOnly(j < 0 ? src.slice(i) : src.slice(i, j + 3)));
    i = src.indexOf(sig, i + 1);
  }
  _bodies.set(fnName, out);
  return out;
}

/** 该行是不是显示调用（函数签名行不算：`fn show_no_activate(…)` 含自己的名字） */
function isShowLine(line) {
  if (/^\s*(pub )?fn \w+\s*\(/.test(line)) return false;
  return SHOWERS.some((re) => re.test(line));
}

/** 该行是不是「以免打扰变量为条件的 if」（顺带提到变量名不算） */
function isGateLine(line) {
  if (!/\bif\b/.test(line)) return false;
  return GATE_VARS.some((v) => new RegExp(`\\b${v}\\b`).test(line));
}

/**
 * 按 `#[cfg(...)]` 归属把函数体切成若干「平台段」。
 *
 * key === null 的段是「与平台无关的部分」。位于**所有 cfg 之前**的那一段
 * 叫「前导段」：函数开头的一行 `if silent { return; }` 对两个平台都生效，
 * 所以它门禁下面所有分支（这一点要单独区分，否则会把前导卫语句漏判成未门禁）。
 */
function platformSegments(lines) {
  const segs = [];
  let cur = { key: null, lines: [] };
  for (const line of lines) {
    const m = line.match(/^\s*#\[cfg\((.+)\)\]\s*$/);
    if (m) {
      if (cur.lines.length) segs.push(cur);
      // codeOnly 已把字符串字面量清成 ""，这里补回可读形式，
      // 免得报错里出现 `not(target_os="")` 这种看不出所以然的 key
      cur = { key: m[1].replace(/\s+/g, "").replace(/""/g, '"…"'), lines: [line] };
      continue;
    }
    cur.lines.push(line);
  }
  if (cur.lines.length) segs.push(cur);
  const firstCfg = segs.findIndex((x) => x.key !== null);
  return {
    segs,
    preambleGates:
      firstCfg < 0
        ? segs.some((x) => x.lines.some(isGateLine))
        : segs.slice(0, firstCfg).some((x) => x.lines.some(isGateLine)),
  };
}

const FN_NAMES = [...src.matchAll(/^(?:pub )?fn (\w+)/gm)].map((m) => m[1]);

const problems = [];
let checked = 0;

for (const fnName of [...new Set(FN_NAMES)].sort()) {
  for (const body of bodiesOf(fnName)) {
    if (!body.split("\n").some(isShowLine)) continue;
    checked++;

    const sigLine = body.split("\n")[0] ?? "";
    const computesIt = /dnd::active\(\)/.test(body);
    const receivesIt = /\bsilent\s*:\s*bool\b/.test(sigLine);
    if (!computesIt && !receivesIt) {
      problems.push(
        `fn ${fnName} 会让通知窗可见，但既没有 dnd::active()、签名里也没有 silent: bool。\n` +
          `      判定必须落在这层：顶层自己调 dnd::active()，底层 helper 收下 silent 参数。\n` +
          `      只靠调用方自觉的话，加一处新调用点就会静默绕过，且没有任何报错。`,
      );
      continue;
    }

    const { segs, preambleGates } = platformSegments(body.split("\n"));
    for (const seg of segs) {
      if (!seg.lines.some(isShowLine)) continue;
      if (preambleGates) continue;
      if (seg.lines.some(isGateLine)) continue;
      problems.push(
        `fn ${fnName} 的 [${seg.key ?? "无 cfg"}] 段里有显示调用，但没有免打扰判断。\n` +
          `      另一平台分支的门禁不能替这一段背书 —— #[cfg] 两块各写一遍时\n` +
          `      只改一块就会静默失配（macOS 上跑的正是 not(windows) 那一支）。\n` +
          `      修法：把显示包进 if !silent { … }。`,
      );
    }
  }
}

if (checked === 0) {
  console.error("[notice-dnd] 一个显示函数都没找到，守卫本身失效（notify.rs 结构变了？）");
  process.exit(1);
}

if (problems.length) {
  console.error(`[notice-dnd] ${problems.length} 处显示路径没被免打扰门禁：`);
  for (const p of problems) console.error(`  ✗ ${p}`);
  console.error(
    "\n  提醒：只在 show_notice 判是不够的。前端每张卡片入场后都会回调\n" +
      "        notice_layout 量高，而那条路径末尾自带 show()。\n",
  );
  process.exit(1);
}
console.log(`[notice-dnd] 所有通知窗显示路径均已门禁免打扰（已查 ${checked} 处显示函数）`);
