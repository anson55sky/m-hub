/**
 * 守卫：工作台默认布局（PRESET）的几何必须合法
 *
 * ## 为什么需要它
 *
 * `useDashboardLayout.ts` 里的 `PRESET` 是一份**手写的坐标表**
 * （每个模块的 `x/y/w/h`）。它没有任何编译期约束：写错了不会有类型错误、
 * 不会有构建失败，只会在运行时变成「模块互相压住 / 超出 12 列 / 被挤成
 * 看不见的细条」，而这些只有打开窗口才看得见。
 *
 * 2026-09-29 重排默认布局时，这类错误的风险很具体 —— 坐标表是手算的，
 * 改一处很容易让相邻两个模块的区间相交。所以把不变量写成守卫：
 *
 *   ① 任意两个模块的矩形**不重叠**
 *   ② 任何一行被覆盖的列数 **≤ 12**（栅格是 12 列）
 *   ③ 每个模块的 w/h **≥ 其 variant 自己声明的 min**
 *      （min 来自 DASH_MODULES，改 variant 尺寸时这条会跟着生效）
 *   ④ 总行数 **= 15**（与旧模板一致：重排不该顺带改掉窗口的高度需求，
 *      否则用户点「重置布局」会看到窗口内容突然变高/变矮）
 *
 * ③ 是这四条里最容易被忽略也最容易犯的：`prompts` 曾被放在 5×8，
 * 而它声明的 ideal 是 4×3 —— 40 格的内容设计给 12 格，超配 3.3 倍，
 * 于是那块地长期空着。这条守卫拦住的是「超配」的反面（欠配到 min 以下），
 * 而「超配」属于设计问题、不该由机械校验裁决，只在这里提醒它存在。
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = readFileSync(join(root, "src/composables/useDashboardLayout.ts"), "utf8");

const GRID_COLS = 12;
const EXPECTED_ROWS = 15;

const problems = [];

// ---- 解析 DASH_MODULES 的 min 尺寸：min[(id, variant)] = [minW, minH, idealW, idealH]
const mins = new Map();
{
  const block = src.slice(src.indexOf("const DASH_MODULES"));
  for (const chunk of block.split(/\n  \{\n/).slice(1)) {
    const id = chunk.match(/id: '([a-z0-9]+)'/)?.[1];
    if (!id) continue;
    // v('name', 'label', minW, minH, idealW, idealH, 'desc')
    const re = /v\('([a-z]+)',\s*'[^']*',\s*(\d+),\s*(\d+),\s*(\d+),\s*(\d+)/g;
    let m;
    while ((m = re.exec(chunk)) !== null) {
      mins.set(`${id}/${m[1]}`, {
        minW: +m[2],
        minH: +m[3],
        idealW: +m[4],
        idealH: +m[5],
      });
    }
  }
}
if (mins.size === 0) {
  console.error("[dash-preset] 解析不到任何 variant 尺寸，守卫本身失效（源码结构变了？）");
  process.exit(1);
}

// ---- 解析 PRESET
const presetBlock = src.slice(
  src.indexOf("const PRESET"),
  src.indexOf("function defaultPlacements"),
);
const items = [...presetBlock.matchAll(
  /\{\s*id:\s*'([a-z0-9]+)',\s*variant:\s*'([a-z]+)',\s*x:\s*(\d+),\s*y:\s*(\d+),\s*w:\s*(\d+),\s*h:\s*(\d+)\s*\}/g,
)].map((m) => ({
  id: m[1],
  variant: m[2],
  x: +m[3],
  y: +m[4],
  w: +m[5],
  h: +m[6],
}));

if (items.length === 0) {
  console.error("[dash-preset] 解析不到 PRESET 里的任何模块，守卫本身失效（源码结构变了？）");
  process.exit(1);
}

const label = (i) => `${i.id}/${i.variant} @(${i.x},${i.y}) ${i.w}×${i.h}`;

// ---- ① 不重叠
const overlaps = (a, b) =>
  a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
for (let i = 0; i < items.length; i++) {
  for (let j = i + 1; j < items.length; j++) {
    if (overlaps(items[i], items[j])) {
      problems.push(`${label(items[i])} 与 ${label(items[j])} 矩形重叠`);
    }
  }
}

// ---- ② 每一行都必须铺满（这是「整齐」的可校验形式）
const totalRows = Math.max(...items.map((i) => i.y + i.h));
for (let y = 0; y < totalRows; y++) {
  const used = new Set();
  for (const i of items) {
    if (i.y <= y && y < i.y + i.h) {
      for (let x = i.x; x < i.x + i.w; x++) used.add(x);
    }
  }
  if (used.size > GRID_COLS) {
    problems.push(`第 ${y} 行覆盖了 ${used.size} 列，超过栅格的 ${GRID_COLS} 列`);
  } else if (used.size < GRID_COLS) {
    // 找出这一行缺了哪几段，好让人一眼看出空洞在哪
    const gaps = [];
    let run = null;
    for (let x = 0; x < GRID_COLS; x++) {
      if (!used.has(x)) {
        if (!run) run = [x, x];
        else run[1] = x;
      } else if (run) {
        gaps.push(run[0] === run[1] ? `第 ${run[0]} 列` : `第 ${run[0]}–${run[1]} 列`);
        run = null;
      }
    }
    if (run) {
      gaps.push(run[0] === run[1] ? `第 ${run[0]} 列` : `第 ${run[0]}–${run[1]} 列`);
    }
    problems.push(
      `第 ${y} 行只覆盖 ${used.size}/${GRID_COLS} 列，${gaps.join("、")} 是空的。\n` +
        `        留一格空在版面上就是一块看得见的洞 —— 用户说的「不够整齐」多半指这个。\n` +
        `        修法：调各段的 y/h 让每段高度一致，段与段依次紧贴铺满 12 列。`,
    );
  }
}

// ---- ③ 同一 y 上的模块高度必须一致
{
  const byY = new Map();
  for (const i of items) {
    if (!byY.has(i.y)) byY.set(i.y, []);
    byY.get(i.y).push(i);
  }
  for (const [y, band] of [...byY.entries()].sort((a, b) => a[0] - b[0])) {
    const hs = [...new Set(band.map((i) => i.h))];
    if (hs.length > 1) {
      problems.push(
        `y=${y} 这一排有 ${hs.length} 种不同高度（${hs.join(" / ")}）：` +
          `${band.map((i) => `${i.id}(${i.w}×${i.h})`).join(" ")}\n` +
          `        顶边齐、底边错开，就是「不整齐」。\n` +
          `        修法：同一排的模块给同一个 h（整段折叠也依赖这条）。`,
      );
    }
    const width = band.reduce((n, i) => n + i.w, 0);
    if (width !== GRID_COLS) {
      problems.push(`y=${y} 这一排宽度合计 ${width}，应为 ${GRID_COLS}`);
    }
  }
}

// ---- ④ 不小于 variant 声明的 min
for (const i of items) {
  const def = mins.get(`${i.id}/${i.variant}`);
  if (!def) {
    problems.push(`${i.id} 没有 variant '${i.variant}'（拼错了？或该 variant 不存在）`);
    continue;
  }
  if (i.w < def.minW || i.h < def.minH) {
    problems.push(
      `${i.id}/${i.variant} 放在 ${i.w}×${i.h}，小于它声明的最小尺寸 ${def.minW}×${def.minH}`,
    );
  }
}

// ---- ⑤ 总行数不变
if (totalRows !== EXPECTED_ROWS) {
  problems.push(
    `总行数 ${totalRows}，应为 ${EXPECTED_ROWS}。\n` +
      `        重排默认布局不该顺带改掉窗口的高度需求 —— 否则用户点「重置布局」\n` +
      `        会看到内容区高度突变。要加行就先确认这是有意的。`,
  );
}

if (problems.length) {
  console.error(`[dash-preset] 默认布局几何不合法 ${problems.length} 条：`);
  for (const p of problems) console.error(`  ✗ ${p}`);
  console.error(
    "\n  改法：调 src/composables/useDashboardLayout.ts 的 PRESET 坐标表。\n" +
      "        要看每块地该多大，查同文件 DASH_MODULES 里各 variant 的 min/ideal。",
  );
  process.exit(1);
}
const bandCount = new Set(items.map((i) => i.y)).size;
console.log(
  `[dash-preset] 默认布局整齐（${items.length} 模块 / ${bandCount} 段 / ${totalRows} 行：\n` +
    `          逐行铺满 ${GRID_COLS}/${GRID_COLS}、同段等高、无重叠、均不小于 min）`,
);
