/**
 * 速达小类的**路径**规则（层级，2026-10-04）—— 前端侧。
 *
 * ## 与 Rust 那份的关系
 *
 * 真源是 `src-tauri/src/repo/subcategory.rs` 的同名函数。**判定**以 Rust 为准，
 * 本文件只提供前端渲染与筛选要用到的纯函数，并且刻意保持同样写法 ——
 * 任何一侧改了另一侧都要跟着改，所以下面每条都配了「变异会红」的用例。
 *
 * ## 为什么路径存法能省掉一次迁移
 *
 * `resource_subcategories.name` 直接存全路径（`开发`、`开发/前端`），
 * 于是老的 `UNIQUE(kind, name)` 继续有效、老数据天然是顶层、`resources.category`
 * 的语义一个字没改。新增一列 `parent_id` 的话要回填，而回填错了表现为
 * 「条目挂到不存在的父级下」——比不改更糟。
 */

/** 层级分隔符。与 `subcategory::SEP` 同为 `/`。 */
export const SEP = '/'

/** 全路径的最后一段（显示用） */
export function leafOf(path: string): string {
  const i = path.lastIndexOf(SEP)
  return i === -1 ? path : path.slice(i + 1)
}

/** 全路径去掉最后一段；顶层返回空串 */
export function parentPath(path: string): string {
  const i = path.lastIndexOf(SEP)
  return i === -1 ? '' : path.slice(0, i)
}

/** 层级深度：顶层 0 */
export function depthOf(path: string): number {
  return path.split(SEP).length - 1
}

/**
 * `path` 是否在 `ancestor` 之下（**含自身**）。
 *
 * ⚠️ 必须按**段**比，不是 `startsWith`：`开发` 不是 `开发者` 的祖先，
 *   而 `startsWith('开发')` 会说是 —— 于是筛「开发」会带出「开发者」下的条目，
 *   删「开发者」会连带删掉「开发」下的全部条目，而界面上这两行看着毫无关系。
 */
export function subcategoryContains(path: string | null, ancestor: string): boolean {
  if (path == null) return false
  if (ancestor === '') return true
  if (path === ancestor) return true
  return (
    path.length > ancestor.length &&
    path.startsWith(ancestor) &&
    path.charAt(ancestor.length) === SEP
  )
}

/** `path` 在 `from` 之下时改挂到 `to` 之下，返回新路径；不在其下则 null */
export function reparentPath(from: string, to: string, path: string): string | null {
  if (path === from) return to
  if (!path.startsWith(from) || path.charAt(from.length) !== SEP) return null
  return to + path.slice(from.length)
}