<script setup lang="ts">
// 速达小类管理（ADR 0012）：按大类分组的小类库管理——行内改名、拖拽排序、设默认、删除
// （删除时条目自动改挂默认小类，后端单事务）。速达页只放小类选择器，管理集中在这里。
import { computed, onBeforeUnmount, ref } from 'vue'
import { ChevronRight, CornerDownRight, GripVertical, Plus, Star, Trash2 } from 'lucide-vue-next'
import type { ResourceSubcategory, SubcategoryNode } from '../../api/tauri'
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

/** 每行一条「新建子级」的输入框草稿（键 = 父小类 id） */
const childDrafts = ref<Record<number, string>>({})
const childTarget = ref<number | null>(null)

function openChildInput(parentId: number) {
  childDrafts.value[parentId] = ''
  childTarget.value = parentId
}

function closeChildInput() {
  childTarget.value = null
}

async function addChild(kind: 'app' | 'web' | 'file', parentId: number) {
  const name = (childDrafts.value[parentId] ?? '').trim()
  if (!name) return
  try {
    // ⚠️ 走 `createSubcategoryChild`（传父 id）而不是自己拼「父路径/名字」：
    //   路径规则只有 Rust 那一份，前端拼错会得到一个挂在不存在节点下的孤儿小类
    await store.addSubcategoryChild(kind, name, parentId)
    // ⚠️ addSubcategoryChild 只就地插平表（书签导入要逐段建，逐段重拉会变成
    //   O(n) 次全量往返），**树**还是旧的 —— 这里补一次重拉，否则新行不出现
    await store.refreshSubcategories()
    const next = new Set(expandedPaths.value)
    // 建完立刻展开：否则新行落在收起状态，用户会以为没建成功
    const parent = store.subcategoriesOf(kind).find((x) => x.id === parentId)
    if (parent) next.add(parent.name)
    expandedPaths.value = next
    childDrafts.value[parentId] = ''
    childTarget.value = null
    error.value = ''
  } catch (e) {
    error.value = String(e)
  }
}

// ---- 批量删除（2026-10-04）----
//
// 与速达批量删除同款：多选态与「点行改名」**互斥** —— 进多选后行内的
// 改名输入框与删除按钮都让位给复选框，否则两套手势在同一个 30px 高的行里
// 打架（点复选框可能触发行内的 blur 提交改名）。
const bulkMode = ref(false)
const bulkSel = ref(new Set<number>())

