<script setup lang="ts">
/**
 * 聚焦模式的顶部切换条（2026-09-29 新增）。
 *
 * ## 为什么是一条「切换条」而不是一个下拉
 *
 * 聚焦模式的整个价值是「减少视觉负担」，而下拉框本身就是负担：多一次点击、
 * 一次展开、一次收起。常用操作（换掉当前三张卡里的某一张）如果藏在二级菜单里，
 * 这个功能会变成「想不起来有」。所以直接把候选平铺成一条可点的窄带。
 *
 * ## 为什么选中项排在前面
 *
 * 一排同权重的芯片里，用户要找的总是「我已经选中的那几个」——
 * 用来改状态，而不是用来发现新模块。已选中的排前面之后，
 * 「取消选中」这一步落在拇指/鼠标的同一个位置，不用横向扫过一整排未选中的。
 *
 * 未选中项仍然全部可见（只是排后面）：扩展能往版面上加模块，
 * 而「这里还能加什么」正是聚焦模式里唯一值得展示的额外信息。
 */
import { computed } from 'vue'
import { Check } from 'lucide-vue-next'

const props = defineProps<{
  /** 候选模块：id + 显示名（已选中的由 pinnedIds 判定） */
  options: { id: string; title: string }[]
  pinnedIds: string[]
  /** 关掉聚焦模式（回到完整工作台） */
  closable: boolean
}>()

const emit = defineEmits<{ (e: 'toggle', id: string): void; (e: 'close'): void }>()

/**
 * 已选中的排前面（理由见文件头）。
 *
 * 放 computed 而不是写在模板里：模板里的 `pinnedIds.map(...).filter(Boolean)`
 * 每次渲染都产生**新数组**，v-for 拿到的引用每次都变、整个列表失去复用，
 * 而且 `.filter(Boolean)` 在 TS 里留下 `(T | undefined)[]`，模板里再用就全是
 * 「可能为 undefined」。在 script 里收窄一次，两件事一起解决。
 */
const ordered = computed(() => {
  const byId = new Map(props.options.map((o) => [o.id, o]))
  const picked = props.pinnedIds.map((id) => byId.get(id)).filter((o) => o !== undefined)
  const rest = props.options.filter((o) => !props.pinnedIds.includes(o.id))
  return [...picked, ...rest]
})
</script>

<template>
  <div class="focus-strip" role="toolbar" aria-label="聚焦模式：选择要显示的模块">
    <span class="focus-strip-label">聚焦</span>

    <div class="focus-strip-chips">
      <!--
        已选中在前：见文件头「为什么选中项排在前面」。
        用 computed 排序而不是在模板里 filter+sort，避免每次渲染重建数组
        （模板里的 sort 会因新数组引用而让整个列表失去 v-for 的复用）。
      -->
      <button
        v-for="o in ordered"
        :key="o.id"
        class="focus-chip"
        :class="{ on: pinnedIds.includes(o.id) }"
        type="button"
        :aria-pressed="pinnedIds.includes(o.id)"
        @click="emit('toggle', o.id)"
      >
        <Check v-if="pinnedIds.includes(o.id)" :size="12" :stroke-width="3" aria-hidden="true" />
        {{ o.title }}
      </button>
    </div>

    <button
      v-if="closable"
      class="focus-chip focus-exit"
      type="button"
      title="退出聚焦，回到完整工作台"
      @click="emit('close')"
    >
      退出
    </button>
  </div>
</template>

<style scoped>
.focus-strip {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 0 0 auto;
  padding: 0 2px 10px;
  min-height: 0;
}
.focus-strip-label {
  flex: 0 0 auto;
  font-size: 0.6875rem;
  color: var(--text-4);
  letter-spacing: 0.04em;
}
.focus-strip-chips {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: 1 1 auto;
  min-width: 0;
  /* 候选很多时横向滚动，而不是把整条撑高换行 */
  overflow-x: auto;
  scrollbar-width: none;
}
.focus-strip-chips::-webkit-scrollbar {
  display: none;
}
.focus-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex: 0 0 auto;
  padding: 3px 9px;
  border: 1px solid var(--border-soft);
  border-radius: 999px;
  background: transparent;
  color: var(--text-3);
  font-size: 0.75rem;
  white-space: nowrap;
  cursor: pointer;
  transition: background 0.14s ease-out, color 0.14s ease-out, border-color 0.14s ease-out;
}
.focus-chip:hover {
  background: var(--bg-card-hover, var(--bg-card-solid));
  color: var(--text-1);
}
.focus-chip.on {
  background: color-mix(in srgb, var(--brand-500) 12%, transparent);
  border-color: color-mix(in srgb, var(--brand-500) 40%, transparent);
  color: var(--brand-500);
}
/* 按下时给一点回推，别让「点了没反应」的感觉混进来 */
.focus-chip:active {
  transform: translateY(1px);
}
.focus-chip:focus-visible {
  outline: 2px solid var(--brand-500);
  outline-offset: 1px;
}
.focus-exit {
  flex: 0 0 auto;
}
</style>
