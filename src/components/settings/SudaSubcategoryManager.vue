<script setup lang="ts">
// 速达小类管理（ADR 0012）：按大类分组的小类库管理——行内改名、拖拽排序、设默认、删除
// （删除时条目自动改挂默认小类，后端单事务）。速达页只放小类选择器，管理集中在这里。
import { computed, onBeforeUnmount, ref } from 'vue'
import { GripVertical, Plus, Star, Trash2 } from 'lucide-vue-next'
import { useStore } from '../../stores/workbench'

const store = useStore()

const KINDS: { kind: 'app' | 'web' | 'file'; label: string; hint: string }[] = [
  { kind: 'app', label: '应用', hint: '扫描/手动添加的程序' },
  { kind: 'web', label: '网页', hint: '网址收藏' },
  { kind: 'file', label: '文件', hint: '已内置 7 个初始小类' },
]

/** 每大类一条新增输入框的草稿 */
const drafts = ref<Record<string, string>>({})
const newName = (kind: string) => drafts.value[kind] ?? ''
function setNewName(kind: string, v: string) {
  drafts.value = { ...drafts.value, [kind]: v }
}

const error = ref('')

async function add(kind: 'app' | 'web' | 'file') {
  const name = newName(kind).trim()
  if (!name) return
  try {
    await store.addSubcategory(kind, name)
    setNewName(kind, '')
    error.value = ''
  } catch (e) {
    error.value = String(e).replace(/^DUP:/, '').trim() || String(e)
  }
}

/** 行内改名：blur/回车提交；未变化或为空则还原 */
async function rename(sub: { id: number; name: string }, ev: Event) {
  const el = ev.target as HTMLInputElement
  const next = el.value.trim()
  if (!next || next === sub.name) {
    el.value = sub.name
    return
  }
  try {
    await store.editSubcategory(sub.id, next)
    error.value = ''
  } catch (e) {
    el.value = sub.name
    error.value = String(e).replace(/^DUP:/, '').trim() || String(e)
  }
}

/** 两段式删除确认：第一次点击变红确认态，3s 内再点才真删 */
const confirmDeleteId = ref<number | null>(null)
let confirmTimer = 0
function onDeleteClick(id: number) {
  if (confirmDeleteId.value === id) {
    confirmDeleteId.value = null
    window.clearTimeout(confirmTimer)
    store
      .removeSubcategory(id)
      .then(() => (error.value = ''))
      .catch((e) => (error.value = String(e)))
    return
  }
  confirmDeleteId.value = id
  window.clearTimeout(confirmTimer)
  confirmTimer = window.setTimeout(() => (confirmDeleteId.value = null), 3000)
}

async function onSetDefault(id: number) {
  try {
    await store.setDefaultSubcategory(id)
    error.value = ''
  } catch (e) {
    error.value = String(e)
  }
}

// ---- 拖拽排序（指针实现；主窗口原生拖放拦截与 HTML5 DnD 互斥，见约定 14） ----
const dragKind = ref<'app' | 'web' | 'file' | null>(null)
/** 拖拽中的本地预览顺序（仅拖拽的那个大类），提交后清空回读 store */
const preview = ref<Record<string, number[]>>({})
let dragFromIndex = 0

function displayIds(kind: 'app' | 'web' | 'file'): number[] {
  const override = preview.value[kind]
  if (override) return override
  return store.subcategoriesOf(kind).map((s) => s.id)
}

function onHandleDown(e: PointerEvent, kind: 'app' | 'web' | 'file', index: number) {
  e.preventDefault()
  dragKind.value = kind
  dragFromIndex = index
  window.addEventListener('pointermove', onDragMove)
  window.addEventListener('pointerup', onDragUp)
}

function onDragMove(e: PointerEvent) {
  const kind = dragKind.value
  if (!kind) return
  const el = document.elementFromPoint(e.clientX, e.clientY)?.closest('[data-sub-row]')
  if (!(el instanceof HTMLElement)) return
  if (el.dataset.kind !== kind) return
  const to = Number(el.dataset.index)
  const ids = displayIds(kind)
  if (Number.isNaN(to) || to === dragFromIndex || to < 0 || to >= ids.length) return
  const next = [...ids]
  next.splice(dragFromIndex, 1)
  next.splice(to, 0, ids[dragFromIndex])
  dragFromIndex = to
  preview.value = { ...preview.value, [kind]: next }
}

function onDragUp() {
  const kind = dragKind.value
  window.removeEventListener('pointermove', onDragMove)
  window.removeEventListener('pointerup', onDragUp)
  dragKind.value = null
  if (!kind) return
  const ids = displayIds(kind)
  preview.value = {}
  store
    .reorderSubcategories(kind, ids)
    .then(() => (error.value = ''))
    .catch((e) => (error.value = String(e)))
}

onBeforeUnmount(() => {
  window.removeEventListener('pointermove', onDragMove)
  window.removeEventListener('pointerup', onDragUp)
  window.clearTimeout(confirmTimer)
})

