<script setup lang="ts">
/**
 * 速记文件夹树（v0.8.0，发布说明 ①⑭）。
 *
 * 三块：层级展示与筛选、右键/行内菜单（新建·重命名·删除）、笔记拖入落点。
 *
 * ## 为什么用指针实现拖拽而不是 HTML5 DnD
 *
 * 主窗口开了 `dragDropEnabled`（Tauri 原生文件拖放拦截），它与 WebView 的
 * HTML5 DnD **互斥** —— 拖得动、落不下。工程里速达小类排序、笔记块拖拽
 * （约定 38）、待办排序（约定 40）三处都已踩过，此处沿用指针方案。
 * 拖的发起方在 `NoteList`，状态经 `useNoteFolderDrag` 共享（兄弟组件无共同
 * 父级可传 props，而把纯交互状态提到视图协调层是污染）。
 *
 * ## 菜单为什么用 ContextMenu 而不是行内绝对定位
 *
 * 列表是 `overflow-y: auto`，行内 `position: absolute` 的菜单会被**裁掉下半截**
 * （我第一版就是，表现为「菜单点不开」）。ContextMenu 是 `Teleport to="body"`
 * + 窗口边界钳制，不受滚动容器裁剪。
 * ⚠️ 置位必须 `setTimeout(0)` —— 它在 window 上监听 click/contextmenu 关闭，
 *   同一事件派发内同步置位会被紧接着的全局关闭监听关掉（表现为右键无反应）。
 */
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import {
  ChevronRight,
  FileText,
  Folder,
  FolderOpen,
  FolderPlus,
  Inbox,
  MoreHorizontal,
} from 'lucide-vue-next'
import type { FolderNode } from '../api/tauri'
import { type NoteFolderTarget } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { useNoteFolderDrag } from '../composables/useNoteFolderDrag'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'
import ConfirmDialog from './ConfirmDialog.vue'

const props = defineProps<{
  /** 当前筛选目标。`null` = 全部，`'unfiled'` = 未归类，数字 = 该文件夹含后代。 */
  active: NoteFolderTarget
}>()

const emit = defineEmits<{
  (e: 'select', target: NoteFolderTarget): void
  (e: 'error', msg: string): void
}>()

const store = useStore()
const { draggingNoteId, dragActive, end: endDrag } = useNoteFolderDrag()

const tree = computed(() => store.noteFolderTree())

/** 未归类的条数：树里没有它的节点，只能从已加载的笔记列表里数。 */
const unfiledCount = computed(
  () => store.state.notes.filter((n) => n.folder_id === null).length,
)

/** 展开状态按 `full_path` 存 —— id 会因改名变吗？不会，但路径是界面已有的键。 */
const expanded = ref<Set<string>>(new Set())

function isExpanded(path: string) {
  return expanded.value.has(path)
}

function toggle(path: string) {
  const next = new Set(expanded.value)
  if (next.has(path)) next.delete(path)
  else next.add(path)
  expanded.value = next
}

/** 拖到折叠的父文件夹上时要能看到落点，所以悬停即展开祖先链。 */
function expandAncestors(path: string) {
  const next = new Set(expanded.value)
  const parts = path.split('/')
  for (let i = 1; i < parts.length; i++) next.add(parts.slice(0, i).join('/'))
  expanded.value = next
}

// 顶层节点进来就展开：否则用户新建完文件夹看不见它（症状「新建了但没出现」）
watch(
  tree,
  (nodes) => {
    const next = new Set(expanded.value)
    for (const n of nodes) next.add(n.full_path)
    expanded.value = next
  },
  { immediate: true },
)

interface FlatRow {
  node: FolderNode
  depth: number
  hasChildren: boolean
}

/** 渲染序 = 树的前序遍历，与 Rust `tree()` 的产出顺序一致。 */
const flatRows = computed(() => {
  const out: FlatRow[] = []
  const walk = (nodes: readonly FolderNode[], depth: number) => {
    for (const n of nodes) {
      out.push({ node: n, depth, hasChildren: n.children.length > 0 })
      if (isExpanded(n.full_path)) walk(n.children, depth + 1)
    }
  }
  walk(tree.value, 0)
  return out
})

