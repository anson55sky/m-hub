/**
 * 守卫：主窗**不允许再有外扩带**，且窗口 inner 尺寸 == 可视区尺寸
 *
 * ## 历史（为什么这个守卫现在「反向」了）
 *
 * 2026-09-29 之前主窗有一圈 32px 透明外扩带（`--window-shadow-margin` /
 * `WINDOW_SHADOW_MARGIN`），存在的唯一理由是**给自绘阴影留位置**：
 * AppKit 对 `transparent: true` 的窗口不画系统阴影（显式 `setHasShadow(true)`
 * 也无效，实机取色证实），而 CSS 阴影只能画在窗口**以内**，不外扩就无处可画。
 *
 * 代价是那圈带子**是透明的** —— 桌面壁纸与图标会从窗口四周直接透进来，
 * 在窗口外面形成一圈明显的「玻璃框」。用户实测：在彩色壁纸上尤其刺眼。
 *
 * 于是带子被移除，取舍变成一句话：
 *   **「有阴影」与「不漏桌面」在透明窗下二选一。**
 * 透明窗拿不到系统阴影，所以选了后者：内容铺满整窗、圆角保留、**没有阴影**。
 * 若哪天要阴影，唯一干净的路是把窗口改成 opaque 并接受直角 —— 那是设计取舍。
 *
 * ## 为什么移除之后还需要守卫
 *
 * 因为「顺手加回一圈带子」是很自然的一次改动（想让窗口有阴影时几乎必然想到它），
 * 而它的症状又是**静默**的：窗口与内容差一圈，肉眼像「边距没对齐」，
 * 排查方向会跑到 CSS 上，而真正错的是尺寸换算。
 * 本守卫把「带子不许回来」以及「inner == visible」这两条钉死：
 *   ① `src/style.css` 不得再声明 `--window-shadow-margin`
 *   ② `src-tauri/src/lib.rs` 不得再有 `WINDOW_SHADOW_MARGIN`
 *   ③ `tauri.conf.json` 的 width/height/minWidth/minHeight 必须**等于**
 *      `config.rs::WindowState::default`（不再有 2×M 的加法）
 *   ④ `index.html` 启动欢迎页的 inset 必须是 0、圆角须与 `--window-radius` 一致
 *
 * ③ 那条是最有价值的：它把「窗口尺寸」与「落盘尺寸」重新绑在同一个数字上。
 * 两者一旦分叉（比如有人只改了 conf 的 width），用户拖好的窗口尺寸会在
 * 下次启动时被改写，且不会有任何报错。
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");

const problems = [];
const css = read("src/style.css");
const libRs = read("src-tauri/src/lib.rs");
const confRs = read("src-tauri/src/config.rs");
const html = read("index.html");
const conf = JSON.parse(read("src-tauri/tauri.conf.json"));
const win = conf.app?.windows?.[0];

// ---- ①② 带子不许回来 ---------------------------------------------------

if (/--window-shadow-margin\s*:/.test(css)) {
  problems.push(
    "src/style.css 又出现了 `--window-shadow-margin` 声明。\n" +
      "        外扩带已移除：那圈带子是透明的，会让桌面从窗口四周直接透进来，\n" +
      "        形成一圈「玻璃框」。要阴影请把窗口改成 opaque 并接受直角。",
  );
}
if (/WINDOW_SHADOW_MARGIN/.test(libRs)) {
  problems.push(
    "src-tauri/src/lib.rs 又出现了 `WINDOW_SHADOW_MARGIN`。\n" +
      "        窗口 inner 尺寸现在**就是**可视区尺寸，任何 ±2×M 的换算都是错的。",
  );
}

// ---- ③ conf 的 inner 尺寸必须等于 WindowState::default -------------------

const defaultW = Number(
  confRs.match(/pub\s+width:\s*([0-9.]+)/)?.[1] ??
    confRs.match(/fn default\(\)[^{]*\{[\s\S]*?width:\s*([0-9.]+)/)?.[1],
);
const defaultH = Number(
  confRs.match(/pub\s+height:\s*([0-9.]+)/)?.[1] ??
    confRs.match(/fn default\(\)[^{]*\{[\s\S]*?height:\s*([0-9.]+)/)?.[1],
);

if (!Number.isFinite(defaultW) || !Number.isFinite(defaultH)) {
  problems.push("config.rs 里取不到 WindowState::default 的 width/height");
} else {
  for (const [key, expected] of [
    ["width", defaultW],
    ["height", defaultH],
  ]) {
    if (win?.[key] !== expected) {
      problems.push(
        `tauri.conf.json 的 ${key} = ${win?.[key]}，应为 ${expected}` +
          `（= WindowState::default 的可视区尺寸）。\n` +
          `        两者必须相等：外扩带移除后窗口 inner 尺寸**就是**可视区尺寸。`,
      );
    }
  }
  // 最小尺寸同理：minWidth/minHeight 是 inner 尺寸，必须等于「可视区下限」
  const minW = Number(confRs.match(/min_width:\s*([0-9.]+)/)?.[1]);
  const minH = Number(confRs.match(/min_height:\s*([0-9.]+)/)?.[1]);
  if (Number.isFinite(minW) && win?.minWidth !== minW) {
    problems.push(`tauri.conf.json 的 minWidth = ${win?.minWidth}，应为 ${minW}`);
  }
  if (Number.isFinite(minH) && win?.minHeight !== minH) {
    problems.push(`tauri.conf.json 的 minHeight = ${win?.minHeight}，应为 ${minH}`);
  }
}

// ---- ④ 启动欢迎页：inset 0 + 圆角与 --window-radius 一致 -------------------
// index.html 的 <style> 早于 style.css 加载，CSS 变量那时还不存在，
// 所以这里只能硬编码 —— 也因此必须机械校验它没跟主样式跑偏。

// ⚠️ 单位必须**可选**：`inset: 0` 是合法 CSS 且不带 px。
// 写成 `\d+px` 会让这条正则整条匹配不上 —— 守卫于是「没发现问题」，
// 而它恰恰是最该拦的那种（带子加回来时值就是 0 的反面）。
// 这不是第一次在这个守卫上栽：早先的 box-shadow 版本也有同样的单位假设。
const splashInset = html.match(/#boot-splash\s*\{[\s\S]*?\binset:\s*(\d+(?:\.\d+)?)\s*(?:px)?\s*;/);
if (!splashInset) {
  problems.push("index.html 找不到 `#boot-splash` 的 inset 声明");
} else if (Number(splashInset[1]) !== 0) {
  problems.push(
    `index.html #boot-splash 的 inset=${splashInset[1]}px，应为 0px。\n` +
      `        外扩带已移除，欢迎页与主界面都铺满整窗；不内缩的话启动时会看到\n` +
      `        一次「圆角方块缩进去 Npx」的形状跳变。`,
  );
}

const cssRadius = Number(css.match(/--window-radius:\s*([0-9.]+)px/)?.[1]);
const splashRadius = Number(
  html.match(/#boot-splash::before\s*\{[\s\S]*?border-radius:\s*([0-9.]+)\s*(?:px)?\s*;/)?.[1],
);
if (!Number.isFinite(cssRadius)) {
  problems.push("src/style.css 找不到 `--window-radius`");
} else if (splashRadius !== cssRadius) {
  problems.push(
    `index.html 欢迎页圆角=${splashRadius}px，应为 ${cssRadius}px（= --window-radius）`,
  );
}

if (problems.length) {
  console.error(`[window-margin] 外扩带已移除，但发现 ${problems.length} 处不一致：`);
  for (const p of problems) console.error(`  ✗ ${p}`);
  console.error(
    "\n  背景见本文件顶部注释：外扩带是为了给自绘阴影留位置，但它是透明的，\n" +
      "  会让桌面从窗口四周透进来形成「玻璃框」。有阴影 / 不漏桌面二选一。",
  );
  process.exit(1);
}
console.log(
  `[window-margin] 无外扩带（inner == 可视区 ${defaultW}×${defaultH}，欢迎页 inset 0 / 圆角 ${cssRadius}px）`,
);
