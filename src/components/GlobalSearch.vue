<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, toRef, watch } from 'vue'
import { Search } from 'lucide-vue-next'
import type { SearchResult, Resource, Note, Todo, Snippet, Countdown } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { useFocusTrap } from '../composables/useFocusTrap'
import { markdownPlainText } from '../utils/markdown'
import { shortcutLabel } from '../utils/platform'

const props = defineProps<{
  visible: boolean
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'openResource', r: Resource): void
  (e: 'openNote', n: Note): void
  (e: 'openTodo', t: Todo): void
  /** 2026-09-29 新增：搜索结果里点提示词/倒计时 */
  (e: 'openSnippet', s: Snippet): void
  (e: 'openCountdown', c: Countdown): void
}>()

const store = useStore()

/**
 * 弹窗内展示当前生效的呼出快捷键。
 *
 * ⚠️ 此前是 `config.search_shortcut || 'Ctrl+K'` —— 唯一**没有**走 `prettyShortcut`
 * 的展示点。而配置里存的是内部写法 `CommandOrControl+K`，于是用户刚用 ⌘K
 * 唤起搜索、弹窗里却写着 `CommandOrControl+K`，而标题栏 tooltip 写着 `⌘K`。
 * 用户会以为要照着输入一串字面量。
 */
const searchShortcutLabel = computed(() => shortcutLabel('search', store.state.config.search_shortcut))

const keyword = ref('')
/**
 * 用共享的 `SearchResult` 而不是就地写一份字段列表。
 * 就地写的那份是**第二份拷贝**：后端加了字段，前端这个 ref 不会跟着变，
 * 报错会以「Property 'snippets' does not exist」的形式出现在**用到它的那行**，
 * 而不是出现在真正该改的地方。
 */
const EMPTY_RESULTS: SearchResult = {
  resources: [],
  notes: [],
  todos: [],
  snippets: [],
  countdowns: [],
}
const results = ref<SearchResult>({ ...EMPTY_RESULTS })
const searched = ref(false)
const inputRef = ref<HTMLInputElement | null>(null)
const cardRef = ref<HTMLElement | null>(null)

useFocusTrap(toRef(props, 'visible'), cardRef, inputRef)

type ResultType = 'resource' | 'note' | 'todo' | 'snippet' | 'countdown'

interface FlatItem {
  type: ResultType
  key: string
  /** 分组名，渲染用。放在数据里而不是模板里拼，模板就不用重复写五遍分组标题 */
  group: string
  resource?: Resource
  note?: Note
  todo?: Todo
  snippet?: Snippet
  countdown?: Countdown
}

/**
 * 把五类结果拍平成一条有序列表。
 *
 * 之所以要有这个「拍平」层：模板此前对每一组都在手写
 * `results.resources.length + results.notes.length + idx` 这种**累加偏移**
 * 来算 `activeIndex`。加一组结果就得把这五个表达式全改一遍，
 * 而漏改一处不会报错 —— 只会让键盘上下键在某一组里选错条目。
 * 改成「模板只遍历 flatResults，索引就是 `i`」之后，
 * 新增一类结果只需要在**这一个**数组里加一行。
 */
const flatResults = computed<FlatItem[]>(() => [
  ...results.value.resources.map((r) => ({ type: 'resource' as const, group: '速达', key: 'r' + r.id, resource: r })),
  ...results.value.notes.map((n) => ({ type: 'note' as const, group: '速记', key: 'n' + n.id, note: n })),
  ...results.value.todos.map((t) => ({ type: 'todo' as const, group: '待办', key: 't' + t.id, todo: t })),
  // 2026-09-29 新增：提示词与倒计时此前搜不到
  ...results.value.snippets.map((s) => ({ type: 'snippet' as const, group: '提示词', key: 's' + s.id, snippet: s })),
  ...results.value.countdowns.map((c) => ({ type: 'countdown' as const, group: '倒计时', key: 'c' + c.id, countdown: c })),
])

const activeIndex = ref(-1)

watch(flatResults, (list) => {
  activeIndex.value = list.length > 0 ? 0 : -1
})

function scrollActiveIntoView() {
  const item = flatResults.value[activeIndex.value]
  if (!item) return
  document
    .querySelector<HTMLElement>(`[data-search-key="${item.key}"]`)
    ?.scrollIntoView({ block: 'nearest' })
}

