<script setup lang="ts">
/**
 * 空内容模块的单行降级条。
 *
 * ## 为什么单独做一个组件，而不是把原卡片压扁
 *
 * 把 5×8 的提示词卡压成 1 行高，它内部那套「空状态占位 + 居中大按钮」的布局
 * 会被挤变形（按钮溢出、留白消失）。而这里要的是**另一件东西**：
 * 一条只说「这里还没有内容，加一条」的单行条。所以是替换整个卡片，
 * 不是缩放它 —— 两者渲染出来的东西根本不同。
 *
 * ## 为什么保留标题
 *
 * 压扁的目的是**把地还回去**，不是把模块藏起来。用户扫一眼仍应知道
 * 「提示词在这里，只是空的」，否则会以为模块被误删了。标题 + 一条
 * 灰色提示 + 右侧一个加号，既省地方又不丢信息。
 */
import { Plus } from 'lucide-vue-next'

defineProps<{
  /** 模块标题（走与常规模块卡片同一套解析，含用户自定义标题） */
  title: string
  /** 空状态提示语 */
  hint: string
  /** 加号的 aria-label（比 hint 更具体，便于读屏用户） */
  actionLabel: string
}>()

const emit = defineEmits<{ (e: 'action'): void }>()
</script>

<template>
  <div class="dash-collapsed">
    <span class="dash-collapsed-title">{{ title }}</span>
    <span class="dash-collapsed-hint">{{ hint }}</span>
    <button
      class="dash-collapsed-add"
      type="button"
      :aria-label="actionLabel"
      :title="actionLabel"
      @click="emit('action')"
    >
      <Plus :size="15" :stroke-width="2" aria-hidden="true" />
    </button>
  </div>
</template>

<style scoped>
/* 单行条。高度不写死：栅格行高由 index.vue 的 gridTemplateRows 统一给，
   这里只保证内容垂直居中、不溢出 —— 压扁后模块只有 1 行高，
   任何 padding/margin 的常量都可能把它顶破。 */
.dash-collapsed {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 100%;
  min-height: 0;
  padding: 0 12px;
  overflow: hidden;
}
.dash-collapsed-title {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  white-space: nowrap;
  flex: 0 0 auto;
}
.dash-collapsed-hint {
  font-size: 0.75rem;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
}
.dash-collapsed-add {
  margin-left: auto;
  flex: 0 0 auto;
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm, 8px);
  background: transparent;
  color: var(--text-2);
  cursor: pointer;
  transition: background 0.14s ease-out, color 0.14s ease-out, transform 0.1s ease-out;
}
.dash-collapsed-add:hover {
  background: var(--bg-card-hover, var(--bg-card-solid));
  color: var(--text-1);
}
/* 按下时的物理回推，给一点触感 */
.dash-collapsed-add:active {
  transform: translateY(1px);
}
.dash-collapsed-add:focus-visible {
  outline: 2px solid var(--brand-500);
  outline-offset: 1px;
}
</style>