/** 展示列表：预览序优先（仅拖拽中的那个大类），其余按 store 排序 */
const groups = computed(() =>
  KINDS.map(({ kind, label, hint }) => {
    const ids = displayIds(kind)
    const map = new Map(store.subcategoriesOf(kind).map((s) => [s.id, s]))
    const list = ids.map((id) => map.get(id)).filter((s): s is NonNullable<typeof s> => !!s)
    return { kind, label, hint, list }
  }),
)
</script>

<template>
  <div class="sub-mgr">
    <p v-if="error" class="sub-mgr-error">{{ error }}</p>
    <div v-for="g in groups" :key="g.kind" class="sub-mgr-group">
      <div class="sub-mgr-head">
        <span class="sub-mgr-title">{{ g.label }}</span>
        <span class="sub-mgr-hint">{{ g.hint }}</span>
      </div>
      <div
        v-for="(s, i) in g.list"
        :key="s.id"
        class="sub-mgr-row"
        data-sub-row
        :data-kind="g.kind"
        :data-index="i"
      >
        <span
          class="sub-mgr-drag"
          title="拖拽排序"
          @pointerdown="onHandleDown($event, g.kind, i)"
        >
          <GripVertical :size="13" :stroke-width="2" />
        </span>
        <input
          class="sub-mgr-name"
          :value="s.name"
          spellcheck="false"
          @change="rename(s, $event)"
          @keydown.enter="($event.target as HTMLInputElement).blur()"
        />
        <button
          class="sub-mgr-btn"
          :class="{ 'is-default': s.is_default }"
          type="button"
          :title="s.is_default ? '默认小类：新建资源未指定时自动归入（点击换默认）' : '设为默认小类'"
          @click="onSetDefault(s.id)"
        >
          <Star :size="13" :stroke-width="2.2" />
        </button>
        <button
          class="sub-mgr-btn"
          :class="{ 'is-confirm': confirmDeleteId === s.id }"
          type="button"
          :title="
            confirmDeleteId === s.id
              ? '再点一次确认删除（条目将改挂默认小类）'
              : '删除小类（条目改挂默认小类）'
          "
          @click="onDeleteClick(s.id)"
        >
          <Trash2 :size="13" :stroke-width="2.2" />
        </button>
      </div>
      <div class="sub-mgr-row sub-mgr-add">
        <span class="sub-mgr-drag is-placeholder"><Plus :size="13" :stroke-width="2" /></span>
        <input
          class="sub-mgr-name"
          :value="newName(g.kind)"
          type="text"
          placeholder="新增小类名称，回车添加"
          spellcheck="false"
          @input="setNewName(g.kind, ($event.target as HTMLInputElement).value)"
          @keydown.enter="add(g.kind)"
        />
        <button class="sub-mgr-btn sub-mgr-add-btn" type="button" title="添加小类" @click="add(g.kind)">
          添加
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.sub-mgr {
  display: flex;
  flex-direction: column;
  gap: 14px;
  margin-top: 4px;
}

.sub-mgr-error {
  margin: 0;
  font-size: 12px;
  color: var(--c-red);
}

.sub-mgr-group {
  border: 1px solid var(--border-soft);
  border-radius: 10px;
  padding: 10px 12px 12px;
  background: var(--bg-card-soft);
}

.sub-mgr-head {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin-bottom: 8px;
}

.sub-mgr-title {
  font-size: 13px;
  font-weight: 650;
  color: var(--text-1);
}

.sub-mgr-hint {
  font-size: 11px;
  color: var(--text-3);
}

.sub-mgr-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 0;
}

.sub-mgr-drag {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 22px;
  color: var(--text-4);
  cursor: grab;
  touch-action: none;
}

.sub-mgr-drag:active {
  cursor: grabbing;
}

.sub-mgr-drag.is-placeholder {
  cursor: default;
  opacity: 0.45;
}

.sub-mgr-name {
  flex: 1;
  min-width: 0;
  height: 28px;
  padding: 0 10px;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  color: var(--text-1);
  font-size: 12.5px;
  outline: none;
  transition: border-color 0.15s, background 0.15s;
}

.sub-mgr-name:hover {
  background: var(--bg-card);
}

.sub-mgr-name:focus {
  border-color: var(--brand-500);
  background: var(--bg-card);
  box-shadow: 0 0 0 2px var(--brand-50);
}

.sub-mgr-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--text-4);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}

.sub-mgr-btn:hover {
  background: var(--bg-card);
  color: var(--text-2);
}

.sub-mgr-btn.is-default {
  color: var(--c-yellow);
}

.sub-mgr-btn.is-confirm {
  background: var(--c-red-soft, var(--c-red));
  color: #fff;
}

.sub-mgr-add {
  margin-top: 4px;
}

.sub-mgr-add .sub-mgr-name {
  border-color: var(--border-soft);
  background: var(--bg-card);
}

.sub-mgr-add-btn {
  width: auto;
  padding: 0 10px;
  font-size: 11.5px;
  color: var(--text-2);
  border: 1px solid var(--border-soft);
}
</style>
