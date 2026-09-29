/**
 * 校验「窗口阴影外扩带」的四处口径一致（见 AGENTS.md 约定 69）
 *
 * ## 这个数为什么牵动四处
 *
 * 主窗为自绘阴影，窗口要比可视区大一圈 `--window-shadow-margin`（AppKit 不给透明
 * 窗口画系统阴影，而 CSS 阴影只能画在窗口以内）。这同一个数出现在四个地方：
 *
 *   ① `src/style.css`          `--window-shadow-margin: 32px`   → .app-shell 的 margin
 *   ② `src-tauri/src/lib.rs`   `WINDOW_SHADOW_MARGIN: f64 = 32.0` → 尺寸换算 inner = 可视区 + 2×它
 *   ③ `tauri.conf.json`        width/height/minWidth/minHeight 是 **inner** 尺寸，各 + 2×它
 *   ④ `config.rs`              `WindowState::default` = 1400×900（可视区），lib.rs 小屏判断**引用**它
 *
 * ## 为什么必须机械校验
 *
 * ① ② ③ 跨两种语言、三种文件类型，**没有任何编译器或类型系统会把它们关联起来**。
 * 漏改一处的症状不是报错，是「窗口和内容差了一圈」——看起来像边距没对齐，
 * 排查方向会跑到 CSS 上，而真正错的是另外几个文件里的数字。
 *
 * ④ 与 ③ 的关系也要锁：`tauri.conf.json` 的初始 inner 尺寸应当正好等于
 * `WindowState::default` 换算后的值。两者不一致时，窗口创建后会被
 * `restore_window_state` 立刻改写（该窗 `visible: false`，看不出），
 * 但会留下一段「尺寸先错后对」的时间窗，且排查时极易误判为闪烁 bug。
 *
 * ## 有一处是「消灭」而不是「校验」
 *
 * ④ 在 lib.rs 那边必须写成 `config::WindowState::default()` 引用、不是字面量。
 * 那里曾硬编码 1400/900 —— 同一组数字的**第五份拷贝**，本守卫覆盖不到，
 * 只改 config.rs 就会让小屏判断静默失准（症状是「换到小屏笔记本上窗口下半截
 * 掉出屏幕」，且在大屏上完全看不出来）。现在写成引用，没有第五处可漂移。
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");

const problems = [];

// ---- ① CSS token ------------------------------------------------------------
const css = read("src/style.css");
const cssMatch = css.match(/--window-shadow-margin:\s*(\d+(?:\.\d+)?)px/);
if (!cssMatch) {
  problems.push("src/style.css 找不到 `--window-shadow-margin` 声明");
}
const cssMargin = cssMatch ? Number(cssMatch[1]) : NaN;

// ---- ② Rust 常量 ------------------------------------------------------------
const libRs = read("src-tauri/src/lib.rs");
const rsMatch = libRs.match(/WINDOW_SHADOW_MARGIN:\s*f64\s*=\s*(\d+(?:\.\d+)?)/);
if (!rsMatch) {
  problems.push("src-tauri/src/lib.rs 找不到 `WINDOW_SHADOW_MARGIN` 定义");
}
const rsMargin = rsMatch ? Number(rsMatch[1]) : NaN;

if (Number.isFinite(cssMargin) && Number.isFinite(rsMargin) && cssMargin !== rsMargin) {
  problems.push(
    `外扩带宽度 CSS=${cssMargin}px ≠ Rust=${rsMargin}px —— 窗口与内容会差一圈（视觉上像边距没对齐）`,
  );
}

// ---- ④ WindowState::default（可视区）----------------------------------------
const configRs = read("src-tauri/src/config.rs");
const defMatch = configRs.match(
  /impl Default for WindowState[\s\S]*?width:\s*(\d+(?:\.\d+)?)\s*,\s*\n\s*height:\s*(\d+(?:\.\d+)?)/,
);
if (!defMatch) {
  problems.push("src-tauri/src/config.rs 找不到 WindowState::default 的 width/height");
}
const defW = defMatch ? Number(defMatch[1]) : NaN;
const defH = defMatch ? Number(defMatch[2]) : NaN;

// ---- ③ tauri.conf.json（inner 尺寸）-----------------------------------------
const conf = JSON.parse(read("src-tauri/tauri.conf.json"));
// 主窗的 label 省略时 Tauri 默认就是 "main"，故 label 缺失也算；
// 若 conf 里有多扇窗而主窗没写 label，则无法无歧义识别，直接报错而不是猜第一个
const allWins = conf.app?.windows ?? [];
const win =
  allWins.find((w) => w.label === "main") ??
  (allWins.length === 1 ? allWins[0] : undefined);
if (!win) {
  problems.push(
    `src-tauri/tauri.conf.json 无法识别主窗（共 ${allWins.length} 扇窗${allWins.map((w) => w.label ?? "<无 label>").join("、")}）——请给主窗显式写 "label": "main"`,
  );
} else if (Number.isFinite(defW) && Number.isFinite(defH)) {
  const band = cssMargin * 2;
  // width/height：与 Rust 的 WindowState::default 硬对账（真正的跨语言校验）
  for (const [key, actual, def] of [
    ["width", win.width, defW],
    ["height", win.height, defH],
  ]) {
    const expected = def + band;
    if (actual !== expected) {
      problems.push(
        `tauri.conf.json main.${key}=${actual}，应为 ${expected}（= WindowState::default ${def} + 2×${cssMargin}）`,
      );
    }
  }
  // minWidth/minHeight：**没有**对应的 Rust 默认值（最小尺寸只在 conf 里），
  // 故不对账具体数字，只校验换算后的「可视区最小值」落在合理区间：
  //   · > 400px —— 低于此布局（220px 侧栏 + 内容）已不可用
  //   · ≤ 默认值 —— 否则 restore_window_state 一开始就把窗口顶在最小尺寸上
  // 这两条在 M 被改动而 conf 忘了跟平时也会变红（M 越大越容易触发下限那条）。
  for (const [key, actual, def] of [
    ["minWidth", win.minWidth, defW],
    ["minHeight", win.minHeight, defH],
  ]) {
    const visible = actual - band;
    if (!(visible > 400)) {
      problems.push(
        `tauri.conf.json main.${key}=${actual} → 可视区最小 ${visible}px，已 ≤ 400px 下限（布局不可用）`,
      );
    }
    if (visible > def) {
      problems.push(
        `tauri.conf.json main.${key}=${actual} → 可视区最小 ${visible}px > 默认 ${def}px，最小值将反过来顶住启动尺寸`,
      );
    }
  }
}

// ---- 阴影延伸量不得超过外扩带（否则阴影被窗口边缘切平）----------------------
// box-shadow 的 `offsetY + blur` 决定向下的可见延伸，blur 决定两侧；
// 超出 --window-shadow-margin 的部分会被窗口边界裁掉，表现为「阴影被削平」。
//
// ⚠️ 必须遍历**全部**声明（亮色 + `[data-theme="dark"]` 各自的），不能只看第一处：
// 暗色阴影是另写一遍的，只查亮色等于放暗色一马过去 —— 而「暗色阴影被切平」
// 恰恰是浅色阴影看不出来的那个问题。
if (Number.isFinite(cssMargin)) {
  const decls = [...css.matchAll(/--window-shadow:\s*([^;]+);/g)];
  if (!decls.length) {
    problems.push("src/style.css 找不到 `--window-shadow` 声明");
  }
  for (const [, value] of decls) {
    const v = value.trim();
    // `none`（最大化态归零）与 `0`（无阴影）天然满足约束，直接跳过
    if (v === "none" || /^0(px)?(\s+0(px)?)*$/.test(v)) continue;
    // 形状是 `<offset-x> <offset-y> <blur> <color>`；**偏移量允许无单位**（`0`），
    // 所以不能要求三段都带 px —— 早期版本因此写错成 `/px$/` 过滤，
    // 结果 `0 8px 40px …` 只捞到 2 段、整条规则被静默跳过（负例测不出来）。
    const nums = v
      .split(/\s+/)
      .map((p) => p.match(/^(-?[\d.]+)(px)?$/))
      .filter(Boolean)
      .map((m) => Number.parseFloat(m[1]));
    if (nums.length < 3) {
      // 解析不出来就必须报出来：否则这条规则等于没有，静默放过比不写更糟
      problems.push(
        `无法解析 --window-shadow（读到 ${nums.length} 个数值，期望至少 3 个）："${v}"`,
      );
      continue;
    }
    const [, y, blur] = nums;
    if (y + blur > cssMargin) {
      problems.push(
        `--window-shadow "${v}" 的向下延伸 ${y}+${blur}=${y + blur}px 超过 --window-shadow-margin ${cssMargin}px，阴影会被窗口边缘切平`,
      );
    }
  }
}

if (problems.length) {
  console.error(`[window-margin] 窗口外扩带口径不一致 ${problems.length} 条：`);
  for (const p of problems) console.error(`  ✗ ${p}`);
  console.error(
    "  改法：改 src/style.css 的 `--window-shadow-margin` 与 src-tauri/src/lib.rs 的\n" +
      "        `WINDOW_SHADOW_MARGIN` 两处之一后，把另一处与 tauri.conf.json 的\n" +
      "        width/height/minWidth/minHeight 一起对齐（后两项是 inner 尺寸 = 可视区 + 2×外扩带）。",
  );
  process.exit(1);
}
console.log(
  `[window-margin] 外扩带口径一致（${cssMargin}px × 4 处 + 阴影延伸量）`,
);
