import type { Note, NoteFolderTarget } from '../api/tauri'

/**
 * 按文件夹筛选速记（v0.8.0，发布说明 ①）。
 *
 * 三态入参，语义各不相同 —— 这正是它被抽出来单独测的原因：
 * 把三态压成一个 `number | null` 或者在调用处内联判断，出错时的表现都是
 * **「界面选中了 A、列表里却是 B」**，而这种反话没有任何报错会提示你。
 *
 * ## `subtreeIds` 为什么可以是 `null`
 *
 * 子树 id 集合由 Rust `note_folder::subtree_ids` 算（判定真源），前端要
 * await 一次。**取回来之前不能开始筛** —— 否则点文件夹的第一帧会先用
 * 「上一个文件夹的子树」或「空集合」筛一次，表现为「点进去闪一下再出内容」。
 * `null` 表示「还没拿到，先别筛」，与「筛成空」是两回事。
 *
 * ## ⚠️ 数字目标 + 空 id 集 **不等于** 未归类
 *
 * `subtree_ids(id)` 对有效 id 永远非空（`is_within(path, ancestor)` 对
 * path == ancestor 为真，即文件夹总在自己的子树里）。所以「数字 + 空集」
 * 是异常态，此时**该显示空列表**（这个文件夹里没有笔记），而不是退回
 * 未归类 —— 我第一版写成 `if (ids.length === 0) return unfiled`，一旦
 * 真走到那条路，用户会看到未归类的笔记，而界面上高亮的是某个具体文件夹。
 */
export function filterNotesByFolder(
  notes: readonly Note[],
  target: NoteFolderTarget,
  subtreeIds: readonly number[] | null,
): readonly Note[] {
  // 未选目标，或子树还没取回来 → 不筛
  if (target === null || subtreeIds === null) return notes
  if (target === 'unfiled') return notes.filter((n) => n.folder_id === null)
  const ids = new Set(subtreeIds)
  // ⚠️ `folder_id !== null` 是**类型**需要，不是行为需要：`Set<number>.has(null)`
  //   同样返回 false，两者可观察行为完全一致（变异验证实测：把这半句去掉，
  //   全部用例仍绿）。别把它当成「这里防住了某种 bug」。
  return notes.filter((n) => n.folder_id !== null && ids.has(n.folder_id))
}
