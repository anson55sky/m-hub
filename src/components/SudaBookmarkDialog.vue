<script setup lang="ts">
/**
 * 浏览器书签一键搬进速达（2026-10-03）。
 *
 * ## 与「扫描已安装应用」的三点差别
 *
 * ① **按文件夹逐层挑选、整组勾选** —— 书签是**有层级**的，而一次导入往往
 *    是几百条。文件夹节点可勾，勾上等于勾它里面全部（递归）。
 *    层级渲染在 `BmFolderNode.vue`（Chromium 的 JSON 是任意深度）。
 *
 * ② **浏览器目录 → 速达分类** —— 后端给每个书签带上 `category`（文件夹**全路径**）。
 *    ⚠️ 落进速达时只取**末级**（用户想看到的是「工作」而不是「书签栏/工作」），
 *    而全路径只用于**去重** —— 否则「书签栏/工作」与「其他书签/工作」会撞成
 *    同一条记录（同 URL 两次导入得到两个一样的条目）。
 *
 * ③ **已在速达的默认不勾** —— 重新扫一次时不该再导一遍。
 */
import { computed, ref, watch } from 'vue'
import { ChevronRight, Folder as FolderIcon, Loader2 } from 'lucide-vue-next'
import type { BookmarkNode, BrowserBookmarks } from '../api/tauri'
import { isTauri, tauriApi } from '../api/tauri'
import { useStore } from '../stores/workbench'
import BmFolderNode from './BmFolderNode.vue'

const props = defineProps<{ visible: boolean }>()
// ⚠️ 用**调用签名**式（本工程既有形状，见 SudaScanDialog），不用元组式
//   `{ picked: [items: X[]] }` —— 后者在本工程的 Vue 版本下类型推导不对，
//   表现为 `emit('picked', arr)` 报「X[] 不能赋给 (items) => void」。
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'picked', items: { name: string; url: string; category: string }[]): void
}>()

const store = useStore()
const browsers = ref<BrowserBookmarks[]>([])
const loading = ref(false)
const error = ref('')
const note = ref('')
const selected = ref(new Set<string>())
const expanded = ref(new Set<string>())
let noteTimer: ReturnType<typeof setTimeout> | undefined

/** URL 归一：去 fragment、去尾斜杠、host+path 转小写（macOS 文件系统大小写不敏感） */
function normUrl(u: string): string {
  try {
    const url = new URL(u)
    return `${url.origin}${url.pathname}`.replace(/\/$/, '').toLowerCase()
  } catch {
    return u.toLowerCase()
  }
}

/** 已在速达里的网页 */
const existing = computed(() => {
  const m = new Set<string>()
  for (const r of store.state.resources) {
    if (r.kind === 'web' && r.target) m.add(normUrl(r.target))
  }
  return m
})

/** 递归收集某节点下的叶子（书签条目），带它们所属的分类全路径 */
function leaves(n: BookmarkNode, cat = '', out: { node: BookmarkNode; category: string }[] = []) {
  if (n.url) {
    out.push({ node: n, category: cat })
    return out
  }
  const next = cat ? `${cat}/${n.name}` : n.name
  for (const c of n.children ?? []) leaves(c, next, out)
  return out
}

function countOf(n: BookmarkNode): number {
  return leaves(n).length
}

function isFolderOn(n: BookmarkNode): boolean {
  const ids = leaves(n).map((l) => l.node.id)
  return ids.length > 0 && ids.every((i) => selected.value.has(i))
}
function isFolderPartial(n: BookmarkNode): boolean {
  const ids = leaves(n).map((l) => l.node.id)
  return !isFolderOn(n) && ids.some((i) => selected.value.has(i))
}

function toggleFolder(n: BookmarkNode) {
  const ids = leaves(n).map((l) => l.node.id)
  const allOn = isFolderOn(n)
  const next = new Set(selected.value)
  for (const id of ids) {
    if (allOn) next.delete(id)
    else next.add(id)
  }
  selected.value = next
  expanded.value.add(n.id)
}

