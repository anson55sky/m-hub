<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ArrowRight, CalendarDays, ChevronLeft, ChevronRight } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import type { Todo, TodoOccurrence } from '../api/tauri'
import { calendarGrid, dueBadge, isoKey, parseServerDate } from '../utils/todoSchedule'

/**
 * 工作台「日历」模块：月历形态展示待办分布（含周期待办的**虚拟实例**）。
 * 只读呈现——点整卡进待办视图做新建与改期；周期规则的展开结果来自 Rust 侧命令。
 */
const props = defineProps<{ onOpenDetail?: () => void; title?: string; hideTitle?: boolean }>()

const store = useStore()

const cursor = ref(new Date())
const occurrences = ref<TodoOccurrence[]>([])
/** 工作台卡片常驻、不会重新挂载：用分钟 tick 推进「今天」，跨午夜后高亮与逾期判定才跟着走
 *  （与 ClockCard 的 minuteTick 同一口径；只在日期真的变了才赋值，避免无谓重算） */
const today = ref(new Date())
let dayTimer: ReturnType<typeof setInterval> | null = null

const cells = computed(() => calendarGrid(cursor.value, today.value))
const monthLabel = computed(() => `${cursor.value.getFullYear()} 年 ${cursor.value.getMonth() + 1} 月`)

const topTodos = computed(() => store.state.todos.filter((t) => t.parent_id == null))

/** 真实条目落格：未完成按 due_at；**已完成也显示**——有截止落原日期，
 *  没截止则落在完成当天（completed_at，UTC 字符串需补 Z 解析），用删除线淡显。
 *  同一格内未完成排在已完成前面（MAX_CHIPS 截断时不至于把待办挤掉）。 */
const realByDay = computed(() => {
  const map = new Map<string, Todo[]>()
  for (const t of topTodos.value) {
    const at = t.due_at ?? (t.done ? parseServerDate(t.completed_at)?.getTime() ?? null : null)
    if (at == null) continue
    const key = isoKey(new Date(at))
    const list = map.get(key)
    if (list) list.push(t)
    else map.set(key, [t])
  }
  for (const list of map.values()) list.sort((a, b) => Number(a.done) - Number(b.done))
  return map
})

const virtualByDay = computed(() => {
  const map = new Map<string, TodoOccurrence[]>()
  for (const o of occurrences.value) {
    const key = isoKey(new Date(o.at_ms))
    const list = map.get(key)
    if (list) list.push(o)
    else map.set(key, [o])
  }
  return map
})

const titleOf = computed(() => {
  const map = new Map<number, string>()
  for (const t of store.state.todos) map.set(t.id, t.title)
  return map
})

/** 请求序号：快速翻月时先发的响应可能后到，旧月份结果会盖掉新月份 */
let occurrenceSeq = 0

async function load() {
  const first = cells.value[0]
  const from = new Date(`${first.key}T00:00:00`)
  const to = new Date(from)
  to.setDate(to.getDate() + 42)
  const seq = ++occurrenceSeq
  try {
    const list = await store.expandTodoOccurrences(from.getTime(), to.getTime() - 1)
    if (seq !== occurrenceSeq) return
    occurrences.value = list
  } catch {
    // 卡片是概览：失败就当作「没有周期实例」，不弹提示打扰用户（待办视图里有明确提示）
    if (seq === occurrenceSeq) occurrences.value = []
  }
}

/** 周期规则签名：待办视图或扩展桥改了周期规则 / 滚动了一轮之后，
 *  常驻卡片的虚拟实例必须重算，否则日历上留着过期的虚线实例 */
const repeatSignature = computed(() =>
  store.state.todos
    .filter((t) => t.parent_id == null && !t.done && t.repeat_mode !== 'once')
    .map((t) => `${t.id}:${t.due_at}:${t.repeat_mode}:${t.repeat_done_count}`)
    .join('|'),
)

onMounted(() => {
  void load()
  dayTimer = setInterval(() => {
    const d = new Date()
    if (d.toDateString() !== today.value.toDateString()) today.value = d
  }, 60_000)
})
onBeforeUnmount(() => {
  if (dayTimer) clearInterval(dayTimer)
})
watch([cursor, today, repeatSignature], () => void load())

