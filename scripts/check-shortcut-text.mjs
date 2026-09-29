/**
 * 校验「用户可见文案里不出现写死的 Windows 快捷键」（见 AGENTS.md 约定 70）
 *
 * ## 为什么要查
 *
 * 内部一律存 Tauri 写法（`CommandOrControl+Control+V`），只有**显示**时才转符号
 * （`platform.ts::prettyShortcut`）。写死 `Ctrl+…` 到模板里的后果分两种，
 * 都很糟：
 *  · 硬编码 `Ctrl+Shift+K` → macOS 用户读到 `⌃⇧K`，而同一功能的标题栏 tooltip
 *    写着 `⌘⇧K`。用户去按没反应，回来当成 bug。
 *  · 直接渲染 `config.xxx_shortcut` → 弹窗里出现字面量 `CommandOrControl+K`，
 *    而用户刚用 `⌘K` 唤起它。
 *
 * 两者**类型检查全绿、单测全绿**，只有实机看得见。
 *
 * ## 豁免
 *
 * · `src/utils/platform.ts` —— 转换器本身
 * · `src/composables/useShortcutRecorder.ts` —— 按键归一化器，内部就是要 `CommandOrControl`
 * · `src/api/tauri.ts` —— 只在注释里说明「内部存什么写法」
 * · 注释与插值表达式（`{{ }}`）
 * · **属性绑定**（`:title="…"`, `v-bind:placeholder="…"`）—— 那里面是 JS 表达式，
 *   出现 `Ctrl+K` 通常是 `prettyShortcut(cfg, isMac ? '⌘K' : 'Ctrl+K')` 的**平台兜底分支**，
 *   正是应该写的。转换由 `prettyShortcut` 负责，不归本守卫管。
 *
 * 只查**静态文本节点**与**静态属性值**（`placeholder="Ctrl+K"` 这种）—— 那才是
 * 「绕过转换器、把 Windows 键直接怼到用户眼前」的地方。
 *
 * ⚠️ 早期版本没剥属性绑定，把 `prettyShortcut(…, isMac ? '⌘K' : 'Ctrl+K')` 里的
 * 平台兜底也报了 —— 假阳性比漏报更糟（守卫一旦喊狼来了就没人再理它）。
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/** 豁免：这些文件就是要处理 `CommandOrControl` / 写默认值本身 */
const EXEMPT = new Set([
  "src/utils/platform.ts",
  "src/composables/useShortcutRecorder.ts",
  "src/api/tauri.ts",
]);

function collect(dir, out = []) {
  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name === "dist" || name === ".git") continue;
    const p = join(dir, name);
    if (statSync(p).isDirectory()) collect(p, out);
    else if (name.endsWith(".vue")) out.push(p);
  }
  return out;
}

const problems = [];
const files = collect(join(root, "src"));

// 模板文本节点与静态属性值里出现的「裸 Ctrl+/Control+ 组合键」
//
// 用**否定环视**而不是「前置字符白名单」：早先写成
// `/(?:^|[\s"'`（(，、：:])((?:Ctrl|Control)\+…)/`，
// 结果 `按 Ctrl+K 搜索` 里的「按」不在那串前置字符里 —— 中文文案全是这种
// 「动词 + 空格 + 键」的写法，等于整个中文界面都漏检。
// 否定环视只要求「前面不是标识符字符」，对中文/标点/行首一律命中。
const BARE =
  /(?<![A-Za-z0-9_.$-])((?:Ctrl|Control)\s*\+\s*[A-Za-z0-9`一-鿿]+(?:\s*\+\s*[A-Za-z0-9`一-鿿]+)*)/g;

/** 剥掉注释、插值、以及属性绑定（双引号或单引号，带换行的绑定用 [\s\S]） */
function scrub(body) {
  return body
    .replace(/<!--[\s\S]*?-->/g, "") // 注释
    .replace(/\{\{[\s\S]*?\}\}/g, "") // 插值表达式
    .replace(/(?::|v-bind:)[\w-]+\s*=\s*"[\s\S]*?"/g, "") // :attr="…"
    .replace(/(?::|v-bind:)[\w-]+\s*=\s*'[\s\S]*?'/g, ""); // :attr='…'
}

for (const file of files) {
  const rel = relative(root, file);
  if (EXEMPT.has(rel.replace(/\\/g, "/"))) continue;
  const src = readFileSync(file, "utf8");
  // 只看 <template> 块：<script> 里出现 Ctrl+ 通常是键盘事件判断（那是逻辑不是文案）
  for (const m of src.matchAll(/<template>([\s\S]*?)<\/template>/g)) {
    const body = scrub(m[1]);
    for (const hit of body.matchAll(BARE)) {
      const line = body.slice(0, hit.index).split("\n").length;
      problems.push(`${rel} 模板第 ${line} 行附近：${hit[1].trim()}`);
    }
  }
}

if (problems.length) {
  console.error(`[shortcut-text] 模板里出现写死的 Windows 快捷键 ${problems.length} 处：`);
  for (const p of problems) console.error(`  ✗ ${p}`);
  console.error(
    "  改法：把文案改成动态绑定，用 platform.ts 的 `shortcutLabel(key, configured)`\n" +
      "        （内部值 → 符号/展开的唯一出口）。例：\n" +
      "        const label = computed(() => shortcutLabel('chat', store.state.config.chat_shortcut))\n" +
      "        <p>按 {{ label }} 唤起</p>",
  );
  process.exit(1);
}
console.log(`[shortcut-text] 模板无写死的 Windows 快捷键（已扫 ${files.length} 个 .vue）`);
