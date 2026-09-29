/**
 * 守卫：所有 CSS / 内联 <style> 的注释必须配平（起始符与收尾符成对）
 *
 * ## 为什么需要它
 *
 * 2026-09-29 一天之内因为「改注释时漏掉收尾的斜杠星号」，把两条规则吞掉了两次：
 *   ① `style.css` 的 `html[data-window-maximized]` —— 整条规则连同选择器一起
 *      被吞进上一个未闭合的注释里，结果**最大化时圆角不再归零**；
 *   ② `index/index.vue` 的 `.app-shell` —— 同样被吞，构建直接报
 *      `CssSyntaxError: Missing opening {`。
 *
 * 这类错误的坏处特别大：
 *   · **静默**。CSS 解析器遇到未闭合注释不会报「你少写了一个收尾符」，
 *     它只是把后面一段当注释吃掉。被吞掉的那条规则就此消失、无声无息。
 *   · **症状离原因很远**。少写一个收尾符，表现是「最大化后圆角没归零」或
 *     「构建挂了」，没人会联想到「半小时前我改过一段注释」。
 *   · **看起来是好的**。如果吞掉的是一条无关紧要的规则，就永远不会被发现。
 *
 * 本工程的 CSS 注释密度极高（几乎每个 token 都带一段「为什么」），所以
 * 「编辑注释」是高频操作，而不是低风险操作。靠人眼盯不现实，交给机器。
 *
 * ## 检查范围
 *
 * 扫全部 `.css` 文件、`index.html`，以及全部 `.vue` 的 `<style>` 块。
 * 对每个文件做一次字符级扫描，统计注释起始符与收尾符的配对，出现
 * 「多余的收尾符」或「有未闭合的起始符」即失败并指出行号。
 *
 * （注释里刻意不写这两个符号的字面形式：写了就会**提前闭合本段注释** ——
 *   写这个脚本的第一版就踩了这个坑，挺讽刺的。）
 *
 * ⚠️ 只做**配平**检查，不做 CSS 语法校验 —— 那属于构建期的事
 * （`vite build` 会报 `CssSyntaxError`）。但构建期只会在**构建失败**时
 * 才拦住你；被吞掉的若是合法但不重要的规则，构建照样成功。
 * 这条守卫覆盖的正是那种「构建是绿的、功能却少了一块」的情形。
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/** 递归收集匹配扩展名的文件 */
function collect(dir, exts, out = []) {
  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name === "target" || name === "dist" || name.startsWith(".")) {
      continue;
    }
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) collect(p, exts, out);
    else if (exts.some((e) => name.endsWith(e))) out.push(p);
  }
  return out;
}

/**
 * 扫描一段文本里的 CSS 注释配对。
 * @returns {null | string} 平衡则 null，否则是问题描述
 */
function unbalanced(text, label) {
  let depth = 0;
  let openLine = 0;
  let line = 1;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === "\n") line++;
    if (text.startsWith("/*", i)) {
      if (depth === 0) openLine = line;
      depth++;
      i++;
      continue;
    }
    if (text.startsWith("*/", i)) {
      depth--;
      if (depth < 0) {
        return `${label}:${line} 出现了多余的注释收尾符（前面没有对应的起始符）`;
      }
      i++;
    }
  }
  if (depth > 0) {
    return `${label}:${openLine} 起的注释没有收尾（后面 ${depth} 个注释都没闭合，\n` +
           `        从这一行起的内容会被解析器整段当成注释吃掉 —— 规则会「消失」且无报错）`;
  }
  return null;
}

const problems = [];

// ① .css 文件
for (const p of collect(join(root, "src"), [".css"])) {
  const r = unbalanced(readFileSync(p, "utf8"), relative(root, p));
  if (r) problems.push(r);
}

// ② index.html（内联 <style>，且**不在** src/ 下，必须单独扫）
{
  const p = join(root, "index.html");
  const r = unbalanced(readFileSync(p, "utf8"), "index.html");
  if (r) problems.push(r);
}

// ③ .vue 的每个 <style> 块
for (const p of collect(join(root, "src"), [".vue"])) {
  const text = readFileSync(p, "utf8");
  const re = /<style[^>]*>([\s\S]*?)<\/style>/g;
  let m;
  let n = 0;
  while ((m = re.exec(text)) !== null) {
    n++;
    const before = text.slice(0, m.index).split("\n").length;
    const r = unbalanced(m[1], `${relative(root, p)} <style#${n}> (约第 ${before + 1} 行起)`);
    if (r) problems.push(r);
  }
}

if (problems.length) {
  console.error(`[css-comment-balance] CSS 注释未配平 ${problems.length} 处：`);
  for (const p of problems) console.error(`  ✗ ${p}`);
  console.error(
    "\n  症状是**静默**的：未闭合的注释起始符会把它后面的规则整段吃掉，\n" +
      "  被吞的若是无关紧要的规则，构建照样是绿的。改完注释请确认收尾符在。",
  );
  process.exit(1);
}
console.log("[css-comment-balance] CSS 注释全部配对");