function toggleLeaf(id: string) {
  const next = new Set(selected.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  selected.value = next
}

function toggleExpand(id: string) {
  const next = new Set(expanded.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  expanded.value = next
}

function showNote(msg: string) {
  error.value = ''
  note.value = msg
  clearTimeout(noteTimer)
  noteTimer = setTimeout(() => (note.value = ''), 4000)
}

/** 勾选该浏览器里「还没在速达」的条目 —— 已导入的不重复勾 */
function selectNew(b: BrowserBookmarks) {
  const next = new Set(selected.value)
  let n = 0
  for (const root of b.roots) {
    for (const l of leaves(root)) {
      const u = l.node.url
      if (!u || existing.value.has(normUrl(u))) continue
      next.add(l.node.id)
      n++
    }
  }
  selected.value = next
  for (const root of b.roots) expanded.value.add(root.id)
  showNote(n > 0 ? `已勾选 ${n} 条未导入的` : '这个浏览器的书签都已在速达里了')
}

const picked = computed(() => {
  const out: { name: string; url: string; category: string }[] = []
  for (const b of browsers.value) {
    for (const root of b.roots) {
      for (const l of leaves(root)) {
        if (!l.node.url || !selected.value.has(l.node.id)) continue
        out.push({
          name: l.node.name,
          url: l.node.url,
          // 只取末级：用户想看到的是「工作」而不是「书签栏/工作」
          category: l.category.split('/').pop() ?? l.category,
        })
      }
    }
  }
  return out
})

/** 同一次导入里「同 URL + 同末级分类」只留一条 */
const uniquePicked = computed(() => {
  const seen = new Set<string>()
  const out: { name: string; url: string; category: string }[] = []
  for (const it of picked.value) {
    const k = `${normUrl(it.url)}|${it.category}`
    if (seen.has(k)) continue
    seen.add(k)
    out.push(it)
  }
  return out
})

async function load() {
  if (!isTauri()) return
  loading.value = true
  error.value = ''
  try {
    browsers.value = await tauriApi.readBrowserBookmarks()
    // 默认展开第一个浏览器的根 —— 用户一进来就该看到东西，而不是一片空白
    const first = browsers.value[0]
    if (first) for (const r of first.roots) expanded.value.add(r.id)
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

watch(
  () => props.visible,
  (v) => {
    if (!v) return
    selected.value = new Set()
    expanded.value = new Set()
    note.value = ''
    void load()
  },
  { immediate: true },
)
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="mask" @click.self="emit('close')">
      <div class="modal-card bm-card" role="dialog" aria-label="从浏览器导入书签">
        <header class="bm-head">
          <h3>从浏览器导入书签</h3>
          <button class="ghost-btn" type="button" @click="emit('close')">关闭</button>
        </header>

        <p class="bm-hint">
          按文件夹逐层挑选，勾文件夹等于勾它里面全部。<strong>浏览器里分好的目录会变成速达里的分类</strong>。
        </p>

        <div v-if="loading" class="bm-empty"><Loader2 :size="18" class="spin" /> 正在读取浏览器书签…</div>
        <div v-else-if="error" class="bm-empty">{{ error }}</div>

        <template v-else>
          <div v-if="note" class="bm-note">{{ note }}</div>
          <div class="bm-list">
            <section v-for="b in browsers" :key="b.browser + '/' + b.profile" class="bm-browser">
              <div class="bm-browser-head">
                <span class="bm-browser-name">{{ b.browser }}</span>
                <span class="bm-browser-meta">profile {{ b.profile }}</span>
                <button class="ghost-btn sm" type="button" @click="selectNew(b)">勾选全部未导入的</button>
              </div>

              <div v-for="root in b.roots" :key="root.id" class="bm-root">
                <div class="bm-root-row">
                  <button class="bm-caret" type="button" @click="toggleExpand(root.id)">
                    <ChevronRight :size="13" :class="{ open: expanded.has(root.id) }" />
                  </button>
                  <input
                    type="checkbox"
                    :checked="isFolderOn(root)"
                    :indeterminate="isFolderPartial(root)"
                    @change="toggleFolder(root)"
                  />
                  <FolderIcon :size="14" :stroke-width="1.9" class="bm-ic" />
                  <span class="bm-name">{{ root.name }}</span>
                  <span class="bm-count">{{ countOf(root) }}</span>
                </div>
                <BmFolderNode
                  v-for="c in root.children ?? []"
                  v-show="expanded.has(root.id)"
                  :key="c.id"
                  :node="c"
                  :depth="1"
                  :selected="selected"
                  :expanded="expanded"
                  @toggle-folder="toggleFolder"
                  @toggle-leaf="toggleLeaf"
                  @toggle-expand="toggleExpand"
                />
              </div>
            </section>
          </div>
        </template>

        <footer class="bm-foot">
          <span class="bm-sel">
            已选 <strong>{{ uniquePicked.length }}</strong> 条
            <span v-if="picked.length !== uniquePicked.length" class="bm-dup">
              （重复 {{ picked.length - uniquePicked.length }} 条已合并）
            </span>
          </span>
          <button
            class="primary-btn"
            type="button"
            :disabled="!uniquePicked.length"
            @click="emit('picked', uniquePicked)"
          >
            导入速达（{{ uniquePicked.length }}）
          </button>
        </footer>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.bm-card {
  width: min(640px, calc(100vw - 64px));
  max-height: min(74vh, 660px);
  display: flex;
  flex-direction: column;
  padding: 0;
}
.bm-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 18px 0;
}
.bm-head h3 {
  margin: 0;
  font-size: 15px;
  font-weight: 650;
}
.bm-hint {
  margin: 6px 18px 10px;
  font-size: 12.5px;
  color: var(--text-2);
  line-height: 1.55;
}
.bm-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 36px 0;
  color: var(--text-3);
  font-size: 13px;
}
.bm-note {
  margin: 0 18px 8px;
  font-size: 12px;
  color: var(--brand-600);
}
.bm-list {
  flex: 1;
  overflow-y: auto;
  padding: 0 10px;
  min-height: 120px;
}
.bm-browser + .bm-browser {
  margin-top: 10px;
  border-top: 1px solid var(--border-soft);
  padding-top: 8px;
}
.bm-browser-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 6px 6px;
}
.bm-browser-name {
  font-size: 13px;
  font-weight: 600;
}
.bm-browser-meta {
  font-size: 11.5px;
  color: var(--text-3);
  margin-right: auto;
}
.ghost-btn.sm {
  font-size: 12px;
  padding: 3px 9px;
}
.bm-root-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 6px;
  border-radius: 6px;
  font-size: 13px;
}
.bm-root-row:hover {
  background: var(--bg-card-soft);
}
.bm-caret {
  background: none;
  border: 0;
  padding: 0;
  width: 14px;
  height: 14px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
}
.bm-caret svg {
  transition: transform 0.15s;
}
.bm-caret svg.open {
  transform: rotate(90deg);
}
.bm-ic {
  color: var(--text-3);
  flex-shrink: 0;
}
.bm-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bm-count {
  font-size: 11px;
  color: var(--text-3);
}
.bm-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 12px 18px 16px;
}
.bm-sel {
  font-size: 12.5px;
  color: var(--text-2);
}
.bm-dup {
  color: var(--text-3);
}
.primary-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.spin {
  animation: bm-spin 0.9s linear infinite;
}
@keyframes bm-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>