// ── 行内改名 ────────────────────────────────────────────────
//
// 用行内输入而不是 `window.prompt()`：prompt 是**同步阻塞**的，在 Tauri 里它
// 还会与自制标题栏抢焦点；而且 prompt 拿不到空提交/取消的清晰区分。
const editingPath = ref<string | null>(null)
const editingValue = ref('')
const editingEl = ref<HTMLInputElement | null>(null)

function beginRename(node: FolderNode) {
  editingPath.value = node.full_path
  editingValue.value = node.name
  void nextTick(() => {
    editingEl.value?.focus()
    // 只选中最后一段：改名只改自己那一段（约定 76），选中整条路径会诱导用户
    // 连父级一起改掉，而那是另一个操作。
    const seg = editingValue.value.split('/').pop() ?? ''
    editingEl.value?.setSelectionRange(
      editingValue.value.length - seg.length,
      editingValue.value.length,
    )
  })
}

async function commitRename() {
  const path = editingPath.value
  editingPath.value = null
  if (!path) return
  const value = editingValue.value.trim()
  if (!value || value === path.split('/').pop()) return
  try {
    // 新路径由 Rust 算（`reparent`），前端不拼 —— 两边规则一旦不同步，
    // 表现就是「改名后子树挂到不存在的父级下」。
    const node = flatRows.value.find((r) => r.node.full_path === path)?.node
    if (!node) return
    await store.editNoteFolder(node.id, value)
  } catch (e) {
    emit('error', e instanceof Error ? e.message : String(e))
  }
}

function cancelRename() {
  editingPath.value = null
}

// ── 菜单 ───────────────────────────────────────────────────

const menu = ref({ visible: false, x: 0, y: 0, items: [] as ContextMenuItem[] })

function openMenu(e: MouseEvent, items: ContextMenuItem[]) {
  // ⚠️ 必须延迟到当前事件派发结束后（约定：Suda.vue::openMenu 同款）
  setTimeout(() => {
    menu.value = { visible: true, x: e.clientX, y: e.clientY, items }
  }, 0)
}

/** 新建名字自动避开重名 —— Rust 侧 `create` 对重名直接报错，让用户撞一次
 *  不如在前端先取个不冲突的（真源仍是 Rust，取不到也只会得到一条明确报错）。 */
function freeName(base: string, taken: Set<string>): string {
  if (!taken.has(base)) return base
  for (let i = 2; i < 1000; i++) {
    const cand = `${base} ${i}`
    if (!taken.has(cand)) return cand
  }
  return `${base} ${Date.now()}`
}

async function createChild(parent: string) {
  try {
    const taken = new Set<string>()
    const collect = (nodes: readonly FolderNode[]) => {
      for (const n of nodes) {
        taken.add(n.full_path)
        collect(n.children)
      }
    }
    collect(tree.value)
    const name = freeName('新文件夹', taken)
    await store.addNoteFolder(parent, name)
    // 建完直接进入改名态：默认名是占位，不改就存会留下一堆「新文件夹」
    const path = parent ? `${parent}/${name}` : name
    const row = flatRows.value.find((r) => r.node.full_path === path)
    if (row) beginRename(row.node)
  } catch (e) {
    emit('error', e instanceof Error ? e.message : String(e))
  }
}

// ── 删除（单实例确认框）────────────────────────────────────

const confirmState = ref({ visible: false, title: '', message: '', targetId: 0 })

function askDelete(node: FolderNode) {
  confirmState.value = {
    visible: true,
    title: `删除「${node.name}」？`,
    // 必须点名连带范围与「笔记不会被删」：只写「确定删除吗」时用户会以为
    // 里面的笔记一起没了，从而不敢删（或误以为删了）。
    message: `其中的子文件夹会一并删除；里面的笔记不会被删除，会改挂到「未归类」。`,
    targetId: node.id,
  }
}

async function doDelete() {
  const { targetId } = confirmState.value
  confirmState.value.visible = false
  try {
    await store.removeNoteFolder(targetId)
  } catch (e) {
    emit('error', e instanceof Error ? e.message : String(e))
  }
}

