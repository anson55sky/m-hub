<script setup lang="ts">
/**
 * 书签文件夹的**递归**渲染（2026-10-03）。
 *
 * ## 为什么必须递归
 *
 * Chromium 的书签 JSON 是**任意深度**的（Chromium 本身支持多级子文件夹）。
 * 把层级写死成两层会让三层以上的目录**既看不见也选不了** —— 而那恰恰是
 * 用户整理书签的主要方式（「工作 / 前端 / 框架」这种三层很常见）。
 *
 * ## 勾选的语义
 *
 * 文件夹节点的勾选 = 它下面**全部叶子**的合取。复选框有第三态（半选）：
 * 「有勾上但没全勾」时显示方块，这样用户能看出「这个文件夹只选了一部分」。
 */
import { computed } from 'vue'
import { ChevronRight, Folder as FolderIcon, Globe } from 'lucide-vue-next'
import type { BookmarkNode } from '../api/tauri'

const props = defineProps<{
  node: BookmarkNode
  depth: number
  selected: Set<string>
  expanded: Set<string>
}>()

const emit = defineEmits<{
  toggleFolder: [node: BookmarkNode]
  toggleLeaf: [id: string]
  toggleExpand: [id: string]
}>()

/** 该节点下全部叶子（书签条目）的 id —— 文件夹节点递归收集 */
function leafIds(n: BookmarkNode): string[] {
  if (n.url) return [n.id]
  return (n.children ?? []).flatMap(leafIds)
}

const ids = computed(() => leafIds(props.node))
const count = computed(() => ids.value.length)
const allChecked = computed(() => count.value > 0 && ids.value.every((i) => props.selected.has(i)))
const someChecked = computed(
  () => !allChecked.value && ids.value.some((i) => props.selected.has(i)),
)
const isOpen = computed(() => props.expanded.has(props.node.id))
const pad = computed(() => ({ paddingLeft: `${props.depth * 16}px` }))
</script>

<template>
  <!-- 叶子（书签条目） -->
  <div v-if="node.url" class="bm-row" :style="pad">
    <span class="bm-caret" aria-hidden="true" />
    <input type="checkbox" :checked="selected.has(node.id)" @change="emit('toggleLeaf', node.id)" />
    <Globe :size="13" :stroke-width="1.9" class="bm-ic" />
    <span class="bm-name" :title="node.url">{{ node.name }}</span>
  </div>

  <!-- 文件夹：自己一行 + 展开后递归渲染子节点 -->
  <template v-else>
    <div class="bm-row" :style="pad">
      <button class="bm-caret" type="button" aria-label="展开/收起" @click="emit('toggleExpand', node.id)">
        <ChevronRight :size="13" :class="{ open: isOpen }" />
      </button>
      <input
        type="checkbox"
        :checked="allChecked"
        :indeterminate="someChecked"
        @change="emit('toggleFolder', node)"
      />
      <FolderIcon :size="14" :stroke-width="1.9" class="bm-ic" />
      <span class="bm-name" :title="node.category ?? node.name">{{ node.name }}</span>
      <span class="bm-count">{{ count }}</span>
    </div>
    <BmFolderNode
      v-for="c in node.children ?? []"
      v-show="isOpen"
      :key="c.id"
      :node="c"
      :depth="depth + 1"
      :selected="selected"
      :expanded="expanded"
      @toggle-folder="emit('toggleFolder', $event)"
      @toggle-leaf="emit('toggleLeaf', $event)"
      @toggle-expand="emit('toggleExpand', $event)"
    />
  </template>
</template>

<style scoped>
.bm-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 6px;
  border-radius: 6px;
  font-size: 13px;
}
.bm-row:hover {
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
.bm-caret :deep(svg) {
  transition: transform 0.15s;
}
.bm-caret :deep(svg.open) {
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
</style>