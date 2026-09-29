<script setup lang="ts">
/**
 * 窗口 8 向缩放边缘。
 *
 * ## 为什么需要它
 *
 * 主窗为了自绘阴影，窗口比可视区大了一圈 `--window-shadow-margin`（约定 69）；
 * 无边框窗口在 macOS 上又没有系统缩放边。两者叠加的结果是**整窗无处可缩**
 * ——所以这圈外扩带必须同时承担缩放热区的职责，否则它就是一片
 * 属于窗口、能吃掉点击、却什么都不做的死区。
 *
 * 附带解决一个瞄准问题：**看得见的圆角内容边缘并不在窗口边缘**，中间隔着
 * 整整一圈外扩带。若把手柄贴在窗口边上，resize 光标会亮在一圈空白里，
 * 离用户看到的边缘差 32px。所以手柄定位在**内容边**（见 CSS 的
 * `--window-shadow-margin` 偏移），薄薄地压住内容几像素。
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
 * 最大化时外扩带归零（`html[data-window-maximized]`，style.css），
 * 窗口就是屏幕四边，没有「边」可拖，隐藏。
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
 * `--window-shadow-margin` 的偏移把定位基准从「窗口边」挪到「**可见内容边**」：
 * 主窗外侧有 32px 透明外扩带，贴窗口边的话光标会亮在空白里。
 * 无该变量的窗口（浮窗）回落到 0px，行为与改动前一致。 */
.wrz {
  position: fixed;
  z-index: 1;
}
.wrz-north {
  top: var(--window-shadow-margin, 0px);
  left: 0;
  right: 0;
  height: 6px;
  cursor: ns-resize;
}
.wrz-south {
  bottom: var(--window-shadow-margin, 0px);
  left: 0;
  right: 0;
  height: 6px;
  cursor: ns-resize;
}
.wrz-east {
  right: var(--window-shadow-margin, 0px);
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: ew-resize;
}
.wrz-west {
  left: var(--window-shadow-margin, 0px);
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: ew-resize;
}
.wrz-northeast {
  top: var(--window-shadow-margin, 0px);
  right: var(--window-shadow-margin, 0px);
  width: 16px;
  height: 16px;
  cursor: nesw-resize;
}
.wrz-southwest {
  bottom: var(--window-shadow-margin, 0px);
  left: var(--window-shadow-margin, 0px);
  width: 16px;
  height: 16px;
  cursor: nesw-resize;
}
.wrz-northwest {
  top: var(--window-shadow-margin, 0px);
  left: var(--window-shadow-margin, 0px);
  width: 16px;
  height: 16px;
  cursor: nwse-resize;
}
.wrz-southeast {
  bottom: var(--window-shadow-margin, 0px);
  right: var(--window-shadow-margin, 0px);
  width: 16px;
  height: 16px;
  cursor: nwse-resize;
}
</style>