function shift(delta: number, e: MouseEvent) {
  e.stopPropagation()
  const d = new Date(cursor.value)
  d.setMonth(d.getMonth() + delta, 1)
  cursor.value = d
}

/** 每格最多渲染的 chip 数（真实条目优先，剩余额度给虚拟实例） */
const MAX_CHIPS = 2

/** 每格要渲染的 chip 与「还有几条」：真实 + 虚拟合计口径，
 *  余数必须按**实际渲染条数**算——真实 2 条 + 虚拟 2 条时只画 2 条、显示 +2，
 *  不能按「各自截断」算成画 4 条还显示 +2。 */
const chipsByDay = computed(() => {
  const map = new Map<string, { real: Todo[]; virtual: TodoOccurrence[]; more: number }>()
  for (const c of cells.value) {
    const real = realByDay.value.get(c.key) ?? []
    const virtual = virtualByDay.value.get(c.key) ?? []
    const realShown = real.slice(0, MAX_CHIPS)
    const virtualShown = virtual.slice(0, Math.max(0, MAX_CHIPS - realShown.length))
    map.set(c.key, {
      real: realShown,
      virtual: virtualShown,
      more: real.length + virtual.length - realShown.length - virtualShown.length,
    })
  }
  return map
})

/** 某天的**完整**清单（不截断），用于格子的悬停提示。
 *
 * ⚠️ 必须另算一份而不是复用 `chipsByDay` —— 后者按 `MAX_CHIPS` 截断了，
 * 拿它拼提示的话，悬停看到的永远是「前 N 条 +N」，那不是「当天完整清单」，
 * 而用户悬停就是为了看被 +N 折叠掉的那几条。
 *
 * 返回空串而不是 undefined：`:title="undefined"` 在部分平台不渲染 title 属性，
 * 悬停不出提示，而「没提示」与「这天没事项」在界面上长得一模一样。 */
function dayFullList(key: string): string {
  const real = realByDay.value.get(key) ?? []
  const virtual = virtualByDay.value.get(key) ?? []
  if (!real.length && !virtual.length) return ''
  const lines: string[] = []
  // 已完成的排在后面：悬停时先看到还没做的，与日历格子里 chip 的排序口径一致
  const undone = real.filter((t) => !t.done)
  const done = real.filter((t) => t.done)
  for (const t of [...undone, ...done]) {
    lines.push(`${t.done ? '✓ ' : '· '}${t.title}`)
  }
  for (const o of virtual) {
    lines.push(`· ${titleOf.value.get(o.todo_id) ?? '周期待办'}（周期待办）`)
  }
  return lines.join('\n')
}
</script>

