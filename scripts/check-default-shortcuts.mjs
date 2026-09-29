/**
 * 校验「全局快捷键默认值」的两份拷贝一致（见 AGENTS.md 约定 70）
 *
 * ## 为什么需要它
 *
 * 快捷键默认值有两份拷贝：
 *   ① `src-tauri/src/shortcut.rs` 的四个 `DEFAULT_*` 常量 —— **真相源**（真机生效）
 *      它们按 `#[cfg(target_os = …)]` **分叉**，两平台**允许不同**
 *      （剪贴板：macOS `⌃⌘V` / Windows `Ctrl+\``，理由见 shortcut.rs 注释）
 *   ② `src/utils/platform.ts` 的 `DEFAULT_SHORTCUT_VARIANTS` —— 镜像，
 *      **每个键同时存 mac / other 两个分支**，构建期两侧都要对上
 *
 * ② 存在的理由：设置页的 placeholder、搜索弹窗的 `<kbd>`、AI 对话空视图的提示，
 * 都要回答「用户还没改过快捷键时该显示什么」——那时配置里可能还是空串，
 * 前端读不到 Rust 的常量。
 *
 * ## 漏改的症状
 *
 * 纯静默，且两个方向的漏法都很隐蔽：
 *  · 只改 ① → 预览/占位显示旧键，用户以为没生效或以为自己记错了
 *  · 只改 ② → 更糟：界面上写着新键，真机却是旧键
 *
 * 历史上已经踩过一次：② 里的剪贴板默认值是 `CommandOrControl+Alt+V`（⌘⌥V），
 * 而 ① 早就换成了 `CommandOrControl+Control+V` —— 而 ⌘⌥V 正是**被明确否决过**的键
 * （macOS 自带「粘贴并匹配样式」，系统级注册，第三方抢不到）。
 *
 * ## 附带守一条禁令
 *
 * 剪贴板快捷键**不得**取 `CommandOrControl+Alt+V`。这不是风格问题：那个组合在
 * macOS 上根本注册不上，且失败是静默的。与其靠人记得，不如让构建期拦住。
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");

const problems = [];

// ---- ① Rust 真相源（注意：常量是按平台 #[cfg] 分叉的）--------------------------
const rust = read("src-tauri/src/shortcut.rs");
/** 常量名 → { mac, other }；缺哪边为 null */
const rustDefaults = {};
for (const m of rust.matchAll(
  /pub const (DEFAULT_\w+):\s*&str\s*=\s*"([^"]+)"/g,
)) {
  // 常量本身不带 cfg 信息，用它前面紧邻的 cfg 行判断归属
  const before = rust.slice(Math.max(0, m.index - 70), m.index);
  const entry = (rustDefaults[m[1]] ??= { mac: null, other: null });
  if (/#\[cfg\(not\(target_os = "macos"\)\)\]\s*\n?\s*$/.test(before)) {
    entry.other = m[2];
  } else {
    entry.mac = m[2];
  }
}

// Rust 常量名 → platform.ts 的键名
const RUST_KEYS = {
  DEFAULT_TOGGLE_SHORTCUT: "toggle",
  DEFAULT_CLIPBOARD_SHORTCUT: "clipboard",
  DEFAULT_SEARCH_SHORTCUT: "search",
  DEFAULT_CHAT_SHORTCUT: "chat",
  // 统一捕获（2026-09-29 新增）
  DEFAULT_CAPTURE_SHORTCUT: "capture",
};

// ---- ② TS 镜像（每键存 mac / other 两个分支）----------------------------------
const ts = read("src/utils/platform.ts");
const block = ts.match(/DEFAULT_SHORTCUT_VARIANTS = \{([\s\S]*?)\} as const/);
if (!block) {
  problems.push("src/utils/platform.ts 找不到 DEFAULT_SHORTCUT_VARIANTS 声明");
} else {
  const tsVariants = {};
  // 形如 `  key: { mac: '…', other: '…' },`
  for (const m of block[1].matchAll(/(\w+):\s*\{\s*mac:\s*'([^']*)'\s*,\s*other:\s*'([^']*)'\s*,?\s*\}/g)) {
    tsVariants[m[1]] = { mac: m[2], other: m[3] };
  }

  for (const [rustName, tsKey] of Object.entries(RUST_KEYS)) {
    const r = rustDefaults[rustName];
    const t = tsVariants[tsKey];
    if (!r) {
      problems.push(`src-tauri/src/shortcut.rs 找不到常量 ${rustName}`);
      continue;
    }
    if (!t) {
      problems.push(`src/utils/platform.ts 的 DEFAULT_SHORTCUT_VARIANTS 缺少 "${tsKey}" 键`);
      continue;
    }
    // 两个分支都要对上。mac 分支是重点（当前构建平台），
    // other 分支同样要锁：它只在该平台构建/运行时才会被读到，
    // 错了不会在开发机上暴露，等于没有反馈。
    if (r.mac !== t.mac) {
      problems.push(
        `${rustName} 的 macOS 分支不一致：shortcut.rs="${r.mac}" ≠ platform.ts="${t.mac}"`,
      );
    }
    if (r.other !== null && r.other !== t.other) {
      problems.push(
        `${rustName} 的非 macOS 分支不一致：shortcut.rs="${r.other}" ≠ platform.ts="${t.other}"。\n` +
          `        注意这个分支只在该平台才被读到，开发机上验不出来。`,
      );
    }
  }

  for (const key of Object.keys(tsVariants)) {
    if (!Object.values(RUST_KEYS).includes(key)) {
      problems.push(
        `platform.ts 的 DEFAULT_SHORTCUT_VARIANTS.${key} 在 shortcut.rs 里没有对应常量 —— ` +
          `要么删掉，要么在 shortcut.rs 补一个`,
      );
    }
  }
}

// ---- 禁令：剪贴板快捷键不得用 ⌘⌥V ---------------------------------------------
if (rustDefaults.DEFAULT_CLIPBOARD_SHORTCUT === "CommandOrControl+Alt+V") {
  problems.push(
    "clipboard 快捷键不能用 CommandOrControl+Alt+V（⌘⌥V）：那是 macOS 自带的" +
      "「粘贴并匹配样式」，系统级注册，第三方应用注册会**静默失败**（没人看得见日志）。",
  );
}

if (problems.length) {
  console.error(`[shortcuts] 全局快捷键默认值口径不一致 ${problems.length} 条：`);
  for (const p of problems) console.error(`  ✗ ${p}`);
  console.error(
    "  真相源是 src-tauri/src/shortcut.rs。改默认值请两边一起改：\n" +
      "    ① src-tauri/src/shortcut.rs 的 DEFAULT_* 常量\n" +
      "    ② src/utils/platform.ts 的 DEFAULT_SHORTCUTS",
  );
  process.exit(1);
}
console.log(
  `[shortcuts] 五个默认值口径一致（shortcut.rs ⇄ platform.ts：${Object.values(RUST_KEYS).join("/")}）`,
);