function menuFor(node: FolderNode, e: MouseEvent) {
  const items: ContextMenuItem[] = [
    {
      label: '新建子文件夹',
      onClick: () => void createChild(node.full_path),
    },
    { label: '重命名', onClick: () => beginRename(node) },
    {
      label: '删除',
      danger: true,
      dividerBefore: true,
      onClick: () => askDelete(node),
    },
  ]
  openMenu(e, items)
}

// ── 笔记拖入（落点）────────────────────────────────────────

const rowEls = new Map<string, HTMLElement>()
const scroller = ref<HTMLElement | null>(null)

/** 落点：`path === ''` 表示「未归类」那一行。 */
const dropPath = ref<string | null>(null)
const dropEdge = ref<'into' | 'before' | 'after'>('into')

function setRowEl(path: string, el: unknown) {
  if (el instanceof HTMLElement) rowEls.set(path, el)
  else rowEls.delete(path)
}

function updateDropTarget(clientY: number) {
  if (!dragActive.value) return
  let best: { path: string; edge: 'into' | 'before' | 'after' } | null = null

  // 未归类行也算一个落点（移出文件夹），但它不是 FolderNode，单独算
  const unfiledEl = rowEls.get('')
  if (unfiledEl) {
    const r = unfiledEl.getBoundingClientRect()
    if (clientY >= r.top && clientY <= r.bottom) {
      dropPath.value = ''
      dropEdge.value = 'into'
      return
    }
  }

  for (const row of flatRows.value) {
    const el = rowEls.get(row.node.full_path)
    if (!el) continue
    const r = el.getBoundingClientRect()
    if (clientY < r.top || clientY > r.bottom) continue
    const rel = (clientY - r.top) / r.height
    // 上下各 1/4 = 插入到它前/后，中间 1/2 = 移进去。
    // ⚠️ 文件夹的 before/after **本轮是 no-op**（拖拽写的是文件夹顺序，而
    //   「改挂到别的父级下」是独立功能，约定 76 同款理由：子级不给把手）。
    //   这里仍画线是为了让用户看见落点位置，commit 时按 into 处理。
    const edge: 'into' | 'before' | 'after' =
      rel < 0.25 ? 'before' : rel > 0.75 ? 'after' : 'into'
    best = { path: row.node.full_path, edge }
    break
  }
  if (best) {
    dropPath.value = best.path
    dropEdge.value = best.edge
    if (best.edge === 'into') expandAncestors(best.path)
  } else {
    dropPath.value = null
  }
}

function autoScroll(clientY: number) {
  const el = scroller.value
  if (!el) return
  const r = el.getBoundingClientRect()
  const EDGE = 40
  if (clientY < r.top + EDGE) el.scrollTop -= 12
  else if (clientY > r.bottom - EDGE) el.scrollTop += 12
}

async function commitDrop() {
  const noteId = draggingNoteId.value
  const path = dropPath.value
  dropPath.value = null
  endDrag()
  // ⚠️ 必须在读完两个值之后再 endDrag —— end 会把 draggingNoteId 清空，
  //   先清后读就变成「拿到落点、id 却没了」，笔记静默不动。
  if (noteId === null || path === null) return
  try {
    if (path === '') {
      await store.moveNoteToFolder(noteId, null)
    } else {
      const node = flatRows.value.find((r) => r.node.full_path === path)?.node
      if (!node) return
      await store.moveNoteToFolder(noteId, node.id)
    }
    // ⚠️ **不**切换当前筛选：搬完就跳走会让用户失去参照（他正看着这个文件夹的
    //   列表）。笔记如果被搬出当前筛选范围，它从列表里消失是正确表现 ——
    //   它确实不在这个文件夹里了。
  } catch (e) {
    emit('error', e instanceof Error ? e.message : String(e))
  }
}

// 落点计算挂 window：指针会移出行的范围（列表边缘），只监听行内会丢事件
function onPointerMove(e: PointerEvent) {
  if (!dragActive.value) return
  updateDropTarget(e.clientY)
  autoScroll(e.clientY)
}

function onPointerUp() {
  if (!dragActive.value) {
    dropPath.value = null
    return
  }
  void commitDrop()
}

