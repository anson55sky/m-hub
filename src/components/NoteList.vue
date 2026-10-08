<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { PanelLeft, Plus, StickyNote, Trash2, X } from 'lucide-vue-next'
import type { Note } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { useNoteFolderDrag } from '../composables/useNoteFolderDrag'
import { markdownPlainText } from '../utils/markdown'
import { parseTimestamp } from '../utils/time'

const props = defineProps<{
  notes: readonly Note[]
  activeId: number | null
  /** 正在按文件夹筛选（用于给「全部」以外的筛选态一个可见出口）。 */
  foldersActive?: boolean
  /** 回收站条数；`0` 也显示入口（空回收站要能进去看，否则像功能不存在）。 */
  trashCount?: number
  trashOpen?: boolean
}>()

const emit = defineEmits<{
  (e: 'select', id: number): void
  (e: 'create'): void
  (e: 'delete', id: number): void
  (e: 'toggle-folders'): void
  (e: 'open-trash'): void
}>()

const store = useStore()
const { begin: beginDrag } = useNoteFolderDrag()

/**
 * 笔记行拖拽 → 归类到文件夹（发布说明 ⑭）。
 *
 * 指针实现，不是 HTML5 DnD：主窗口的原生文件拖放拦截与 WebView DnD 互斥
 * （约定 14/38，速达小类、笔记块、待办排序都踩过）。
 *
 * ⚠️ 4px 起拖阈值：没有它的话**单击**也会启动拖拽，而拖拽结束时的 `click`
 *   仍然会派发 —— 表现为「点一下笔记却把它拖进了别的文件夹」。阈值让
 *   「点击」与「拖动」真正分开。
 */
function onNotePointerDown(e: PointerEvent, note: Note) {
  if (e.button !== 0) return
  const startY = e.clientY
  let started = false
  const move = (ev: PointerEvent) => {
    if (!started && Math.abs(ev.clientY - startY) < 4) return
    if (!started) {
      started = true
      beginDrag(note.id)
      // 抑制本次拖拽的选择行为；不 preventDefault（会拦掉后续 click）
      document.body.classList.add('note-dragging')
    }
    void ev
  }
  const up = () => {
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', up)
    document.body.classList.remove('note-dragging')
    // 拖动结束时**取消掉**这次 click —— 否则松手在别的行上会顺带选中它，
    // 用户看到「笔记被拖走了，同时列表跳到了另一条」。
    if (started) suppressNextClick()
  }
  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', up)
}

let suppressClick = false
function suppressNextClick() {
  suppressClick = true
  // 只压制紧随其后的那一次：设成宏任务级别清零，跨过同一个手势
  setTimeout(() => {
    suppressClick = false
  }, 0)
}
function onClickCapture(e: MouseEvent) {
  if (suppressClick) {
    e.stopPropagation()
    e.preventDefault()
    suppressClick = false
  }
}

// ---- 标签筛选 ----
const activeTagId = ref<number | null>(null)
const tagMap = ref<Map<number, number[]>>(new Map()) // note_id -> tag_ids

onMounted(async () => {
  const rows = await store.loadNoteTagsMap()
  const map = new Map<number, number[]>()
  for (const row of rows) {
    const list = map.get(row.note_id) ?? []
    list.push(row.tag_id)
    map.set(row.note_id, list)
  }
  tagMap.value = map
})

const sortedNotes = computed(() => {
  const list = [...props.notes].sort(
    (a, b) => parseTimestamp(b.updated_at) - parseTimestamp(a.updated_at),
  )
  if (activeTagId.value === null) return list
  return list.filter((n) => tagMap.value.get(n.id)?.includes(activeTagId.value!))
})

function formatTime(iso: string): string {
  const t = new Date(parseTimestamp(iso))
  const now = new Date()
  const diffMs = now.getTime() - t.getTime()
  const diffMin = Math.floor(diffMs / 60000)
  if (diffMin < 1) return '刚刚'
  if (diffMin < 60) return `${diffMin} 分钟前`
  const diffHour = Math.floor(diffMin / 60)
  if (diffHour < 24 && sameDay(now, t)) return `${diffHour} 小时前`
  if (sameYear(now, t)) return `${t.getMonth() + 1}月${t.getDate()}日`
  return `${t.getFullYear()}年${t.getMonth() + 1}月${t.getDate()}日`
}

function sameDay(a: Date, b: Date) {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate()
}

function sameYear(a: Date, b: Date) {
  return a.getFullYear() === b.getFullYear()
}

function summary(n: Note): string {
  return markdownPlainText(n.content, 60) || '空白笔记'
}
</script>

