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

// ---- 最小尺寸的合法性与镜像常量 ⇄ tauri.conf.json ---------------------------，且与窗口最小值逻辑一致 -------
// 外扩带已于 2026-09-29 移除（见 check-window-margin.mjs），故这里不再有
// 「min = 可视区下限 + 2×外扩带」这层换算。剩下的不变量是：
// 最小尺寸必须为正、且不得大于默认尺寸（否则窗口一创建就小于 minSize）。

const win = conf.app?.windows?.[0];
const minW = win?.minWidth;
const minH = win?.minHeight;
if (!Number.isFinite(minW) || !Number.isFinite(minH) || minW <= 0 || minH <= 0) {
  problems.push(`tauri.conf.json 的 minWidth/minHeight 非法: ${minW}×${minH}`);
} else {
  if (minW > win.width) {
    problems.push(
      `minWidth(${minW}) > width(${win.width}) —— 窗口一创建就小于自己的 minSize，\n` +
        `        AppKit 会立刻把它撑到 minWidth，用户看到的初始尺寸与配置不符`,
    );
  }
  if (minH > win.height) {
    problems.push(`minHeight(${minH}) > height(${win.height}) —— 同上`);
  }
  // Rust 侧 window_resize 的镜像常量必须与之一致（tauri 只有 setter 没有 getter，
  // 读不到真实下限，只能写一份；两者不一致时往西/往北拖到底窗口会猛地窜一下）
  for (const [name, key] of [
    ["MIN_INNER_W", "minWidth"],
    ["MIN_INNER_H", "minHeight"],
  ]) {
    const m = rust.match(new RegExp(`pub const ${name}: f64 = ([0-9.]+);`));
    if (!m) {
      problems.push(`window_resize.rs 里找不到 \`pub const ${name}: f64\``);
    } else if (Number(m[1]) !== conf.app?.windows?.[0]?.[key]) {
      problems.push(
        `window_resize.rs 的 ${name} = ${m[1]}，但 tauri.conf.json 的 ${key} = ${conf.app.windows[0][key]}。\n` +
          `        Rust 侧是 AppKit 下限的镜像（tauri 只给了 setter、没有 getter）。`,
      );
    }
  }
}

if (problems.length) {
  console.error(`[window-min-size] 主窗最小尺寸口径不一致 ${problems.length} 条：`);
  for (const p of problems) console.error(`  ✗ ${p}`);
  console.error(
    "\n  改法：改 tauri.conf.json 的 minWidth/minHeight 后，把 window_resize.rs 的\n" +
      "        MIN_INNER_W/MIN_INNER_H 同步（tauri 没有 min size 的 getter，\n" +
      "        Rust 侧只能写一份镜像）。",
  );
  process.exit(1);
}
console.log(
  `[window-min-size] 最小尺寸口径一致（${conf.app?.windows?.[0]?.minWidth}×${conf.app?.windows?.[0]?.minHeight} ⇄ MIN_INNER_W/H）`,
);