window.addEventListener('pointermove', onPointerMove)
window.addEventListener('pointerup', onPointerUp)
onBeforeUnmount(() => {
  window.removeEventListener('pointermove', onPointerMove)
  window.removeEventListener('pointerup', onPointerUp)
  // 卸载时若正拖着（比如用户切走了视图），必须收尾 —— 否则 noteId 残留，
  // 回到本视图时文件夹行一直高亮。
  endDrag()
})

const dragging = computed(() => dragActive.value && draggingNoteId.value !== null)
</script>

<template>
  <div ref="scroller" class="nf" :class="{ 'nf--dragging': dragging }">
    <!-- 全部笔记：不筛（null） -->
    <button
      class="nf-row nf-root"
      :class="{ active: active === null }"
      @click="emit('select', null)"
    >
      <FileText :size="14" :stroke-width="1.9" class="nf-icon" />
      <span class="nf-name">全部笔记</span>
      <span class="nf-count">{{ store.state.notes.length }}</span>
    </button>

    <!-- 未归类：速记的一等公民，无默认文件夹 -->
    <button
      :ref="(el) => setRowEl('', el)"
      class="nf-row nf-unfiled"
      :class="{
        active: active === 'unfiled',
        'nf-drop': dragging && dropPath === '',
      }"
      @click="emit('select', 'unfiled')"
    >
      <Inbox :size="14" :stroke-width="1.9" class="nf-icon" />
      <span class="nf-name">未归类</span>
      <span class="nf-count">{{ unfiledCount }}</span>
    </button>

    <div class="nf-sep" role="separator" />

    <template v-for="row in flatRows" :key="row.node.full_path">
      <div
        v-if="dropPath === row.node.full_path && dropEdge === 'before'"
        class="nf-insert"
      />
      <div
        :ref="(el) => setRowEl(row.node.full_path, el)"
        class="nf-row"
        :class="{
          active: active === row.node.id,
          'nf-drop': dragging && dropPath === row.node.full_path && dropEdge === 'into',
          'nf-edge-b': dragging && dropPath === row.node.full_path && dropEdge === 'before',
          'nf-edge-a': dragging && dropPath === row.node.full_path && dropEdge === 'after',
        }"
        :style="{ paddingLeft: `${8 + row.depth * 14}px` }"
        @click="emit('select', row.node.id)"
      >
        <!-- 就地改名 -->
        <template v-if="editingPath === row.node.full_path">
          <input
            ref="editingEl"
            v-model="editingValue"
            class="nf-input"
            @click.stop
            @keydown.enter.prevent="commitRename"
            @keydown.esc.stop.prevent="cancelRename"
            @blur="commitRename"
          />
        </template>

        <template v-else>
          <button
            class="nf-caret"
            :class="{ 'nf-caret--empty': !row.hasChildren }"
            :aria-label="isExpanded(row.node.full_path) ? '收起' : '展开'"
            :tabindex="row.hasChildren ? 0 : -1"
            @click.stop="row.hasChildren && toggle(row.node.full_path)"
          >
            <ChevronRight
              :size="12"
              :stroke-width="2.2"
              :class="{ rot: isExpanded(row.node.full_path) }"
            />
          </button>

          <component
            :is="isExpanded(row.node.full_path) && row.hasChildren ? FolderOpen : Folder"
            :size="14"
            :stroke-width="1.9"
            class="nf-icon"
          />
          <span class="nf-name" :title="row.node.full_path">{{ row.node.name }}</span>
          <span class="nf-count">{{ row.node.note_count }}</span>

          <button
            class="nf-more"
            aria-label="文件夹操作"
            @click.stop="menuFor(row.node, $event)"
          >
            <MoreHorizontal :size="13" :stroke-width="2" />
          </button>
        </template>
      </div>
      <div
        v-if="dropPath === row.node.full_path && dropEdge === 'after'"
        class="nf-insert"
      />
    </template>

    <button class="nf-new" @click="createChild('')">
      <FolderPlus :size="13" :stroke-width="1.9" /> 新建文件夹
    </button>

    <div v-if="dragging" class="nf-hint">放到文件夹上即可归类</div>

    <ContextMenu
      :visible="menu.visible"
      :x="menu.x"
      :y="menu.y"
      :items="menu.items"
      @close="menu.visible = false"
    />

    <ConfirmDialog
      :visible="confirmState.visible"
      :title="confirmState.title"
      :message="confirmState.message"
      tone="danger"
      confirm-text="删除"
      @confirm="doDelete"
      @cancel="confirmState.visible = false"
    />
  </div>