function toggleBulkSel(id: number) {
  const next = new Set(bulkSel.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  bulkSel.value = next
}

function toggleBulkAll(kind: 'app' | 'web' | 'file') {
  const ids = store.state.resourceSubcategories.filter((s) => s.kind === kind).map((s) => s.id)
  const allOn = ids.length > 0 && ids.every((id) => bulkSel.value.has(id))
  const next = new Set(bulkSel.value)
  for (const id of ids) {
    if (allOn) next.delete(id)
    else next.add(id)
  }
  bulkSel.value = next
}

function exitBulk() {
  bulkMode.value = false
  bulkSel.value = new Set()
}

const bulkBusy = ref(false)
/**
 * 批量删：同样走**两段式确认**。
 * ⚠️ 批量删除的影响面比单个大得多（一次可能改挂几十条资源），
 *   所以第一下只进入确认态、把选中数量写出来，第二下才真删。
 */
const bulkArmed = ref(false)
function onBulkDeleteClick() {
  if (!bulkSel.value.size) return
  if (!bulkArmed.value) {
    bulkArmed.value = true
    window.clearTimeout(bulkTimer)
    bulkTimer = window.setTimeout(() => (bulkArmed.value = false), 3000)
    return
  }
  bulkArmed.value = false
  window.clearTimeout(bulkTimer)
  const ids = [...bulkSel.value]
  bulkBusy.value = true
  store
    .removeSubcategories(ids)
    .then(({ ok, failed }) => {
      if (!failed.length) {
        exitBulk()
      } else {
        // 失败的那些留在选中态：用户能立刻看到是哪几个没删掉
        bulkSel.value = new Set(failed.map((f) => f.id))
      }
      error.value = failed.length ? `已删除 ${ok.length} 个，${failed.length} 个失败：${failed[0]?.error ?? ''}` : ''
    })
    .catch((e) => (error.value = String(e)))
    .finally(() => (bulkBusy.value = false))
}
let bulkTimer = 0
onBeforeUnmount(() => {
  window.clearTimeout(confirmTimer)
  window.clearTimeout(bulkTimer)
})

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
/** 一行 = 树里的一个小类 + 它的层级与展开态 */
interface SubRow {
  sub: ResourceSubcategory
  depth: number
  hasKids: boolean
  expanded: boolean
  /** 是否带子级（决定行内要不要显示「新建子级」按钮） */
  isParent: boolean
}

/** 展开了哪些层级（存全路径） */
const expandedPaths = ref<Set<string>>(new Set())

function toggleExpand(path: string) {
  const next = new Set(expandedPaths.value)
  if (next.has(path)) next.delete(path)
  else next.add(path)
  expandedPaths.value = next
}

/**
 * 把小类树摊成带层级的行。
 *
 * ⚠️ **只有顶层**按用户拖拽出来的顺序排（`displayIds`）；子级按各自的
 *   `sort_order` 排。把子级也塞进拖拽顺序的话，「拖一下子级」就会把它
 *   拖到别的父级下面去 —— 那是「跨父级改挂」，是一条独立功能，
 *   本轮不做，所以干脆不给子级拖拽把手（见模板）。
 */
function flattenTree(kind: 'app' | 'web' | 'file', order: number[]): SubRow[] {
  const flat = new Map(store.subcategoriesOf(kind).map((s) => [s.id, s]))
  const out: SubRow[] = []
  const walk = (nodes: SubcategoryNode[], depth: number) => {
    for (const n of nodes) {
      const sub = flat.get(n.id)
      if (!sub) continue
      const expanded = expandedPaths.value.has(n.fullPath)
      out.push({
        sub,
        depth,
        hasKids: n.children.length > 0,
        expanded,
        isParent: n.children.length > 0,
      })
      if (expanded) walk(n.children, depth + 1)
    }
  }
  // 顶层按拖拽顺序重排（其余按 sort_order）
  const tree = [...store.subcategoryTree(kind)]
  const rank = new Map(order.map((id, i) => [id, i]))
  tree.sort(
    (a, b) =>
      (rank.get(a.id) ?? Number.MAX_SAFE_INTEGER) - (rank.get(b.id) ?? Number.MAX_SAFE_INTEGER),
  )
  walk(tree, 0)
  return out
}

const groups = computed(() =>
  KINDS.map(({ kind, label, hint }) => {
    const ids = displayIds(kind)
    return { kind, label, hint, list: flattenTree(kind, ids), flatIds: ids }
  }),
)
</script>

<template>
  <div class="sub-mgr">
    <div class="sub-mgr-toolbar">
      <button class="ghost-btn sm" type="button" @click="bulkMode ? exitBulk() : (bulkMode = true)">
        {{ bulkMode ? '退出批量' : '批量删除小类' }}
      </button>
      <template v-if="bulkMode">
        <span class="sub-mgr-bulk-count">已选 {{ bulkSel.size }} 个</span>
        <button
          class="ghost-btn sm danger"
          type="button"
          :class="{ armed: bulkArmed }"
          :disabled="!bulkSel.size || bulkBusy"
          @click="onBulkDeleteClick"
        >
          {{ bulkBusy ? '删除中…' : bulkArmed ? `再点一次确认删除这 ${bulkSel.size} 个` : '删除所选' }}
        </button>
      </template>
    </div>
    <p v-if="error" class="sub-mgr-error">{{ error }}</p>
    <div v-for="g in groups" :key="g.kind" class="sub-mgr-group">
      <div class="sub-mgr-head">
        <span class="sub-mgr-title">{{ g.label }}</span>
        <span class="sub-mgr-hint">{{ g.hint }}</span>
        <button class="ghost-btn sm sub-mgr-bulk" type="button" @click="toggleBulkAll(g.kind)">
          {{ bulkMode && g.list.length && g.list.every((x) => bulkSel.has(x.sub.id)) ? '取消全选' : '全选本组' }}
        </button>
      </div>
      <!--
        ⚠️ `data-index` 传的是**顶层行在 g.flatIds 里的下标**，不是行号 ——
           拖拽落位要写回的 `sort_order` 是全组的 id 顺序，子级不参与
           （见 flattenTree 的注释）。用行号的话，展开后拖第二行会写错位置。
      -->
      <div
        v-for="row in g.list"
        :key="row.sub.id"
        class="sub-mgr-row"
        :class="{ 'is-child': row.depth > 0, 'is-collapsed': row.hasKids && !row.expanded }"
        :style="{ '--sub-depth': row.depth }"
        data-sub-row
        :data-kind="g.kind"
        :data-id="row.sub.id"
        :data-index="g.flatIds.indexOf(row.sub.id)"
      >
        <!-- 多选态：拖拽把手让位给复选框（两套手势在同一行里会打架） -->
        <input
          v-if="bulkMode"
          class="sub-mgr-check"
          type="checkbox"
          :checked="bulkSel.has(row.sub.id)"
          :aria-label="`选择小类 ${row.sub.name}`"
          @change="toggleBulkSel(row.sub.id)"
        />
        <!-- 展开/收起：只折叠，不改筛选；有子级才显示 -->
        <button
          v-else-if="row.hasKids"
          class="sub-mgr-drag is-twisty"
          type="button"
          :aria-label="(row.expanded ? '收起 ' : '展开 ') + row.sub.name"
          :aria-expanded="row.expanded"
          @click="toggleExpand(row.sub.name)"
        >
          <ChevronRight :size="13" :stroke-width="2.2" :class="{ 'is-open': row.expanded }" />
        </button>
        <span v-else-if="row.depth > 0" class="sub-mgr-drag is-twisty is-spacer" aria-hidden="true" />
        <!--
          ⚠️ 子级**不给拖拽把手**：拖拽排序写的是全组顺序，让它跨到别的父级
          下就是「改挂」——那是独立功能，这轮不做。与其做一个半对的手势，
          不如不给（顶层仍有把手，拖的是顶层顺序）。
        -->
        <span
          v-else
          class="sub-mgr-drag"
          title="拖拽排序"
          @pointerdown="onHandleDown($event, g.kind, g.flatIds.indexOf(row.sub.id))"
        >
          <GripVertical :size="13" :stroke-width="2" />
        </span>
        <input
          class="sub-mgr-name"
          :value="row.sub.name"
          :title="row.depth > 0 ? `上一级：${row.sub.name.split('/').slice(0, -1).join(' / ')}` : '顶层小类'"
          spellcheck="false"
          @change="rename(row.sub, $event)"
          @keydown.enter="($event.target as HTMLInputElement).blur()"
        />
        <!-- 在这一层下面新建子级 -->
        <button
          class="sub-mgr-btn"
          type="button"
          title="在这一层下面新建子级"
          @click="openChildInput(row.sub.id)"
        >
          <CornerDownRight :size="13" :stroke-width="2.2" />
        </button>
        <button
          class="sub-mgr-btn"
          :class="{ 'is-default': row.sub.is_default }"
          type="button"
          :title="
            row.sub.is_default
              ? '默认小类：新建资源未指定时自动归入（点击换默认）'
              : '设为默认小类'
          "
          @click="onSetDefault(row.sub.id)"
        >
          <Star :size="13" :stroke-width="2.2" />
        </button>
        <button
          class="sub-mgr-btn"
          :class="{ 'is-confirm': confirmDeleteId === row.sub.id }"
          type="button"
          :title="
            confirmDeleteId === row.sub.id
              ? row.hasKids
                ? '再点一次确认删除（这一层及其下级都会删掉，条目改挂默认小类）'
                : '再点一次确认删除（条目将改挂默认小类）'
              : row.hasKids
                ? '删除这一层及其下级（条目改挂默认小类）'
                : '删除小类（条目改挂默认小类）'
          "
          @click="onDeleteClick(row.sub.id)"
        >
          <Trash2 :size="13" :stroke-width="2.2" />
        </button>
      </div>
      <div
        v-for="row in g.list"
        v-show="row.sub.id === childTarget"
        :key="'child-' + row.sub.id"
        class="sub-mgr-row sub-mgr-add is-child"
        :style="{ '--sub-depth': (row.depth + 1) }"
      >
        <span class="sub-mgr-drag is-placeholder">
          <CornerDownRight :size="13" :stroke-width="2" />
        </span>
        <input
          class="sub-mgr-name"
          :value="childDrafts[row.sub.id] ?? ''"
          type="text"
          :placeholder="`在「${row.sub.name}」下新建子级，回车添加`"
          spellcheck="false"
          autofocus
          @input="childDrafts[row.sub.id] = ($event.target as HTMLInputElement).value"
          @keydown.enter="addChild(g.kind, row.sub.id)"
          @keydown.esc="closeChildInput()"
          @blur="closeChildInput()"
        />
        <button class="sub-mgr-btn sub-mgr-add-btn" type="button" title="添加子级" @click="addChild(g.kind, row.sub.id)">
          添加
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

/* ---- 批量删除小类（2026-10-04） ---- */
.sub-mgr-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 6px 0 8px;
}
.sub-mgr-bulk-count {
  font-size: 12px;
  color: var(--text-2);
  margin-right: auto;
}
.sub-mgr-bulk {
  margin-left: auto;
}
.sub-mgr-check {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
  margin: 0 4px;
  cursor: pointer;
}
.ghost-btn.sm.danger {
  color: var(--c-red);
  border-color: var(--c-red);
}
.ghost-btn.sm.danger.armed {
  background: var(--c-red);
  color: #fff;
  border-color: var(--c-red);
}
.ghost-btn.sm:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
/* ---- 层级小类：缩进 / 折叠箭头（2026-10-04） ---- */
.sub-mgr-row {
  padding-left: calc(var(--sub-depth, 0) * 14px);
}
.sub-mgr-drag.is-twisty {
  cursor: pointer;
  color: var(--text-3);
}
.sub-mgr-drag.is-twisty:hover {
  color: var(--text-1);
}
.sub-mgr-drag.is-twisty.is-spacer {
  cursor: default;
}
.sub-mgr-drag.is-twisty svg {
  transition: transform 0.15s ease;
}
.sub-mgr-drag.is-twisty svg.is-open {
  transform: rotate(90deg);
}
.sub-mgr-row.is-collapsed .sub-mgr-name {
  color: var(--text-2);
}

</style>