<template>
  <section class="card todo-cal" :aria-label="title ?? '日历'" @click="props.onOpenDetail?.()">
    <header class="tc-header" :class="{ 'hd-float': hideTitle }">
      <h3 v-if="!hideTitle" class="tc-title">
        <CalendarDays :size="14" :stroke-width="2" aria-hidden="true" />
        <span>{{ title ?? '日历' }}</span>
      </h3>
      <span class="tc-month">{{ monthLabel }}</span>
      <span class="tc-spacer"></span>
      <button class="tc-nav" type="button" aria-label="上个月" @click="shift(-1, $event)">
        <ChevronLeft :size="13" :stroke-width="2" />
      </button>
      <button class="tc-nav" type="button" aria-label="下个月" @click="shift(1, $event)">
        <ChevronRight :size="13" :stroke-width="2" />
      </button>
      <ArrowRight v-if="!hideTitle" class="tc-more" :size="14" :stroke-width="2" aria-hidden="true" />
    </header>

    <div class="tc-grid">
      <div v-for="d in ['一', '二', '三', '四', '五', '六', '日']" :key="d" class="tc-dow">{{ d }}</div>
      <div
        v-for="c in cells"
        :key="c.key"
        class="tc-cell"
        :class="{ out: c.out, today: c.today }"
        :title="dayFullList(c.key)"
      >
        <!-- 日期与事项**同一行**：格子再小也能同时看到「几号」和「有什么事」。
             此前日期独占一行、事项在下一行，格子一矮日期就把事项挤没了。 -->
        <span class="tc-day">{{ c.day }}</span>
        <div class="tc-chips">
          <!-- ⚠️ chip 上**刻意不给** `:title`：原生 title 取「最深层命中元素」的那个，
               给 chip 加了 title，鼠标悬停在 chip 上就只会看到这一条，
               永远看不到格子的「当天完整清单」—— 而悬停正是为了看被 +N 折叠掉的那几条。
               「已完成」这个信息已由格子级提示里的 `✓` 前缀承担。 -->
          <span
            v-for="t in (chipsByDay.get(c.key)?.real ?? [])"
            :key="'r' + t.id"
            class="tc-chip real"
            :class="{ done: t.done, late: !t.done && dueBadge(t, today)?.kind === 'over' }"
          >{{ t.title }}</span>
          <span
            v-for="o in (chipsByDay.get(c.key)?.virtual ?? [])"
            :key="'v' + o.todo_id + o.at_ms"
            class="tc-chip virtual"
          >{{ titleOf.get(o.todo_id) ?? '周期待办' }}</span>
          <span v-if="(chipsByDay.get(c.key)?.more ?? 0) > 0" class="tc-more-cnt">
            +{{ chipsByDay.get(c.key)?.more }}
          </span>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.todo-cal {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 12px;
  min-height: 0;
  cursor: pointer;
}
.tc-header {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 6px;
}
.tc-title {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
}
.tc-title :deep(svg) {
  color: var(--brand-500);
}
.tc-month {
  font-size: 0.6875rem;
  color: var(--text-3);
}
.tc-spacer {
  flex: 1;
}
.tc-nav {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
}
.tc-nav:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.tc-more {
  color: var(--text-4);
}
.tc-grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  grid-auto-rows: minmax(0, 1fr);
  gap: 2px;
}
.tc-dow {
  font-size: 0.5625rem;
  font-weight: 700;
  color: var(--text-4);
  text-align: center;
}
.tc-cell {
  min-height: 0;
  padding: 2px 3px;
  border: 1px solid var(--border-soft);
  border-radius: 5px;
  background: var(--bg-card-soft);
  overflow: hidden;
  /* 日期与事项同行（发布说明 ⑪）：日期占固定宽度，事项吃掉剩余横向空间。 */
  display: flex;
  align-items: flex-start;
  gap: 2px;
}
.tc-cell.out {
  opacity: 0.4;
}
.tc-cell.today {
  border-color: var(--brand-500);
}
.tc-day {
  font-size: 0.5625rem;
  color: var(--text-4);
  font-variant-numeric: tabular-nums;
  /* 不参与收缩：横向空间被事项挤掉时，保住的是日期而不是标题 ——
     日期没了整格失去意义，标题被 `+N` 折叠掉还能靠悬停看全。 */
  flex: 0 0 auto;
  line-height: 1.5;
}
.tc-chips {
  display: flex;
  flex-direction: column;
  gap: 1px;
  /* ⚠️ `min-width: 0` 不可省：flex 子项默认 `min-width: auto`，
     chip 里的长标题会把这一列撑得比格子宽，把日期挤到格子外面去。
     同行布局下这是最容易被忽略的一条。 */
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
}
.tc-chip {
  display: block;
  padding: 0 3px;
  font-size: 0.5rem;
  line-height: 1.5;
  border-radius: 3px;
  border: 1px solid var(--brand-500);
  background: var(--brand-50);
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tc-chip.virtual {
  border-style: dashed;
  background: transparent;
  color: var(--text-3);
}
.tc-chip.late {
  border-color: var(--c-red-ink);
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.tc-chip.done {
  border-color: var(--border-soft);
  background: transparent;
  color: var(--text-4);
  text-decoration: line-through;
}
.tc-more-cnt {
  font-size: 0.5rem;
  color: var(--text-4);
}
</style>