</template>

<style scoped>
.nf {
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 4px;
  overflow-y: auto;
  overflow-x: hidden;
  min-height: 0;
  position: relative;
}
.nf--dragging {
  /* 拖拽中不选中文字：指针划过会把「未归类」「全部笔记」整片选蓝 */
  user-select: none;
}
.nf-row {
  display: flex;
  align-items: center;
  gap: 5px;
  width: 100%;
  padding: 4px 8px;
  border: none;
  border-radius: 5px;
  background: none;
  cursor: pointer;
  font-size: 12px;
  color: var(--text-2);
  text-align: left;
  user-select: none;
  position: relative;
}
.nf-row:hover {
  background: var(--bg-card-soft);
}
.nf-row.active {
  background: var(--brand-50);
  color: var(--text-1);
}
.nf-root,
.nf-unfiled {
  /* 非文件夹行不缩进，且不参与 depth 计算 */
  padding-left: 8px;
}
.nf-sep {
  margin: 4px 6px;
  border-top: 1px solid var(--border-soft);
}
.nf-caret {
  display: flex;
  align-items: center;
  flex: 0 0 auto;
  padding: 0;
  border: none;
  background: none;
  color: var(--text-4);
  cursor: pointer;
}
/* 无子级时**占位但不可点**：visibility 保留 12px 宽度，图标不会左右跳动 */
.nf-caret--empty {
  visibility: hidden;
  pointer-events: none;
}
.nf-caret .rot {
  transform: rotate(90deg);
}
.nf-icon {
  flex: 0 0 auto;
  color: var(--text-4);
}
.nf-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.nf-count {
  font-size: 10px;
  color: var(--text-4);
  font-variant-numeric: tabular-nums;
}
.nf-more {
  flex: 0 0 auto;
  padding: 0;
  border: none;
  background: none;
  color: var(--text-4);
  cursor: pointer;
  opacity: 0;
}
.nf-row:hover .nf-more {
  opacity: 1;
}
.nf-input {
  flex: 1;
  min-width: 0;
  padding: 1px 4px;
  font-size: 12px;
  color: var(--text-1);
  background: var(--bg-card-solid);
  border: 1px solid var(--brand-500);
  border-radius: 4px;
  outline: none;
}
.nf-new {
  display: flex;
  align-items: center;
  gap: 5px;
  margin-top: 4px;
  padding: 5px 8px;
  font-size: 12px;
  color: var(--text-4);
  background: none;
  border: 1px dashed var(--border-soft);
  border-radius: 5px;
  cursor: pointer;
}
.nf-new:hover {
  color: var(--text-2);
  border-color: var(--text-4);
}

/* ── 拖拽落点（发布说明 ⑭）───────────────────────────────────
   整行高亮而非一条细线：细线在小屏/低对比度下看不见，而「不知道落哪」
   正是要解决的问题。三种 edge 各有形态，避免用户分不清「移进去」和
   「排在它旁边」。 */
.nf-drop {
  background: var(--brand-50);
  box-shadow: inset 0 0 0 1px var(--brand-500);
}
.nf-edge-b {
  box-shadow: inset 0 2px 0 var(--brand-500);
}
.nf-edge-a {
  box-shadow: inset 0 -2px 0 var(--brand-500);
}
.nf-insert {
  height: 2px;
  margin: 0 8px;
  background: var(--brand-500);
  border-radius: 1px;
}
.nf-hint {
  position: sticky;
  bottom: 4px;
  align-self: center;
  margin-top: 6px;
  padding: 3px 10px;
  font-size: 11px;
  color: var(--text-2);
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-radius: 999px;
  box-shadow: var(--shadow-card);
}
</style>
