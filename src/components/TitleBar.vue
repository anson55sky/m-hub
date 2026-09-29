<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { computed, inject, onBeforeUnmount, onMounted, ref } from 'vue'
import { Copy, Focus, Maximize2, MessageSquare, Minus, Pin, PinOff, Search, Square, X } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'
import {
  isMac,
  prettyShortcut,
  RIGHT_CLICK_TERM,
  TRAY_TERM,
  trayLabel,
} from '../utils/platform'
import { useStore } from '../stores/workbench'

const store = useStore()
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

defineEmits<{
  (e: 'search'): void
  (e: 'chat'): void
  /** 聚焦模式开关（2026-09-29 新增） */
  (e: 'focus'): void
}>()

const alwaysOnTop = computed(() => store.state.config.window.always_on_top)

/**
 * 聚焦模式是否开着（2026-09-29 新增）。按钮的 active 态靠它 ——
 * 界面变成三张大卡之后，用户唯一能判断「我在不在聚焦里」的线索就是这个按钮。
 */
const focusOn = computed(() => store.state.config.focus_enabled === true)

// ---- 平台：macOS 的窗口按钮在**左侧**、是三颗红黄绿圆点，且没有「最大化/还原」这个概念
// （macOS 用「缩放」：拖角 / 双击标题栏 / 绿点，都是在最大化与恢复之间切换，
//  与 Windows 的「最大化 ⇄ 还原」按钮语义不同，硬套会让用户找错按钮）。
// 这里保留 maximize 按钮（绿点）并映射到 toggleMaximize，行为等价。
// 判定与快捷键格式化都在 utils/platform.ts（与设置页共用同一份）。

function toggleAlwaysOnTop() {
  void store.setAlwaysOnTop(!alwaysOnTop.value)
}

const appWindow = isTauri() ? getCurrentWindow() : null

// ---- 窗口拖动：data-tauri-drag-region 只对 mousedown 的精确目标生效，
// 点击标题栏内的子元素（svg/span）时不触发；改用 startDragging 统一处理
//
// ---- 双击最大化：自己判双击，不靠 @dblclick（2026-09-29 修） ----
//
// 原先只有 mousedown → startDragging，没有任何双击处理，所以标题栏双击毫无反应。
// 这里**不用 `@dblclick`**：macOS 的 startDragging 走
// `performWindowDragWithEvent:`，那是一个**原生的模态拖拽会话**，会接管鼠标；
// 双击的第二次按下/抬起是否还能回到 webview 并不保证（实测与 tao 的时序有关）。
// 而 mousedown 是在 startDragging **之前**就收到的，所以从 mousedown 流里
// 自己判双击是确定性的：两次按下间隔够近、位置够近 → 判为双击，
// 此时**不**再调 startDragging（否则第二次按下会先把窗口拖动一下，手感很抖）。
const DOUBLE_CLICK_MS = 400
const DOUBLE_CLICK_SLOP = 5
let lastDownAt = 0
let lastDownX = 0
let lastDownY = 0

function onDragStart(e: MouseEvent) {
  if (!appWindow || e.button !== 0) return
  const target = e.target as HTMLElement
  if (target.closest('button')) return

  const now = performance.now()
  const isDouble =
    now - lastDownAt < DOUBLE_CLICK_MS &&
    Math.abs(e.clientX - lastDownX) <= DOUBLE_CLICK_SLOP &&
    Math.abs(e.clientY - lastDownY) <= DOUBLE_CLICK_SLOP
  lastDownAt = now
  lastDownX = e.clientX
  lastDownY = e.clientY

  if (isDouble) {
    // 清掉时间戳：否则连续三击会被算成「双击 + 双击」而连续切换两次最大化
    lastDownAt = 0
    toggleMaximize()
    return
  }
  appWindow.startDragging()
}

// ---- 最大化状态（切换图标：最大化 ⇄ 还原） ----
const isMaximized = ref(false)
let unlistenResize: (() => void) | null = null

