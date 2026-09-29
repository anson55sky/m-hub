/**
 * 校验「全出血瞬态层」与主窗口同款圆角（见 AGENTS.md 约定 69）
 *
 * ## 为什么要检查
 *
 * 主窗口本体是**透明**的（tauri.conf.json 的 `transparent: true`），圆角靠
 * `.app-shell` 的 `border-radius` 裁出来。于是窗口四角是「没有像素」的，
 * 能透出后面的桌面 —— 这正是圆角要的效果。
 *
 * 但**弹到 body 上的全出血遮罩**（`position: fixed; inset: 0`）是方形：
 * 它不经过 `.app-shell`，也就没有被 `overflow: hidden` + `border-radius` 裁到。
 * 一旦打开弹窗 / 灯箱 / 拖拽遮罩，那一帧就会用方形遮罩把圆角盖回去，
 * 视觉上是「窗口方了一下又圆回来」，非常突兀。
 *
 * 而且这类 bug **编译期完全看不出来、类型检查也全绿**，只能实机看见。
 * 所以把它固化成构建期检查。
 *
 * ## 规则
 *
 * `position: fixed` + `inset: 0`（铺满整个窗口）的规则，必须同时声明
 * `border-radius`。小而定位的下拉 / 气泡 / tooltip 不受此限——
 * 它们本来就内缩，永远碰不到窗口边缘。
 *
 * 浮窗（剪贴板 / 悬浮球 / 便签）里的同类规则一并要求加圆角：
 * 那里加了看不出差别，但**不检查就会让这个约定变得有例外**，
 * 例外一多，规则就等于不存在。
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/** 递归收集源码文件 */
function collect(dir, out = []) {
  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name === "dist" || name === ".git") continue;
    const p = join(dir, name);
    if (statSync(p).isDirectory()) collect(p, out);
    else if (/\.(vue|css)$/.test(name)) out.push(p);
  }
  return out
}

/** 把 `<style>` 块与整份 css 一视同仁地扫 */
function styleBlocks(src, file) {
  const blocks = [];
  const global = src.replace(/<style[^>]*>[\s\S]*?<\/style>/g, "");
  if (global.trim()) blocks.push({ text: global, where: file });
  const re = /<style[^>]*>([\s\S]*?)<\/style>/g;
  let m;
  while ((m = re.exec(src))) blocks.push({ text: m[1], where: `${file} <style>` });
  return blocks
}

/** 拆成 `选择器 { 声明 }`，够用即可（不追求完整 CSS 解析） */
function rules(css) {
  const out = [];
  const re = /([^{}]+)\{([^{}]*)\}/g;
  let m;
  while ((m = re.exec(css))) {
    const sel = m[1].trim();
    if (!sel || sel.startsWith("@") || sel === "from" || sel === "to") continue;
    out.push({ sel, body: m[2], at: m.index });
  }
  return out
}

const problems = [];
const files = collect(join(root, "src"));

for (const file of files) {
  const src = readFileSync(file, "utf8");
  for (const { text, where } of styleBlocks(src, relative(root, file))) {
    for (const r of rules(text)) {
      if (!/position\s*:\s*fixed/.test(r.body)) continue;
      // 铺满「可视区」：两种写法都要认
      //   ① inset: 0                                —— 铺满整个窗口
      //   ② inset: var(--window-shadow-margin, 0px)  —— 铺满去掉外扩带后的可视区
      //      （约定 69：主窗外侧有 32px 透明带，遮罩若铺满整窗，圆角会落在
      //        窗口角而不是内容角 → 四个角各冒出一块方角。2026-09-29 实测）
      // ⚠️ ② 必须一并纳入：只认 ① 的话，改用 ② 的规则会**静默失去检查**。
      // 「守卫不报错」与「没问题」是两回事，这种沉默正是本守卫存在的理由。
      const fullWindow = /inset\s*:\s*0\s*[;}]/.test(r.body);
      const fullContent = /inset\s*:\s*var\(\s*--window-shadow-margin/.test(r.body);
      const fourSides =
        /top\s*:\s*0/.test(r.body) &&
        /right\s*:\s*0/.test(r.body) &&
        /bottom\s*:\s*0/.test(r.body) &&
        /left\s*:\s*0/.test(r.body);
      if (!fullWindow && !fullContent && !fourSides) continue;
      if (/border-radius\s*:/.test(r.body)) continue;
      // 透明且纯事件拦截的层画不出方角（如 .pop-mask），不算问题
      if (/background\s*:\s*transparent/.test(r.body) && !/backdrop-filter/.test(r.body)) {
        continue;
      }
      problems.push(`${where}  ${r.sel.replace(/\s+/g, " ")}`);
    }
  }
}

if (problems.length) {
  console.error(`[rounded-window] 全出血瞬态层缺圆角 ${problems.length} 条：`);
  for (const p of problems) console.error(`  ✗ ${p}`);
  console.error(
    "  规则：position:fixed + inset:0（或 inset:var(--window-shadow-margin,0px)）的层\n" +
      "        必须同时声明 border-radius（用 var(--window-radius)），\n" +
      "        否则弹窗/灯箱打开的那一帧会用方形遮罩把主窗口的圆角盖回去。\n" +
      "        小而定位的下拉、气泡、tooltip 不受此限（它们碰不到窗口边缘）。",
  );
  process.exit(1);
}
console.log(`[rounded-window] 全出血层圆角口径正常（已扫 ${files.length} 个文件）`);
