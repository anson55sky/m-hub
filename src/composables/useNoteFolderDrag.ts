import { ref } from 'vue'

/**
 * 「把笔记拖进文件夹」的共享拖拽状态（v0.8.0，发布说明 ⑭）。
 *
 * 为什么必须是**模块级单例**：拖的发起方（`NoteList` 笔记行）与落点接收方
 * （`NoteFolderTree` 文件夹行）是兄弟组件，没有共同的直接父组件能持有 props ——
 * 硬要把状态提到 index.vue 再往下传两层，会让一个纯交互状态污染视图协调层。
 *
 * 为什么不用 HTML5 DnD：主窗口开了 `dragDropEnabled`（Tauri 原生文件拖放拦截），
 * 它与 WebView 的 HTML5 DnD **互斥** —— 拖得动、落不下。工程里速达小类排序、
 * 笔记块拖拽（约定 38）、待办排序（约定 40）三处都已踩过，一律用指针实现。
 *
 * ⚠️ `draggingNoteId` 为 `null` 有**两种**含义（不是 idle）：「没在拖」与
 *   「拖到一半松开但 up 没派发」。所以判「是否在拖笔记」要看 `dragActive`，
 *   不能只看 id —— 否则窗口外松键后残留的 id 会让文件夹行一直高亮。
 */
const draggingNoteId = ref<number | null>(null)
const dragActive = ref(false)

export function useNoteFolderDrag() {
  function begin(noteId: number) {
    draggingNoteId.value = noteId
    dragActive.value = true
  }
  function end() {
    draggingNoteId.value = null
    dragActive.value = false
  }
  return { draggingNoteId, dragActive, begin, end }
}
