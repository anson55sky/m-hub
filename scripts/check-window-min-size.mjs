/**
 * 校验「主窗最小尺寸」的 Rust / tauri.conf.json 两处口径一致
 *
 * ## 为什么这两个数必须锁
 *
 * `tauri.conf.json` 的 `minWidth`/`minHeight` 是**逻辑**像素，tao 建窗时用它调
 * `NSWindow.setMinSize`。`window_resize.rs` 因为拿不到 min size 的 getter（tauri
 * 只给了 `min_inner_size` / `set_size_constraints` 两个 setter），只能在 Rust 侧
 * 写一份镜像常量 `MIN_INNER_W` / `MIN_INNER_H`，供拖拽时夹取用。
 *
 * 两者不一致时的症状很难自查：
 *  - Rust 侧夹得太松 → 算出的宽度小于 AppKit 的下限，AppKit 在**之后**把它撑回去，
 *    而原点已经按那个永远不会被采用的宽度算过了 → 往西/往北拖到底时
 *    **窗口猛地往右下窜一下**。
 *  - Rust 侧夹得太紧 → 窗口再也拖不到真实允许的最小尺寸。
 *
 * ## 为什么要机械校验
 *
 * 两处跨 JSON 与 Rust，**没有任何编译器或类型系统会把它们关联起来**。
 * 漏改一处的症状不是报错，而是「拖到底窗口弹一下」这种难以归因到尺寸配置的
 * 视觉故障 —— 排查方向会跑到 CSS 和拖拽逻辑上，而真正错的是两个数字对不上。
 *
 * 另外本守卫顺带校验一件同源的事：`minWidth/minHeight` 必须恰好等于
 * 「可视区最小值 + 2 × WINDOW_SHADOW_MARGIN」。主窗外侧有一圈 32px 的透明外扩带
 * 用于自绘阴影（见 AGENTS.md 约定 69），而 tauri.conf.json 里写的是 **inner**
 * 尺寸（含外扩带）。这个关系一旦被打破，窗口创建后的可视区就不是 1000×700，
 * 而小屏判断（lib.rs 参照 `WindowState::default`）的前提也随之失效。
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");

const problems = [];

const conf = JSON.parse(read("src-tauri/tauri.conf.json"));
const rust = read("src-tauri/src/window_resize.rs");

// ---- ① Rust 镜像常量 ⇄ tauri.conf.json -------------------------------------

const numConst = (name) => {
  const m = rust.match(new RegExp(`pub const ${name}: f64 = ([0-9.]+);`));
  if (!m) {
    problems.push(`window_resize.rs 里找不到 \`pub const ${name}: f64\``);
    return null;
  }
  return Number(m[1]);
};

for (const [name, key] of [
  ["MIN_INNER_W", "minWidth"],
  ["MIN_INNER_H", "minHeight"],
]) {
  const rustVal = numConst(name);
  const confVal = conf.app?.windows?.[0]?.[key];
  if (rustVal === null || confVal === undefined) continue;
  if (rustVal !== confVal) {
    problems.push(
      `window_resize.rs 的 ${name} = ${rustVal}，但 tauri.conf.json 的 ${key} = ${confVal}。\n` +
        `        Rust 侧是 AppKit 下限的镜像（tauri 只给了 setter、没有 getter，\n` +
        `        读不到真实值，只能写一份）。两者不一致时，拖拽夹取用的下限与\n` +
        `        AppKit 实际生效的下限不同：往西/往北拖到底窗口会猛地窜一下。`,
    );
  }
}

// ---- ② minWidth/minHeight = 可视区最小值 + 2 × 外扩带 -----------------------
// 数值直接引用约定 69 的四个口径点，避免这里再写第五份拷贝。

const style = read("src/style.css");
const libRs = read("src-tauri/src/lib.rs");

const cssMargin = Number(style.match(/--window-shadow-margin:\s*([0-9.]+)px/)?.[1]);
const rustMargin = Number(libRs.match(/WINDOW_SHADOW_MARGIN: f64 = ([0-9.]+)/)?.[1]);

if (!Number.isFinite(cssMargin) || !Number.isFinite(rustMargin)) {
  problems.push("取不到 --window-shadow-margin / WINDOW_SHADOW_MARGIN");
} else if (cssMargin !== rustMargin) {
  problems.push(
    `--window-shadow-margin(${cssMargin}) 与 WINDOW_SHADOW_MARGIN(${rustMargin}) 不一致`,
  );
} else {
  const m = cssMargin;
  // 可视区最小值 1000×700（config.rs::WindowState 的下限约定）
  const expectW = 1000 + 2 * m;
  const expectH = 700 + 2 * m;
  const minW = conf.app?.windows?.[0]?.minWidth;
  const minH = conf.app?.windows?.[0]?.minHeight;
  if (minW !== expectW) {
    problems.push(`tauri.conf.json 的 minWidth = ${minW}，应为 ${expectW}（= 1000 + 2×${m}）`);
  }
  if (minH !== expectH) {
    problems.push(`tauri.conf.json 的 minHeight = ${minH}，应为 ${expectH}（= 700 + 2×${m}）`);
  }
}

if (problems.length) {
  console.error(`[window-min-size] 主窗最小尺寸口径不一致 ${problems.length} 条：`);
  for (const p of problems) console.error(`  ✗ ${p}`);
  console.error(
    "\n  改法：改 tauri.conf.json 的 minWidth/minHeight 后，把 window_resize.rs 的\n" +
      "        MIN_INNER_W/MIN_INNER_H 同步；改最小可视区则还要同步 config.rs。\n" +
      "        推导关系见 AGENTS.md 约定 69（外扩带 32px，tauri.conf 记的是 inner 尺寸）。",
  );
  process.exit(1);
}
console.log(
  `[window-min-size] 最小尺寸口径一致（${conf.app?.windows?.[0]?.minWidth}×${conf.app?.windows?.[0]?.minHeight} ⇄ MIN_INNER_W/H，外扩带 ${cssMargin}px 已计入）`,
);
