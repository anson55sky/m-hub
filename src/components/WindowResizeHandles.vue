<script setup lang="ts">
/**
 * 窗口 8 向缩放边缘。
 *
 * ## 为什么需要它
 *
 * 主窗无边框，而无边框窗口在 macOS 上**没有系统缩放边**，所以整窗无处可缩。
 * ��意早先这里还有第二个理由：主窗当时有一圈 32px 透明外扩带（为了给自绘阴影
 * 留位置），那圈带子必须同时承担缩放热区职责，否则它就是一片属于窗口、
 * 能吃掉点击、却什么都不做的死区。外扩带已于 2026-09-29 移除（见
 * `scripts/check-window-margin.mjs`），本组件的**唯一**理由就是上面那句。
 *
 * 现在手柄直接贴在窗口边缘 —— 这同时修掉了一个瞄准问题：早先手柄要偏移
 * `--window-shadow-margin` 才能落在「看得见的圆角内容边」上，否则 resize 光标
 * 会亮在一圈空白里、离用户看到的边缘差 32px。带子没了，偏移也就不需要了。
 *
 * ## 为什么不用 `startResizeDragging`
 *
 * tao 在 macOS 上对该操作恒返回 `NotSupported`，且无边框窗口没有系统缩放边
 * —— 详见 `src-tauri/src/window_resize.rs` 头注释。故走宿主自实现的
 * `window_resize_begin`（后台线程轮询光标）。
 *
 * ## 为什么必须在 `.app-shell` 之外
 *
 * `.app-shell` 有 `contain: paint`（圆角裁切所必需，见约定 69），
 * 它会**裁掉所有后代**——放进壳内的手柄会被自己的裁切剪掉。
 * 故本组件由 index.vue 作为**兄弟节点**渲染。
 *
 * ## 最大化态
 *
 * 最大化时窗口就是屏幕四边，没有「边」可拖，隐藏。
 * 状态直接读 `<html>` 上那个属性，不另存一份：那个属性由
 * `TitleBar.refreshMaximized()` 写入，是唯一真相源。
 */
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { isTauri, tauriApi } from '../api/tauri'

// 与 @tauri-apps/api window 的 ResizeDirection 同构（该类型未导出，此处本地声明）
type ResizeDirection = 'East' | 'North' | 'NorthEast' | 'NorthWest' | 'South' | 'SouthEast' | 'SouthWest' | 'West'

const RESIZE_DIRECTIONS: ResizeDirection[] = [
  'North',
  'South',
  'East',
  'West',
  'NorthEast',
  'NorthWest',
  'SouthEast',
  'SouthWest',
]

const isTauriApp = isTauri()

// 最大化态：跟随 <html data-window-maximized>
const maximized = ref(false)
let observer: MutationObserver | null = null

function readMaximized() {
  maximized.value = document.documentElement.dataset.windowMaximized === '1'
}

function onResizeStart(e: MouseEvent, dir: ResizeDirection) {
  if (e.button !== 0) return
  e.preventDefault()
  // 拦掉冒泡：否则 mousedown 会同时命中 titlebar 的拖动逻辑，窗口跟着跑
  e.stopPropagation()
  void tauriApi.windowResizeBegin(dir)
}

onMounted(() => {
  readMaximized()
  observer = new MutationObserver(readMaximized)
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-window-maximized'],
  })
})

onBeforeUnmount(() => observer?.disconnect())
</script>

<template>
  <!-- 仅 Tauri 窗口内、且非最大化态渲染 -->
  <template v-if="isTauriApp && !maximized">
    <div
      v-for="dir in RESIZE_DIRECTIONS"
      :key="dir"
      class="wrz"
      :class="'wrz-' + dir.toLowerCase()"
      @mousedown="onResizeStart($event, dir)"
    ></div>
  </template>
</template>

<style scoped>
/* 缩放边缘区：隐形热区，命中范围靠 `cursor` 自解释，故不画任何可见边框。
 *
 * 边 6px / 角 16px：
 *  · 6px 够好点中，又不至于压到内容（标题栏右上角的搜索/对话/置顶按钮
 *    垂直居中在 ~48px 高的条里，6px 不会碰到）
 *  · 角 16px 是为了让斜向拖拽有舒服的起手区
 *
 * 定位基准直接是**窗口边**：外扩带 2026-09-29 移除后，窗口边就是用户看得见的
 * 内容边，不再需要为它加一层偏移。 */
.wrz {
  position: fixed;
  z-index: 1;
}
.wrz-north {
  top: 0px;
  left: 0;
  right: 0;
  height: 6px;
  cursor: ns-resize;
}
.wrz-south {
  bottom: 0px;
  left: 0;
  right: 0;
  height: 6px;
  cursor: ns-resize;
}
.wrz-east {
  right: 0px;
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: ew-resize;
}
.wrz-west {
  left: 0px;
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: ew-resize;
}
.wrz-northeast {
  top: 0px;
  right: 0px;
  width: 16px;
  height: 16px;
  cursor: nesw-resize;
}
.wrz-southwest {
  bottom: 0px;
  left: 0px;
  width: 16px;
  height: 16px;
  cursor: nesw-resize;
}
.wrz-northwest {
  top: 0px;
  left: 0px;
  width: 16px;
  height: 16px;
  cursor: nwse-resize;
}
.wrz-southeast {
  bottom: 0px;
  right: 0px;
  width: 16px;
  height: 16px;
  cursor: nwse-resize;
}
</style>