// 最大化时必须把 `data-window-maximized` 写到 <html>：主窗为自绘阴影比可视区
// 最大化时若还留着圆角，屏幕四角会露出四个小小的桌面色缺口，
// style.css 据此把 --window-radius 归零（外扩带与阴影已随外扩带一并移除）。
// **这里是该属性的唯一写方。** Rust 侧刻意不再 eval 一份，两个理由：
//   ① 双写方会出现两边状态不一致的窗口期（一边归零一边没归零 = 缝闪一下）
//   ② 启动期那次 eval 会打在**加载中的空白文档**上 —— 真正页面一换就没了，
//      而「启动即最大化」（restore_window_state 的小屏分支）恰恰是启动期发生的
// 本函数已有的 onMounted + onResized 两条入口恰好覆盖「启动即最大化」与
// 「运行期切换」，不需要新增信号源。
// 幂等：值没变不写（onResized 在拖拽中每秒触发十几次）。
function applyMaximizedClass(maximized: boolean) {
  const el = document.documentElement
  if (maximized) {
    if (el.dataset.windowMaximized !== '1') el.dataset.windowMaximized = '1'
  } else if (el.dataset.windowMaximized !== undefined) {
    delete el.dataset.windowMaximized
  }
}

async function refreshMaximized() {
  if (!appWindow) return
  const next = await appWindow.isMaximized()
  isMaximized.value = next
  applyMaximizedClass(next)
}

onMounted(async () => {
  if (!appWindow) return
  await refreshMaximized()
  unlistenResize = await appWindow.onResized(() => refreshMaximized())
})

onBeforeUnmount(() => {
  unlistenResize?.()
})

function minimize() {
  if (isTauri()) tauriApi.minimizeWindow()
}

function toggleMaximize() {
  if (isTauri()) tauriApi.toggleMaximize()
}

function close() {
  if (isTauri()) {
    tauriApi.hideToTray()
    // 首次关闭时提示已最小化到托盘，避免用户误以为应用退出了
    if (!localStorage.getItem('tray-hint-shown')) {
      localStorage.setItem('tray-hint-shown', '1')
      showToast(`已${trayLabel()}，在${TRAY_TERM}图标上${RIGHT_CLICK_TERM}可退出`)
    }
  }
}
</script>

<template>
  <div class="title-bar" :class="{ 'is-mac': isMac }" @mousedown="onDragStart">
    <!-- macOS：三颗交通灯在左侧。红=关闭 黄=最小化 绿=缩放 -->
    <div v-if="isMac" class="traffic-lights">
      <button class="tl tl-close" :title="`关闭（${trayLabel()}）`" @click="close">
        <X :size="9" :stroke-width="2.4" />
      </button>
      <button class="tl tl-min" title="最小化" @click="minimize">
        <Minus :size="9" :stroke-width="2.4" />
      </button>
      <button class="tl tl-zoom" :title="isMaximized ? '还原' : '缩放'" @click="toggleMaximize">
        <Maximize2 v-if="!isMaximized" :size="8" :stroke-width="2.6" />
        <Copy v-else :size="8" :stroke-width="2.6" />
      </button>
    </div>

    <div class="window-title">
      <span class="title-text">M-Hub</span>
    </div>

    <div class="window-controls">
      <button
        class="tool-btn"
        :title="`全局搜索 (${prettyShortcut(store.state.config.search_shortcut, isMac ? '⌘K' : 'Ctrl+K')}，可在设置中自定义)`"
        @click="$emit('search')"
      >
        <Search :size="15" :stroke-width="1.8" />
      </button>
      <button
        class="tool-btn"
        data-chat-opener
        :title="`AI 对话 (${prettyShortcut(store.state.config.chat_shortcut, isMac ? '⌘⇧K' : 'Ctrl+Shift+K')}，可在设置中自定义)`"
        @click="$emit('chat')"
      >
        <MessageSquare :size="15" :stroke-width="1.8" />
      </button>
      <!--
        聚焦模式开关放在标题栏而不是工作台内部：它切换的是「工作台长什么样」，
        而工作台自身没有常驻表头，塞进去等于每次都要滚回顶部才按得到。
        按钮用 active 态表示当前是否在聚焦里 —— 否则用户按下后、界面变成
        三张大卡之后，反而不知道自己在哪个模式里，也就不知道怎么回去。
      -->
      <button
        class="tool-btn"
        :class="{ active: focusOn }"
        :title="focusOn ? '退出聚焦，回到完整工作台' : '聚焦模式：只留几张卡'"
        @click="$emit('focus')"
      >
        <Focus :size="15" :stroke-width="1.8" />
      </button>
      <div class="tool-divider"></div>
      <button
        class="win-btn top-btn"
        :class="{ active: alwaysOnTop }"
        :title="alwaysOnTop ? '取消窗口置顶' : '窗口置顶'"
        @click="toggleAlwaysOnTop"
      >
        <Pin v-if="!alwaysOnTop" :size="14" :stroke-width="1.8" />
        <PinOff v-else :size="14" :stroke-width="1.8" />
      </button>
      <!-- Windows 风格：窗口按钮在右侧。macOS 走左侧交通灯，这里整组不渲染 -->
      <template v-if="!isMac">
        <button class="win-btn minimize" title="最小化" @click="minimize">
          <Minus :size="15" :stroke-width="1.8" />
        </button>
        <button
          class="win-btn maximize"
          :title="isMaximized ? '还原' : '最大化'"
          @click="toggleMaximize"
        >
          <Square v-if="!isMaximized" :size="14" :stroke-width="1.8" />
          <Copy v-else :size="14" :stroke-width="1.8" />
        </button>
        <button class="win-btn close" :title="`关闭（${trayLabel()}）`" @click="close">
          <X :size="15" :stroke-width="1.8" />
        </button>
      </template>
    </div>
  </div>