/** 鼠标点某一条：先把 activeIndex 移到它，再走与回车相同的打开逻辑。
 *  写成「复用 openActive」而不是各自 emit，是为了让点击与回车**不可能**分叉。 */
function openActiveAt(i: number) {
  activeIndex.value = i
  openActive()
}

function openActive() {
  const item = flatResults.value[activeIndex.value]
  if (!item) return
  if (item.type === 'resource' && item.resource) emit('openResource', item.resource)
  else if (item.type === 'note' && item.note) emit('openNote', item.note)
  else if (item.type === 'todo' && item.todo) emit('openTodo', item.todo)
  // 2026-09-29 新增两类
  else if (item.type === 'snippet' && item.snippet) emit('openSnippet', item.snippet)
  else if (item.type === 'countdown' && item.countdown) emit('openCountdown', item.countdown)
}

function onKeydown(e: KeyboardEvent) {
  if (!props.visible) return
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('close')
    return
  }
  const list = flatResults.value
  if (list.length === 0) return
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    activeIndex.value = (activeIndex.value + 1) % list.length
    scrollActiveIntoView()
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    activeIndex.value = (activeIndex.value - 1 + list.length) % list.length
    scrollActiveIntoView()
  } else if (e.key === 'Enter') {
    e.preventDefault()
    openActive()
  }
}

let searchTimer: ReturnType<typeof setTimeout> | null = null

watch(
  () => props.visible,
  (v) => {
    if (v) {
      keyword.value = ''
      results.value = { ...EMPTY_RESULTS }
      searched.value = false
      activeIndex.value = -1
    }
  },
)

watch(keyword, (kw) => {
  if (searchTimer) clearTimeout(searchTimer)
  const trimmed = kw.trim()
  if (!trimmed) {
    results.value = { ...EMPTY_RESULTS }
    searched.value = false
    return
  }
  searchTimer = setTimeout(async () => {
    results.value = await store.searchAll(trimmed)
    searched.value = true
  }, 300)
})

