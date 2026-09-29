<script setup lang="ts">
/**
 * 主窗口 8 方向缩放边缘。
 *
 * ## 为什么需要它
 *
 * 主窗为了自绘阴影，窗口比可视区大了一圈（`--window-shadow-margin`，见约定 69）。
 * 这圈外扩带对内容是「死区」：它属于窗口、能吃掉点击，却什么也不渲染。
 * 如果就这么放着，用户在窗口边缘附近按下鼠标会「没反应」——
 * 尤其因为**看得见的圆角内容边缘并不在窗口边缘**，差着整整一圈，
 * 瞄准「窗框」的人会一直落空。
 *
 * 把死区变成缩放手柄，两个问题一起解决：
 * ① 边缘点击有明确去处（系统级 resize，和系统窗口行为一致）
 * ② 手柄自带 resize 光标，鼠标划过去就等于告诉了用户「这儿能拖」
 *
 * 手法沿用 `TodoFloat.vue` 的 8 向隐形边缘（约定 41 的同款 idiom），
 * 不引新依赖。
 *
 * ## 为什么必须在 `.app-shell` 之外

 * `.app-shell` 有 `contain: paint`（圆角裁切所必需，见约定 69），
 * 它会**裁掉所有后代**——放进壳内的手柄会被自己的裁切剪掉。
 * 故本组件由 index.vue 作为**兄弟节点**渲染。
 *
 * ## 最大化态

 * 最大化时外扩带归零（`html[data-window-maximized]`，style.css），
 * 窗口就是屏幕四边，缩放手柄无处安放且会与系统 zoom 行为打架 —— 隐藏。
 * 状态直接读 `<html>` 上那个属性，不另存一份：那个属性由 Rust 的窗口事件写入，
 * 是唯一真相源（前端自己判断会有延迟，缝已经露出来了）。
 */
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauri } from '../api/tauri'

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

const appWindow = isTauri() ? getCurrentWindow() : null

// 最大化态：跟随 <html data-window-maximized>（Rust 侧窗口事件写入）
const maximized = ref(false)
let observer: MutationObserver | null = null

function readMaximized() {
  maximized.value = document.documentElement.dataset.windowMaximized === '1'
}

function onResizeStart(e: MouseEvent, dir: ResizeDirection) {
  if (!appWindow || e.button !== 0) return
  e.preventDefault()
  // 拦掉冒泡：否则 mousedown 会同时命中 titlebar 的拖动逻辑，窗口跟着跑
  e.stopPropagation()
  void appWindow.startResizeDragging(dir)
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
  <template v-if="appWindow && !maximized">
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
/* 缩放边缘区：贴在窗口四边的隐形热区，宽度取 --window-shadow-margin 的
   前半（14px）——再宽就会压到圆角内容边缘，抢走卡片/按钮的点击。
   命中区靠 `cursor` 自解释，故不画任何可见边框。 */
.wrz {
  position: fixed;
  z-index: 1;
}
.wrz-north {
  top: 0;
  left: 0;
  right: 0;
  height: 14px;
  cursor: ns-resize;
}
.wrz-south {
  bottom: 0;
  left: 0;
  right: 0;
  height: 14px;
  cursor: ns-resize;
}
.wrz-east {
  top: 0;
  bottom: 0;
  right: 0;
  width: 14px;
  cursor: ew-resize;
}
.wrz-west {
  top: 0;
  bottom: 0;
  left: 0;
  width: 14px;
  cursor: ew-resize;
}
.wrz-northeast {
  top: 0;
  right: 0;
  width: 22px;
  height: 22px;
  cursor: nesw-resize;
}
.wrz-southwest {
  bottom: 0;
  left: 0;
  width: 22px;
  height: 22px;
  cursor: nesw-resize;
}
.wrz-northwest {
  top: 0;
  left: 0;
  width: 22px;
  height: 22px;
  cursor: nwse-resize;
}
.wrz-southeast {
  bottom: 0;
  right: 0;
  width: 22px;
  height: 22px;
  cursor: nwse-resize;
}
</style>