</template>

<style scoped>
.title-bar {
  height: 48px;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-left: 18px;
  flex-shrink: 0;
}
/* 铬件（标题栏）始终全透明：与背景（渐变/壁纸）构成同一个连续平面，表面只属于卡片（ADR 0003） */
.window-title {
  display: flex;
  align-items: center;
  gap: 10px;
}
.title-text {
  font-size: 0.9375rem;
  font-weight: 700;
  color: var(--text-1);
  letter-spacing: -0.2px;
}
.window-controls {
  display: flex;
  align-items: center;
  height: 100%;
}
.tool-btn {
  width: 38px;
  height: 100%;
  border: none;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.tool-btn:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
[data-theme="dark"] .tool-btn:hover {
  background: var(--brand-50);
  color: var(--text-1);
}
/* 激活态（聚焦模式）。
   刻意**不**依赖 hover 才看得出：hover 一移开就没了，而「我在不在聚焦里」
   是必须随时可读的状态。用与 `.win-btn.top-btn.active` 同一套口径，
   两处「已开启」的按钮看起来一致 —— 同一语义不该有两种长相。 */
.tool-btn.active {
  background: var(--brand-50);
  color: var(--brand-500);
}
.tool-btn.active:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
[data-theme="dark"] .tool-btn.active,
[data-theme="dark"] .tool-btn.active:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.tool-divider {
  width: 1px;
  height: 18px;
  background: var(--border-soft);
  margin: 0 4px;
}
.win-btn {
  width: 46px;
  height: 100%;
  border: none;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-2);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.win-btn:hover {
  background: var(--bg-card-soft);
}
.win-btn.top-btn.active {
  background: var(--brand-50);
  color: var(--brand-500);
}
.win-btn.top-btn.active:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.win-btn.close:hover {
  background: var(--window-close);
  color: var(--text-on-accent);
}

/* ---- macOS 交通灯 ----
   尺寸/间距/配色对齐 macOS Sonoma 起的默认交通灯（12px 圆、8px 间距、
   悬停时才显示符号）。`padding-left` 从 18px 收到 13px 给三颗灯让位，
   标题的视觉起点与 Windows 版（右对齐窗口按钮那侧）保持同一量级。 */
.title-bar.is-mac {
  padding-left: 13px;
}
.traffic-lights {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-right: 12px;
}
.tl {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  border: none;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: default;
  color: transparent;
  transition: color 0.12s;
}
.tl-close { background: #ff5f57; }
.tl-min { background: #febc2e; }
.tl-zoom { background: #28c840; }
/* macOS 交通灯的原生行为：静止时只有色块，鼠标悬停到**这一组**上才显示符号 */
.traffic-lights:hover .tl {
  color: rgba(0, 0, 0, 0.55);
}
</style>