function kindText(kind: Resource['kind']): string {
  if (kind === 'app') return '程序'
  if (kind === 'web') return '网页'
  return '文件'
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask">
        <div class="search-card" ref="cardRef" role="dialog" aria-label="全局搜索" aria-modal="true">
          <div class="search-input-row">
            <Search class="search-icon" :size="17" :stroke-width="1.8" />
            <input
              ref="inputRef"
              v-model="keyword"
              class="search-input"
              type="text"
              placeholder="搜索资源与笔记…"
            />
            <kbd class="esc-hint" :title="`按 ${searchShortcutLabel} 可随时开关搜索`">{{ searchShortcutLabel }}</kbd>
            <kbd class="esc-hint">ESC</kbd>
          </div>

          <div class="search-results" role="listbox" aria-label="搜索结果">
            <!--
              单一数据源：遍历 flatResults，分组靠「上一条的 group 与当前不同」来插标题。
              此前每组各写一遍模板，并在 activeIndex 上手写累加偏移
              （results.resources.length + results.notes.length + idx …）——
              加一组结果要改五处，漏一处不会报错，只会让键盘选中错行。
            -->
            <template v-for="(item, i) in flatResults" :key="item.key">
              <p
                v-if="i === 0 || flatResults[i - 1].group !== item.group"
                class="result-group-title"
                role="presentation"
              >
                {{ item.group }}
              </p>
              <div
                class="result-item"
                :class="{ active: i === activeIndex }"
                :data-search-key="item.key"
                role="option"
                :aria-selected="i === activeIndex"
                @click="openActiveAt(i)"
                @mouseenter="activeIndex = i"
              >
                <!-- 速达 -->
                <template v-if="item.resource">
                  <span class="result-badge" :class="item.resource.kind">
                    {{ kindText(item.resource.kind) }}
                  </span>
                  <span class="result-name" :title="item.resource.name">
                    {{ item.resource.name }}
                  </span>
                  <span class="result-sub">{{ item.resource.target }}</span>
                </template>
                <!-- 速记 -->
                <template v-else-if="item.note">
                  <span class="result-badge note-badge">笔记</span>
                  <span class="result-name" :title="item.note.title">{{ item.note.title }}</span>
                  <span class="result-sub">{{ markdownPlainText(item.note.content, 60) }}</span>
                </template>
                <!-- 待办 -->
                <template v-else-if="item.todo">
                  <span class="result-badge todo-badge">
                    {{ item.todo.done ? '已完成' : '待完成' }}
                  </span>
                  <span class="result-name" :title="item.todo.title">{{ item.todo.title }}</span>
                  <span class="result-sub">
                    {{ ['普通', '重要', '紧急'][item.todo.priority] ?? '普通' }}优先级
                  </span>
                </template>
                <!-- 提示词（2026-09-29 新增） -->
                <template v-else-if="item.snippet">
                  <span class="result-badge">提示词</span>
                  <span class="result-name" :title="item.snippet.title">
                    {{ item.snippet.title }}
                  </span>
                  <span class="result-sub">{{ item.snippet.content }}</span>
                </template>
                <!-- 倒计时（2026-09-29 新增） -->
                <template v-else-if="item.countdown">
                  <span class="result-badge">倒计时</span>
                  <span class="result-name" :title="item.countdown.name">
                    {{ item.countdown.name }}
                  </span>
                  <span class="result-sub">
                    {{ item.countdown.finished ? '已结束' : '进行中' }}
                  </span>
                </template>
              </div>
            </template>

            <!-- 状态 -->
            <div v-if="searched && flatResults.length === 0" class="empty-state">
              <span style="font-size: 1.625rem">🔍</span>
              <p>未找到「{{ keyword.trim() }}」相关内容</p>
            </div>
            <div
              v-else-if="!keyword.trim()"
              class="empty-state"
              style="padding: 24px 16px"
            >
              <p style="color: var(--text-4)">
                检索速达、速记、待办、提示词与倒计时
              </p>
              <div class="shortcut-hints">
                <span><kbd>Ctrl</kbd> + <kbd>K</kbd> 唤起搜索</span>
                <span><kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>Space</kbd> 显示/隐藏窗口</span>
                <span><kbd>Esc</kbd> 关闭</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.search-card {
  width: 560px;
  max-width: calc(100vw - 48px);
  max-height: calc(100vh - 120px);
  display: flex;
  flex-direction: column;
  background: var(--bg-card-solid);
  -webkit-backdrop-filter: blur(18px) saturate(160%);
  backdrop-filter: blur(18px) saturate(160%);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-dock);
  overflow: hidden;
  animation: card-in 0.2s cubic-bezier(0.16, 1, 0.3, 1);
}
@keyframes card-in {
  from {
    opacity: 0;
    transform: translateY(10px) scale(0.97);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

.search-input-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-soft);
}
.search-icon {
  color: var(--text-3);
  flex-shrink: 0;
}
.search-input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  outline: none;
  font-size: 0.9375rem;
  font-family: inherit;
  color: var(--text-1);
}
.search-input::placeholder {
  color: var(--text-4);
}
.esc-hint {
  flex-shrink: 0;
  font-size: 0.625rem;
  color: var(--text-4);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  border-radius: 5px;
  padding: 2px 6px;
  font-family: inherit;
}

.search-results {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 12px 12px 16px;
}
.result-group-title {
  font-size: 0.6875rem;
  font-weight: 600;
  color: var(--text-3);
  padding: 8px 10px 4px;
}
.result-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 10px;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: background 0.12s;
}
.result-item:hover,
.result-item.active {
  background: var(--brand-50);
}
.result-badge {
  flex-shrink: 0;
  width: 34px;
  text-align: center;
  font-size: 0.625rem;
  font-weight: 600;
  border-radius: 6px;
  padding: 3px 0;
}
.result-badge.app {
  background: var(--c-blue-soft);
  color: var(--c-blue-ink);
}
.result-badge.web {
  background: var(--c-green-soft);
  color: var(--c-green-ink);
}
.result-badge.file {
  background: var(--c-purple-soft);
  color: var(--c-purple-ink);
}
.note-badge {
  background: var(--c-yellow-soft);
  color: var(--c-yellow-ink);
}
.todo-badge {
  background: var(--c-gray-soft);
  color: var(--c-gray-ink);
}
.shortcut-hints {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 12px;
  font-size: 0.75rem;
  color: var(--text-3);
}
.shortcut-hints kbd {
  font-family: inherit;
  font-size: 0.6875rem;
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  border-radius: 5px;
  padding: 1px 6px;
  color: var(--text-2);
}
.result-name {
  flex-shrink: 0;
  max-width: 160px;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.result-sub {
  flex: 1;
  min-width: 0;
  font-size: 0.75rem;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mask-enter-active,
.mask-leave-active {
  transition: opacity 0.18s ease-out;
}
.mask-enter-from,
.mask-leave-to {
  opacity: 0;
}
</style>