<template>
  <section class="card note-list" @click.capture="onClickCapture">
    <header class="nl-header">
      <h2 class="nl-title">
        速记
        <span v-if="foldersActive" class="nl-filtered" title="正在按文件夹筛选">
          筛选中
        </span>
      </h2>
      <div class="nl-head-acts">
        <button
          class="icon-btn"
          :class="{ on: trashOpen }"
          title="回收站"
          aria-label="回收站"
          @click="emit('open-trash')"
        >
          <Trash2 :size="14" :stroke-width="2" />
          <span v-if="trashCount" class="nl-badge">{{ trashCount }}</span>
        </button>
        <button
          class="icon-btn"
          title="文件夹"
          aria-label="显示或隐藏文件夹"
          @click="emit('toggle-folders')"
        >
          <PanelLeft :size="14" :stroke-width="2" />
        </button>
        <button class="icon-btn add" title="新建笔记" @click="emit('create')">
          <Plus :size="15" :stroke-width="2.2" />
        </button>
      </div>
    </header>

    <!-- 标签筛选（横向滚动） -->
    <nav v-if="store.state.tags.length > 0" class="filter-tabs tag-filter" aria-label="标签筛选">
      <button
        class="filter-tab filter-tab--tag"
        :class="{ active: activeTagId === null }"
        @click="activeTagId = null"
      >
        全部
      </button>
      <button
        v-for="t in store.state.tags"
        :key="t.id"
        class="filter-tab filter-tab--tag"
        :class="{ active: activeTagId === t.id }"
        @click="activeTagId = t.id"
      >
        {{ t.name }}
      </button>
    </nav>

    <div v-if="sortedNotes.length > 0" class="nl-body">
      <div
        v-for="n in sortedNotes"
        :key="n.id"
        class="note-item"
        :class="{ active: n.id === activeId }"
        role="button"
        tabindex="0"
        @click="emit('select', n.id)"
        @keydown.enter="emit('select', n.id)"
        @keydown.space.prevent="emit('select', n.id)"
        @pointerdown="onNotePointerDown($event, n)"
      >
        <div class="note-item-main">
          <span class="note-title" :title="n.title">{{ n.title }}</span>
          <span class="note-meta">{{ formatTime(n.updated_at) }}</span>
          <span class="note-summary" :title="summary(n)">{{ summary(n) }}</span>
        </div>
        <button
          class="icon-btn del"
          title="删除笔记"
          aria-label="删除笔记"
          @click.stop="emit('delete', n.id)"
        >
          <X :size="13" :stroke-width="2" />
        </button>
      </div>
    </div>

    <div v-else class="empty-state">
      <StickyNote :size="24" :stroke-width="1.7" aria-hidden="true" />
      <p>还没有笔记</p>
      <button class="pill-btn" style="margin-top: 6px" @click="emit('create')">
        新建笔记
      </button>
    </div>
  </section>
</template>

<style scoped>
.note-list {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px 16px;
  min-height: 0;
  /* 速记模块字号：全局基准 × 模块系数 */
  font-size: calc(1rem * var(--fs-notes, 1));
}
.nl-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 4px;
  margin-bottom: 12px;
}
.nl-title {
  font-size: 1em;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
}
.nl-head-acts {
  display: flex;
  align-items: center;
  gap: 6px;
}
/* 选中态：与筛选按钮同款，避免「回收站开着但看着没开」 */
.icon-btn.on {
  background: var(--brand-50);
  color: var(--brand-500);
}
.nl-badge {
  position: absolute;
  top: -3px;
  right: -3px;
  min-width: 15px;
  padding: 0 4px;
  font-size: 9px;
  line-height: 15px;
  text-align: center;
  color: var(--text-on-accent);
  background: var(--brand-500);
  border-radius: 999px;
  font-variant-numeric: tabular-nums;
}
.nl-head-acts .icon-btn {
  position: relative;
}
.nl-filtered {
  margin-left: 6px;
  padding: 1px 6px;
  font-size: 10px;
  font-weight: 500;
  color: var(--brand-500);
  background: var(--brand-50);
  border-radius: 999px;
}
.icon-btn.add {
  width: 30px;
  height: 30px;
  background: var(--brand-50);
  color: var(--brand-500);
}
.icon-btn.add:hover {
  background: var(--brand-500);
  color: var(--text-on-accent);
}

.nl-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

/* 标签筛选条 */
.tag-filter {
  padding: 0 4px 8px;
  margin-bottom: 4px;
}
.note-item {
  position: relative;
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: background 0.15s;
}
.note-item:hover {
  background: var(--bg-card-soft);
}
.note-item.active {
  background: var(--brand-50);
}
.note-item.active::before {
  content: '';
  position: absolute;
  left: 0;
  top: 10px;
  bottom: 10px;
  width: 3px;
  border-radius: 2px;
  background: var(--brand-500);
}
.note-item-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.note-title {
  font-size: 0.8125em;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.note-meta {
  font-size: 0.6875em;
  color: var(--text-3);
}
.note-summary {
  font-size: 0.75em;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.del {
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  opacity: 0;
  margin-top: -2px;
}
.note-item:hover .del,
.note-item:focus-within .del {
  opacity: 1;
}
.del:hover {
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
}
</style>

<!--
  非 scoped：这条类由 JS 加在 `document.body` 上，scoped 样式带 data-v 属性、
  匹配不到 body（组件根之外的节点）。
  拖拽期间禁止选中：指针划过笔记列表会把标题/摘要整片选蓝，
  而用户此刻的意图是「搬这条笔记」，不是「选一段文字」。
-->
<style>
body.note-dragging,
body.note-dragging * {
  user-select: none !important;
  cursor: grabbing !important;
}
</style>
