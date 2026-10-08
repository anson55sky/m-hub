<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, onUnmounted, provide, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { open, save } from '@tauri-apps/plugin-dialog'
import TitleBar from '../components/TitleBar.vue'
import SudaWebPanel from '../components/SudaWebPanel.vue'
import TodoCard from '../components/TodoCard.vue'
import TodoCalendarCard from '../components/TodoCalendarCard.vue'
import Suda from '../components/Suda.vue'
import NoteList from '../components/NoteList.vue'
import NoteFolderTree from '../components/NoteFolderTree.vue'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import { parseTimestamp } from '../utils/time'
import { filterNotesByFolder } from '../utils/noteFolderFilter'
import type { ToastOptions } from '../utils/toast'
import NotesOverviewCard from '../components/NotesOverviewCard.vue'
import TodoOverviewCard from '../components/TodoOverviewCard.vue'
import ResourcesOverviewCard from '../components/ResourcesOverviewCard.vue'
import SudaCustomCard from '../components/SudaCustomCard.vue'
import SysMonitorCard from '../components/SysMonitorCard.vue'
import PromptBoxCard from '../components/PromptBoxCard.vue'
import RecentBar from '../components/RecentBar.vue'
import ClockCard from '../components/ClockCard.vue'
import WeatherCard from '../components/WeatherCard.vue'
import StickyCard from '../components/StickyCard.vue'
import DashCollapsedBar from '../components/DashCollapsedBar.vue'
const FocusStrip = defineAsyncComponent(() => import('../components/FocusStrip.vue'))
import CountdownCard from '../components/CountdownCard.vue'
import WindowResizeHandles from '../components/WindowResizeHandles.vue'
import { useStore } from '../stores/workbench'
import { isTauri, tauriApi } from '../api/tauri'
import { convertFileSrc } from '@tauri-apps/api/core'
import type { Countdown, ExtensionEntry, Note, NoteFolderTarget, Resource, Snippet, Todo } from '../api/tauri'
import { playChime } from '../utils/chime'
import { shortcutLabel } from '../utils/platform'
import { FileText, FolderOpen, LayoutDashboard, ListTodo, MessageSquare, Puzzle, Settings, ChevronLeft, ChevronRight, AppWindow, PanelRight } from 'lucide-vue-next'
import type { Component } from 'vue'
import { useTheme } from '../composables/useTheme'
import { broadcastThemeToFrames } from '../composables/themeTokens'
import { iconSrc } from '../composables/useResourceIcon'
import {
  dashPlacementHideTitle,
  dashPlacementTitle,
  dashVariantDef,
  useDashboardLayout,
  type DashPlacement,
} from '../composables/useDashboardLayout'
import SettingsSkeleton from '../components/SettingsSkeleton.vue'

// 大体量/低频视图异步分包按需加载，缩小首屏主 chunk
const NoteEditor = defineAsyncComponent(() => import('../components/NoteEditor.vue'))
const GlobalSearch = defineAsyncComponent(() => import('../components/GlobalSearch.vue'))
const QuickCapture = defineAsyncComponent(() => import('../components/QuickCapture.vue'))
// 待办视图：自带编辑弹层 / 确认弹窗 / 日期时间字段，体量大且非首屏，同样按需分包
const TodoView = defineAsyncComponent(() => import('../components/TodoView.vue'))
// 设置页：加载期间用同骨架占位（**delay 0**：以前设 80ms 是为了避免骨架一闪而过，
// 结果那 80ms 是纯空白 —— 用户感知到的「点设置先空白」有一半来自这里）
// 另外启动后空闲时预热这个 chunk，点「设置」时直接命中模块缓存，几乎零等待。
const SettingsView = defineAsyncComponent({
  loader: () => import('../components/SettingsView.vue'),
  loadingComponent: SettingsSkeleton,
  delay: 0,
})
const PromptManageDialog = defineAsyncComponent(() => import('../components/PromptManageDialog.vue'))
const SudaCustomEditDialog = defineAsyncComponent(() => import('../components/SudaCustomEditDialog.vue'))
const ChatPanel = defineAsyncComponent(() => import('../components/ChatPanel.vue'))
const ExtensionCenter = defineAsyncComponent(() => import('../components/ExtensionCenter.vue'))
const ExtensionView = defineAsyncComponent(() => import('../components/ExtensionView.vue'))
const DashboardLayoutEditor = defineAsyncComponent(() => import('../components/DashboardLayoutEditor.vue'))

const store = useStore()
/**
 * AI 对话快捷键的**显示**文本。
 *
 * ⚠️ 此前这里硬编码 `Ctrl+Shift+K`：macOS 上真实键是 ⌘⇧K，同一功能的标题栏
 * tooltip 与设置页都写着 ⌘⇧K，三处不一致 —— 用户会去按 ⌃⇧K 发现没反应，
 * 回来当成 bug。而且用户改了自定义快捷键后这行也不更新。
 */
const chatShortcutLabel = computed(() => shortcutLabel('chat', store.state.config.chat_shortcut))

// 初始化三轴主题系统（应用 data-theme/data-preset/inline --accent，监听系统变化）
useTheme()

// ---- 应用壁纸：主窗口所有视图共用，浮窗不跟随；文件失效时静默回退渐变背景 ----
// 壁纸单一套，所有主题模式共用同一张
const wallpaperFailed = ref(false)
watch(
  () => store.state.config.wallpaper_path,
  () => {
    wallpaperFailed.value = false
  },
)
const wallpaperSrc = computed(() => {
  const p = store.state.config.wallpaper_path
  if (!p || wallpaperFailed.value || !isTauri()) return ''
  return convertFileSrc(p)
})

// 蒙版不透明度钳制 0–0.85：85% 封顶保证壁纸仍可辨识
const wallpaperVeil = computed(() =>
  Math.min(0.85, Math.max(0, store.state.config.wallpaper_veil)),
)

// 壁纸在场标记（html data-wallpaper）：壁纸态可读性增强的作用域开关。
// data-wallpaper-clear：卡片真实透底（低玻璃透明度或沉浸模式）时才为真，控制卡片文字光晕的启停
watch(
  [
    wallpaperSrc,
    () => store.state.config.glass_opacity,
    () => store.state.config.wallpaper_immersive,
  ] as const,
  ([src, glass, immersive]) => {
    const el = document.documentElement
    if (src) {
      el.dataset.wallpaper = '1'
    } else {
      delete el.dataset.wallpaper
    }
    if (src && (immersive || glass < 0.9)) {
      el.dataset.wallpaperClear = '1'
    } else {
      delete el.dataset.wallpaperClear
    }
    // 壁纸态切换会翻转扩展令牌（压墨/白墨），重新广播给扩展 iframe
    requestAnimationFrame(() => broadcastThemeToFrames())
  },
  { immediate: true },
)

function onWallpaperError() {
  wallpaperFailed.value = true
}

// ---- 视图切换（统一导航范式：每个侧栏项 = 一个独立视图） ----
const navigation = [
  { id: 'dashboard', label: '工作台', icon: LayoutDashboard },
  { id: 'todos', label: '待办', icon: ListTodo },
  { id: 'notes', label: '速记', icon: FileText },
  { id: 'suda', label: '速达', icon: FolderOpen },
  { id: 'chat', label: '对话', icon: MessageSquare },
] as const

// 对话入口暂时隐藏（后续恢复），功能仍可通过标题栏按钮 / Ctrl+Shift+K 唤起
const visibleNavigation = navigation.filter((item) => item.id !== 'chat')

// 设置不在顶部导航列表，作为独立入口固定在侧栏左下角，但同样是视图切换逻辑
// 自定义布局编辑器也是独立视图（从设置进入，完成后回主页面）
type ViewId =
  | (typeof navigation)[number]['id']
  | 'settings'
  | 'layout-editor'
  | 'extensions'
  | 'extension'
  | 'suda-web'
const activeView = ref<ViewId>('dashboard')

// 对话入口：点击侧栏「对话」即唤起右侧面板（面板是主形态，视图仅占位说明）
function onNavClick(id: ViewId) {
  activeView.value = id
  if (id === 'chat' && !chatOpen.value) {
    toggleChat()
  }
}

// ---- 扩展打开：扩展中心点开某个扩展 → 主区渲染扩展入口（view 形态） ----
const openedExtension = ref<{ id: string; surface: string | null; name: string } | null>(null)
const drawerExtension = ref<{ id: string; surface: string | null; name: string } | null>(null)
// 强制重载计数：WebView2 在窗口后台久置后可能丢弃扩展 iframe 内容（恢复前台后空白），
// 而点击「同一个」已打开的扩展时 extId/surface 均不变、useExtensionFrame 的 watch 不触发，
// 表现为点了没反应——每次打开都递增，强制 iframe 重新导航
const extensionReloadTick = ref(0)
const installedExtensions = ref<ExtensionEntry[]>([])

function onOpenExtension(ext: ExtensionEntry) {
  // 扩展中心点开扩展 = 按该扩展的「打开方式」打开（设置弹窗里配置，默认 view=软件内）。
  // view 保持旧的 surface:null（入口按 manifest 默认形态解析），window/drawer 走分流
  const mode = store.state.config.extension_open_modes?.[ext.id] ?? 'view'
  if (mode === 'view') {
    extensionReloadTick.value++
    openedExtension.value = { id: ext.id, surface: null, name: ext.name }
    activeView.value = 'extension'
    return
  }
  openExtensionSurface(ext.id, mode)
}

// ---- 速达「应用内打开网页」：主窗内嵌面板（ADR 0011） ----
// store.launchResource 在网页条目 + panel 模式时派发 CustomEvent（store 不持有视图状态），
// 这里切到 suda-web 视图渲染 SudaWebPanel（挂载时按内容区矩形调 suda_panel_show）
const sudaWebPanel = ref<{ id: number; url: string; name: string } | null>(null)
let viewBeforeSudaWeb: ViewId = 'suda'

function openSudaWebPanel(detail: { id: number; url: string; name: string }) {
  if (activeView.value !== 'suda-web') viewBeforeSudaWeb = activeView.value
  sudaWebPanel.value = detail
  activeView.value = 'suda-web'
}

function onSudaWebPanelEvent(e: Event) {
  const detail = (e as CustomEvent<{ id: number; url: string; name: string }>).detail
  if (detail && typeof detail.id === 'number') openSudaWebPanel(detail)
}

function closeSudaWebPanel() {
  void tauriApi.sudaPanelHide().catch(() => {})
  sudaWebPanel.value = null
  activeView.value = viewBeforeSudaWeb
}

// 经侧栏等途径离开面板视图时也要收起子 webview（面板没有侧栏入口，回不去即应隐藏）
watch(activeView, (now, prev) => {
  if (prev === 'suda-web' && now !== 'suda-web') {
    void tauriApi.sudaPanelHide().catch(() => {})
    sudaWebPanel.value = null
  }
})

async function refreshInstalledExtensions() {
  if (!isTauri()) return
  try {
    installedExtensions.value = await tauriApi.listExtensions()
    pruneStalePinnedExtensions()
  } catch {
    // 忽略：扩展列表加载失败时侧栏仅保留已缓存的条目
  }
}

// 卸载后同步清理配置里残留的固定 id（否则下次重启侧栏会出现失效条目，点击报“不存在”）
function pruneStalePinnedExtensions() {
  const pinned = store.state.config.sidebar_extensions ?? []
  const validIds = new Set(installedExtensions.value.map((e) => e.id))
  const stale = pinned.filter((id) => !validIds.has(id))
  if (stale.length > 0) {
    store.setSidebarExtensionBulk(pinned.filter((id) => validIds.has(id)))
  }
}

function onExtensionsChanged() {
  void refreshInstalledExtensions()
}

// 固定到左侧栏的扩展：点击侧栏菜单即在主区打开（view 形态）
const pinnedExtensionIds = computed(() => store.state.config.sidebar_extensions ?? [])

const sidebarExtensions = computed(() =>
  installedExtensions.value.filter(
    (e) =>
      pinnedExtensionIds.value.includes(e.id) &&
      !e.invalid &&
      !e.disabled &&
      (e.missing_dependencies ?? []).length === 0,
  ),
)

function openSidebarExtension(ext: ExtensionEntry) {
  // 按扩展设置的「默认打开方式」打开：view（主区）/ window（独立窗口）/ drawer（抽屉）
  const mode = store.state.config.extension_open_modes?.[ext.id] ?? 'view'
  openExtensionSurface(ext.id, mode)
}

// 进出扩展中心时刷新侧栏扩展列表（新装/卸载/固定后即时反映）
watch(activeView, (v) => {
  if (v === 'extensions') void refreshInstalledExtensions()
})

// 扩展 module 内通过 runtime.open(surface) 请求打开自身某个形态（通用能力）
const VALID_EXT_SURFACES = new Set(['view', 'window', 'drawer'])

function openExtensionSurface(extId: string, surface: string) {
  const s = VALID_EXT_SURFACES.has(surface) ? surface : 'view'
  const ext = installedExtensions.value.find((e) => e.id === extId)
  if (s === 'window') {
    tauriApi.openExtensionWindow(extId).catch((e) => {
      showToast(`打开窗口失败：${String(e)}`)
    })
    return
  }
  if (s === 'drawer') {
    extensionReloadTick.value++
    drawerExtension.value = { id: extId, surface: 'drawer', name: ext?.name ?? extId }
    return
  }
  extensionReloadTick.value++
  openedExtension.value = { id: extId, surface: s, name: ext?.name ?? extId }
  activeView.value = 'extension'
}

function closeExtension() {
  openedExtension.value = null
  activeView.value = 'extensions'
}

function openExtensionWindow() {
  if (!openedExtension.value) return
  tauriApi.openExtensionWindow(openedExtension.value.id).catch((e) => {
    showToast(`打开窗口失败：${String(e)}`)
  })
}

function openExtensionDrawer() {
  if (!openedExtension.value) return
  extensionReloadTick.value++
  drawerExtension.value = { ...openedExtension.value }
}

function closeExtensionDrawer() {
  drawerExtension.value = null
}

// ---- 侧边栏收起（展开功能默认关闭，侧栏默认收起；开启后显示展开/收起按钮） ----
const sidebarCollapsed = ref(true)

function toggleSidebar() {
  sidebarCollapsed.value = !sidebarCollapsed.value
}

function openPromptManage() {
  promptManageVisible.value = true
}

function openNotes() {
  activeView.value = 'notes'
}
function openSuda() {
  activeView.value = 'suda'
}
// 待办概览卡 / 待办卡标题栏的「打开待办视图」：进独立的待办视图（标签筛选、月/周日历、周期待办）
function openTodo() {
  activeView.value = 'todos'
}

// ---- 工作台自定义布局（12 列单元格网格，模块库两栏编辑器） ----
const layout = useDashboardLayout()

// 模块 id → 组件 + props 映射（含原「中上可切换」的 5 个独立模块）
const dashCardComponents: Record<string, Component> = {
  clock: ClockCard,
  weather: WeatherCard,
  sysmon: SysMonitorCard,
  sticky1: StickyCard,
  sticky2: StickyCard,
  notes: NotesOverviewCard,
  todo_overview: TodoOverviewCard,
  resources: ResourcesOverviewCard,
  suda1: SudaCustomCard,
  suda2: SudaCustomCard,
  suda3: SudaCustomCard,
  suda4: SudaCustomCard,
  countdown: CountdownCard,
  prompts: PromptBoxCard,
  todo: TodoCard,
  calendar: TodoCalendarCard,
  recent: RecentBar,
}

function dashCardComponent(id: string): Component {
  // 扩展 module：id 形如 ext:<扩展id>，用 iframe 渲染其 module 入口（复用桥 API）
  if (id.startsWith('ext:')) return ExtensionView
  return dashCardComponents[id] ?? ClockCard
}

function dashCardProps(p: DashPlacement): Record<string, unknown> {
  const id = p.id
  if (id.startsWith('ext:')) {
    const extId = id.slice('ext:'.length)
    return {
      extId,
      surface: 'module',
      variant: p.variant ?? dashVariantDef(id)?.id ?? null,
      onOpenSurface: (surface: string) => openExtensionSurface(extId, surface),
      // 扩展 module 卡片也有宿主表头（标题 = 自定义标题 ?? manifest.name），与内置模块一致
      ...titleProps(p),
      // 扩展卡没有内置文案兜底，标题给宿主侧解析后的值
      title: dashPlacementTitle(p),
    }
  }
  switch (id) {
    case 'clock':
      return { variant: p.variant }
    case 'weather':
      return { variant: p.variant }
    case 'sticky1':
      return { slot: 1, ...titleProps(p) }
    case 'sticky2':
      return { slot: 2, ...titleProps(p) }
    case 'notes':
      return { onOpenDetail: openNotes, ...titleProps(p) }
    case 'todo_overview':
      return { onOpenDetail: openTodo, ...titleProps(p) }
    case 'resources':
      return { onOpenDetail: openSuda, ...titleProps(p) }
    case 'suda1':
    case 'suda2':
    case 'suda3':
    case 'suda4':
      return {
        slotId: id,
        ...titleProps(p),
        onConfigure: () => {
          sudaCustomEditId.value = id
        },
      }
    case 'prompts':
      return { onOpenManage: openPromptManage, ...titleProps(p) }
    case 'todo':
      return { highlightId: highlightTodoId.value, onOpenDetail: openTodo, ...titleProps(p) }
    case 'calendar':
      return { onOpenDetail: openTodo, ...titleProps(p) }
    case 'countdown':
      return { sizeW: p.w, sizeH: p.h, ...titleProps(p) }
    case 'sysmon':
    case 'recent':
      return { ...titleProps(p) }
    default:
      return {}
  }
}

/** 卡片标题相关 props：title = 自定义标题（缺省由卡片回退内置文案），hideTitle = 关闭标题行 */
function titleProps(p: DashPlacement): { title?: string; hideTitle: boolean } {
  return { title: p.title, hideTitle: dashPlacementHideTitle(p) }
}

// 主界面行高按「总行数均分可用高度」自适应（1fr），窗口缩放/分辨率变化只改每格像素值、
// 卡片占格比例不变 → 不滚动、不留白、不因缩放而错位
const dashGridRows = computed(() => {
  let m = 1
  // 用有效版面而非保存布局：否则整段折叠了、栅格高度却没跟着缩，
  // 底部会留下一截没有任何模块的空行（看起来像布局出错）。
  for (const p of layout.effectivePlacements.value) {
    m = Math.max(m, p.y + p.h)
  }
  return m
})

/* ───────────────── 聚焦模式（2026-09-29 新增）─────────────────
 *
 * 只显示几个模块、竖排铺满整宽。为什么不用原来的 12 列栅格：
 * 聚焦的价值是「每张卡拿到全部宽度」，而栅格的宽是固定的 1/12、2/4、1/3……
 * 挑三张出来还占原来的窄格，等于没聚焦。
 *
 * 高度用 flex 比例而不是固定行高：比例取自各 variant 自己声明的 idealH，
 * 于是「待办多高、便签多高」是模块自己定的，不是这里拍脑袋。
 *
 * 空内容模块在聚焦模式里**逐个**折叠（不是整段）—— 这在栅格里不行（会留洞），
 * 但这里是 flex 竖排：压扁一个不会在它下面留洞，所以逐个折叠是安全的。
 * 同一个「折叠」概念在两种布局下规则不同，原因是布局不同，不是规则自相矛盾。
 */
const focusOn = computed(() => store.state.config.focus_enabled === true)

/** 磁盘上 focus_pins 为空（老配置 / 用户全取消）时的兜底。与 config.rs 的
 *  `default_focus_pins` 同值 —— 两边各写一份是「第二份拷贝」，但真源在 Rust 的
 *  serde default 上（磁盘已有该字段时以磁盘为准），这里只是防止读到空数组。 */
const DEFAULT_FOCUS_PINS = ['todo', 'sticky1', 'countdown']
/** 选中的模块 id（顺序即显示顺序）。缺失的补上默认项，避免升级后一屏空白 */
const focusPins = computed(() => {
  const raw = store.state.config.focus_pins ?? []
  const known = new Set(allModuleOptions.value.map((o) => o.id))
  const valid = raw.filter((id) => known.has(id))
  return valid.length > 0 ? valid : DEFAULT_FOCUS_PINS
})

/** 候选模块：当前版面上的全部模块（+ 未加入版面的），供切换条使用 */
const allModuleOptions = computed(() => {
  const out: { id: string; title: string }[] = []
  const seen = new Set<string>()
  for (const p of layout.placements.value) {
    if (seen.has(p.id)) continue
    seen.add(p.id)
    out.push({ id: p.id, title: dashPlacementTitle(p) })
  }
  // 版面上还没有的模块也列出来：聚焦模式下「还能加什么」是唯一值得展示的额外信息
  for (const m of layout.available.value) {
    if (seen.has(m.id)) continue
    seen.add(m.id)
    out.push({ id: m.id, title: m.title })
  }
  return out
})

async function persistFocus(enabled: boolean, pins: string[]) {
  await store.setFocusMode(enabled, pins)
}

/** 从标题栏切进来：开启时把视图也切到工作台，否则「聚焦」在一个看不见的视图里生效 */
function toggleFocusMode() {
  const next = !focusOn.value
  if (next && activeView.value !== 'dashboard') activeView.value = 'dashboard'
  void persistFocus(next, focusPins.value)
}

/** 切换条上点某个模块：在/不在聚焦列表里 */
function toggleFocusPin(id: string) {
  const cur = focusPins.value
  const next = cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]
  // 一个都不选会让工作台变成空屏 —— 那不是「更专注」，是坏了。
  // 所以最后一次取消被忽略，并在下面明确告诉用户。
  if (next.length === 0) {
    showToast('至少要保留一个模块')
    return
  }
  void persistFocus(true, next)
}

/** 聚焦模式里每个模块占的高度比例（取自 variant 的 idealH） */
function focusWeight(p: DashPlacement): number {
  if (isFocusCollapsed(p.id)) return 0
  // `dashVariantDef` 直接返回**该模块当前 variant** 的定义（含 idealH），
  // 不是「模块 + variants 数组」—— 之前按后者写会报「variants 不存在」。
  const def = dashVariantDef(p.id, p.variant)
  // 没有 variant 定义（扩展 module 少见）时给 1：宁可不精确也不要 0（0 会让卡片消失）
  return Math.max(1, Math.min(def?.idealH ?? 3, 8))
}

/** 聚焦模式里的空内容判定：与栅格版同一套「是否降级」名单 */
function isFocusCollapsed(id: string): boolean {
  const state = store.state
  switch (id) {
    case 'prompts':
      return state.snippets.length === 0
    case 'todo':
    case 'todo_overview':
      return state.todos.every((t) => t.done)
    case 'countdown':
      return state.countdowns.length === 0
    case 'notes':
      return state.notes.length === 0
    case 'recent':
    case 'resources':
      return state.resources.length === 0
    default:
      return false
  }
}

/**
 * 聚焦模式里模块的呈现：沿用保存的 variant / 标题，只是丢掉坐标。
 *
 * 版面上没有该模块时给一个占位 placement —— 用户可以从切换条里挑一个**还没**
 * 放上版面的模块来看效果，不该因为「版面上没有」就渲染不出来。
 * 但 `ext:` 前缀的扩展不行：它的 manifest 决定卡片长什么样，
 * 凭一个空壳 placement 渲染只会得到一个错的框（见 index.vue 里 ext: 的分支）。
 */
function focusPlacement(id: string): DashPlacement | undefined {
  const saved = layout.placements.value.find((p) => p.id === id)
  if (saved) return { ...saved, x: 0, y: 0, w: 12, h: 1 }
  if (id.startsWith('ext:')) return undefined
  // 显式标注类型：就地写对象字面量会被推断成「少了 x/y/w/h 的窄类型」，
  // 与 DashPlacement 不兼容（TS2322）。
  // `variant` 留空而不是写 null —— 接口声明的是 `variant?: string`，
  // 而 `null` 在这里不合法（读侧 `p.variant ?? defaultVariant` 用的是 ??，
  // 空值与 null 行为一致，声明却只允许 undefined）。
  const fallback: DashPlacement = { id, x: 0, y: 0, w: 12, h: 1 }
  return fallback
}

/** 各模块压扁后单行条上的提示语 */
const COLLAPSED_HINT: Record<string, string> = {
  prompts: '还没有提示词片段',
  todo: '今天没有待办',
  todo_overview: '没有待办',
  countdown: '还没有倒计时',
  notes: '还没有速记',
  recent: '还没有最近使用',
  resources: '速达是空的',
}

/** 单行条上的加号：复用各模块**已有的**打开入口，不新造跳转路径 */
function onCollapsedAction(id: string) {
  switch (id) {
    case 'prompts':
      openPromptManage()
      break
    case 'todo':
    case 'todo_overview':
    case 'calendar':
      openTodo()
      break
    case 'notes':
      openNotes()
      break
    case 'recent':
    case 'resources':
    case 'suda1':
    case 'suda2':
    case 'suda3':
    case 'suda4':
      openSuda()
      break
    default:
      // countdown 的内容只能在它自己的卡片里加（没有独立的列表视图），
      // 而卡片此刻正被压成一条 —— 所以这里退回工作台并把提示交给 toast，
      // 而不是静默无反应。
      showToast('倒计时要在卡片上点「+」添加')
  }
}

function dashCellStyle(p: DashPlacement) {
  return {
    gridColumn: `${p.x + 1} / span ${p.w}`,
    gridRow: `${p.y + 1} / span ${p.h}`,
  }
}

function openLayoutEditor() {
  activeView.value = 'layout-editor'
}

// ---- 启动加载：数据就绪后隐藏欢迎页（窗口已改为启动即显示） ----
// 欢迎页至少展示 2.2s：本地数据库很小，数据可能几十毫秒就加载完，
// 若不保底，淡出会发生在首帧绘制之前，用户根本看不到欢迎页
const bootStartAt = performance.now()
const BOOT_MIN_MS = 2200
const BOOT_MAX_MS = 4000

onMounted(async () => {
  // 设置页与待办视图 chunk 空闲预热：点进去时直接命中模块缓存，不再出现加载空白。
  // （设置页内部也按大类分包 + 占位 + 悬停预取，见 SettingsView.vue 头部说明）
  const warmSettings = () => {
    void import('../components/SettingsView.vue')
    void import('../components/TodoView.vue')
  }
  const idle = (
    window as unknown as {
      requestIdleCallback?: (cb: () => void, opts?: { timeout: number }) => void
    }
  ).requestIdleCallback
  if (idle) idle(warmSettings, { timeout: 3000 })
  else window.setTimeout(warmSettings, 1500)

  store.loadInitialData().finally(() => {
    store.startOnlineMonitor()
    const wait = Math.max(0, BOOT_MIN_MS - (performance.now() - bootStartAt))
    setTimeout(hideBootSplash, wait)
  })
  // 兜底：无论数据是否加载成功，最多 4s 后隐藏欢迎页，避免一直遮挡
  setTimeout(hideBootSplash, BOOT_MAX_MS)
  // 浮窗便签还原/删除后，主窗口实时同步便签与脱离状态
  if (isTauri()) {
    // 速达网页内嵌面板打开请求（store.launchResource 派发，DOM 事件免视图状态跨层传递）
    window.addEventListener('suda-open-web-panel', onSudaWebPanelEvent)
    /** 单条监听注册失败（桥未就绪等）不能中断启动流程：
     *  裸 await 的任一 reject 会让其后所有监听与抽屉还原都不再执行。 */
    const on = async <T,>(ev: string, cb: (e: { payload: T }) => void) => {
      try {
        return await listen<T>(ev, cb)
      } catch {
        return null
      }
    }
    void refreshInstalledExtensions()
    unlistenStickies = await on('stickies-changed', () => {
      store.refreshStickies()
    })
    // 倒计时到点：toast 提示 + 刷新列表（浮窗水罐同步）
    unlistenCountdownFired = await on<Countdown>('countdown-fired', (e) => {
      const name = e.payload?.name ?? ''
      showToast(name ? `「${name}」时间到` : '倒计时时间到')
      if (store.state.config.countdown_sound) {
        playChime()
      }
      void store.refreshCountdowns()
    })
    // ticker 顺延 / 创建更新后同步
    unlistenCountdownsChanged = await on('countdowns-changed', () => {
      void store.refreshCountdowns()
    })
    // 剪贴板浮层「存为速记 / 加入提示词库」后，主窗口实时刷新列表
    unlistenNotesChanged = await on('notes-changed', () => {
      void store.refreshNotes()
    })
    unlistenSnippetsChanged = await on('snippets-changed', () => {
      void store.loadSnippets()
    })
    unlistenTodosChanged = await on('todos-changed', () => {
      void store.refreshTodos()
    })
    // 待办标签定义/关联被外部（扩展桥）改动后刷新
    unlistenTodoTagsChanged = await on('todo-tags-changed', () => {
      void store.refreshTodoTags()
    })
    // 待办提醒到点：toast 提示（系统通知由后端 todo_reminder 直接发）
    unlistenTodoRemind = await on<Todo>('todo-remind', (e) => {
      const title = e.payload?.title ?? ''
      showToast(title ? `待办提醒：「${title}」` : '待办提醒时间到')
    })
    // 桌面悬浮球动作（ADR 0004）：切视图 / 全局搜索 / 新建速记
    // （Rust trigger 已先显示主窗口；剪贴板 act:clipboard 在 Rust 直呼浮层，不经这里）
    unlistenBallAction = await on<string>('floating-ball-action', (e) => {
      const id = e.payload ?? ''
      if (id.startsWith('view:')) {
        const v = id.slice(5)
        if (v === 'chat') {
          activeView.value = 'chat'
          if (!chatOpen.value) toggleChat()
        } else {
          activeView.value = v as ViewId
        }
      } else if (id === 'act:search') {
        searchVisible.value = true
      } else if (id === 'act:note') {
        void onCreateNote()
      }
    })
    // 独立对话窗「模型设置」入口：唤出主窗并定位到设置 → AI 助手
    unlistenOpenChatSettings = await on('open-chat-settings', () => {
      onOpenChatSettings()
    })
    // 搜索快捷键（全局注册，Rust 分发）：主窗前端负责打开/关闭搜索弹窗
    unlistenSearchShortcut = await on('search-shortcut', () => {
      searchVisible.value = !searchVisible.value
    })
    // AI 对话快捷键（全局注册，Rust 分发）：与抽屉/独立窗口形态分流共用 toggleChat
    unlistenChatShortcut = await on('chat-shortcut', () => {
      toggleChat()
    })
    // 统一捕获快捷键（全局注册，Rust 分发，2026-09-29 新增）
    unlistenCaptureShortcut = await on('capture-shortcut', () => {
      captureVisible.value = !captureVisible.value
    })
    // 速记快捷键（全局注册，Rust 分发，2026-10-08 新增，v0.8.0 发布说明 ⑩）：
    // 切到速记并新建一条空白笔记。onCreateNote 自己会把 activeView 切成 'notes'，
    // 所以这里不需要再赋值一次（两处都写 = 下次改一处就漂）。
    unlistenNotesShortcut = await on('notes-shortcut', () => {
      void onCreateNote()
    })
    // 扩展页「去授权」跳转（桥 API mhub.openPermissions）：切到扩展中心并打开该扩展的
    // 设置弹窗（权限管理所在处）。payload = 扩展 id
    unlistenOpenExtSettings = await on<string>('open-extension-settings', (e) => {
      if (e.payload) openExtensionSettings(e.payload)
    })
    // 形态切换（设置里的开关 / 独立窗侧改动）：切到独立窗口时收起内嵌抽屉，二者互斥
    unlistenChatMode = await on<boolean>('chat-window-mode', (e) => {
      if (e.payload && chatOpen.value) {
        chatOpen.value = false
        persistChatPanelSize()
      }
    })
  }
  // 浏览器预览没有全局快捷键：保留应用内 Ctrl+K / Ctrl+Shift+K 兜底。
  // 桌面端一律走全局注册 + 事件分发（在这里注册应用内监听会与全局事件双触发、
  // 两次 toggle 相互抵消，表现为「按了没反应」）
  if (!isTauri()) {
    window.addEventListener('keydown', onSearchKeydown)
    window.addEventListener('keydown', onChatKeydown)
  }
  await restoreChatPanel()
  // 点击抽屉外部收起（捕获阶段，覆盖 main-area / 标题栏 / 侧边栏等一切抽屉外区域）
  document.addEventListener('pointerdown', onDocPointerDown, true)
})

let unlistenStickies: (() => void) | null = null
let unlistenCountdownFired: (() => void) | null = null
let unlistenCountdownsChanged: (() => void) | null = null
let unlistenNotesChanged: (() => void) | null = null
let unlistenSnippetsChanged: (() => void) | null = null
let unlistenTodosChanged: (() => void) | null = null
let unlistenTodoTagsChanged: (() => void) | null = null
let unlistenTodoRemind: (() => void) | null = null
let unlistenBallAction: (() => void) | null = null
let unlistenOpenChatSettings: (() => void) | null = null
let unlistenSearchShortcut: (() => void) | null = null
let unlistenChatShortcut: (() => void) | null = null
let unlistenCaptureShortcut: (() => void) | null = null
let unlistenNotesShortcut: (() => void) | null = null
let unlistenOpenExtSettings: (() => void) | null = null
let unlistenChatMode: (() => void) | null = null

onUnmounted(() => {
  store.stopOnlineMonitor()
  unlistenStickies?.()
  unlistenCountdownFired?.()
  unlistenCountdownsChanged?.()
  unlistenNotesChanged?.()
  unlistenSnippetsChanged?.()
  unlistenTodosChanged?.()
  unlistenTodoTagsChanged?.()
  unlistenTodoRemind?.()
  unlistenBallAction?.()
  unlistenOpenChatSettings?.()
  unlistenSearchShortcut?.()
  unlistenChatShortcut?.()
  unlistenCaptureShortcut?.()
  unlistenNotesShortcut?.()
  unlistenOpenExtSettings?.()
  unlistenChatMode?.()
  window.removeEventListener('suda-open-web-panel', onSudaWebPanelEvent)
  window.removeEventListener('keydown', onSearchKeydown)
  window.removeEventListener('keydown', onChatKeydown)
  document.removeEventListener('pointerdown', onDocPointerDown, true)
})

function hideBootSplash() {
  const el = document.getElementById('boot-splash')
  if (el && !el.classList.contains('hide')) {
    el.classList.add('hide')
    setTimeout(() => el.remove(), 450)
  }
}

// ---- 笔记选中与操作 ----
const activeNoteId = ref<number | null>(null)
const highlightTodoId = ref<number | null>(null)

/** 当前筛选目标：数字 = 某文件夹（含后代），`'unfiled'` = 未归类，`null` = 全部。 */
const activeNoteFolderId = ref<NoteFolderTarget>(null)
/** 「全部后代」id 集合 —— 真源是 Rust `note_folder::subtree_ids`，这里只是它的缓存。 */
const activeNoteFolderSubtree = ref<number[] | null>(null)
/** 回收站视图开关。 */
const trashVisible = ref(false)
const trashNotes = ref<Note[]>([])
const trashDays = ref(0)
/** 文件夹树显隐（收起 = 笔记列表独占左栏）。 */
const foldersVisible = ref(true)

/**
 * 文件夹筛选：点父级 = 连全部后代一起看（约定 76 同款口径）。
 *
 * ⚠️ `activeNoteFolderSubtree` 异步取，取回来之前**不筛** —— 否则切文件夹的
 *   第一帧会先闪一次「全部笔记」里恰好不含该文件夹的空列表，用户看到的是
 *   「点进去是空的，等一下才有内容」。
 * ⚠️ 未归类（`subtree = []`）必须单独判：`[]` 在 `includes` 里恒假，
 *   用它筛会得到空列表，而用户看到的是「未归类里一条都没有」（实际有）。
 */
const visibleNotes = computed(() =>
  filterNotesByFolder(store.state.notes, activeNoteFolderId.value, activeNoteFolderSubtree.value),
)

const activeNote = computed(
  // 用**全量**列表而不是 visibleNotes：筛掉之后如果当前笔记不在筛选内，
  // 编辑器会变成空白且没有任何提示，用户以为笔记丢了（它其实在另一个文件夹里）。
  () => store.state.notes.find((n) => n.id === activeNoteId.value) ?? null,
)

async function onCreateNote() {
  const n = await store.addNote('无标题笔记')
  activeNoteId.value = n.id
  // 悬浮球等入口触发时可能停在其它视图：新建后必须切到速记页，否则只见新建不见页面
  activeView.value = 'notes'
}

function onSelectNote(id: number) {
  activeNoteId.value = id
}

/**
 * 反链跳转（发布说明 ②）：把目标笔记从链接里点开。
 *
 * 目标若被当前的文件夹筛选 / 回收站挡住，要先把挡住它的那层撤掉 ——
 * 编辑器切过去了而列表里找不到对应那一行，看上去就是
 * 「我点的是 A，界面给的是 B」（`activeNote` 刻意读全量，见上面那条注释）。
 */
function onOpenNoteFromBacklink(id: number) {
  trashVisible.value = false
  if (!visibleNotes.value.some((n) => n.id === id)) {
    activeNoteFolderId.value = null
    activeNoteFolderSubtree.value = null
  }
  activeNoteId.value = id
}

async function onSelectNoteFolder(target: NoteFolderTarget) {
  activeNoteFolderId.value = target
  trashVisible.value = false
  if (target === null) {
    activeNoteFolderSubtree.value = null
    return
  }
  if (target === 'unfiled') {
    // 未归类没有文件夹 id，Rust 那侧也就没有子树可查 —— 传空数组（筛选的
    // 真正判定在 `filterNotesByFolder`，它把 `'unfiled'` 单独处理）。
    activeNoteFolderSubtree.value = []
    return
  }
  // 真源在 Rust；拿不到就留 null（= 不筛），绝不猜
  try {
    activeNoteFolderSubtree.value = await store.noteFolderSubtree(target)
  } catch (e) {
    // ⚠️ 目标也要一起复位：只把子树置 null 的话，树上仍高亮着那个（可能已被
    //   删掉的）文件夹，而列表显示的是全部 —— 正是「选中 A 显示 B」的反话。
    activeNoteFolderId.value = null
    activeNoteFolderSubtree.value = null
    showToast(e instanceof Error ? e.message : String(e))
  }
}

// ---- 速记导出 / 导入（发布说明 ⑤）----
//
// 入口放在速记视图的列表栏里（NoteList 头部两个图标钮）而不是设置页：
// 导出/导入的对象就是「我眼前这批笔记」，让用户为了导笔记先去设置页找一遍
// 是本末倒置。设置 → 数据与关于里另有一份完整数据备份，那不是同一件事。

/** 打包期间禁掉两个按钮，防连点开两个文件选择器 / 同时导两次。 */
const notesPortBusy = ref(false)

async function onExportNotes() {
  if (notesPortBusy.value) return
  if (!isTauri()) {
    showToast('浏览器预览下不可导出速记')
    return
  }
  notesPortBusy.value = true
  try {
    // 默认文件名带时间戳：同名覆盖时对话框已经会问，而用户在文件列表里
    // 也能一眼分出「这次导出是哪一份」。
    const stamp = new Date()
      .toISOString()
      .slice(0, 16)
      .replace(/[-:T]/g, '')
    const target = await save({
      title: '导出速记',
      defaultPath: `m-hub-notes-${stamp}.zip`,
      filters: [{ name: '速记导出包', extensions: ['zip'] }],
    })
    if (!target) return
    const s = await tauriApi.exportNotes(target)
    const bits = [`${s.notes} 篇笔记`]
    if (s.folders > 0) bits.push(`${s.folders} 个文件夹`)
    if (s.tags > 0) bits.push(`${s.tags} 个标签`)
    if (s.images > 0) bits.push(`${s.images} 张图片`)
    showToast(`已导出 ${bits.join('、')}`)
    // ⚠️ 有引用却取不到文件的历史孤儿图要说出来 —— 静默少几张图，
    //   用户只能在导入回去之后才发现，而那时他已经迁走了。
    if (s.images_missing > 0) {
      showToast(`另有 ${s.images_missing} 张图片在磁盘上已找不到，未包含在导出包中`, {
        duration: 6000,
      })
    }
  } catch (e) {
    showToast(e instanceof Error ? e.message : String(e))
  } finally {
    notesPortBusy.value = false
  }
}

async function onImportNotes() {
  if (notesPortBusy.value) return
  if (!isTauri()) {
    showToast('浏览器预览下不可导入速记')
    return
  }
  notesPortBusy.value = true
  try {
    const picked = await open({
      title: '导入速记',
      multiple: false,
      filters: [{ name: '速记导出包', extensions: ['zip'] }],
    })
    if (!picked || Array.isArray(picked)) return
    const s = await tauriApi.importNotes(picked)
    // 后端刻意没 emit `notes-changed`：它的监听者只刷笔记，而导入还**新建**了
    // 文件夹与标签 —— 只刷笔记的话它们不出现，而笔记已经挂在那些文件夹下
    // （表现为「导入的笔记筛不出来」）。见 store.reloadNotesFull。
    await store.reloadNotesFull()
    if (s.notes === 0) {
      showToast(
        s.skipped > 0
          ? `没有新增：这 ${s.skipped} 篇都已经存在了（导入是追加，不会覆盖）`
          : '这个包里没有笔记',
        { duration: 5000 },
      )
      return
    }
    const bits = [`导入 ${s.notes} 篇笔记`]
    if (s.skipped > 0) bits.push(`跳过重复 ${s.skipped} 篇`)
    if (s.folders > 0) bits.push(`新建 ${s.folders} 个文件夹`)
    if (s.tags > 0) bits.push(`新建 ${s.tags} 个标签`)
    if (s.images > 0) bits.push(`${s.images} 张图片`)
    showToast(bits.join('、'), { duration: 5000 })
  } catch (e) {
    showToast(e instanceof Error ? e.message : String(e))
  } finally {
    notesPortBusy.value = false
  }
}

async function openTrash() {
  activeNoteFolderId.value = null
  activeNoteFolderSubtree.value = null
  trashNotes.value = isTauri() ? await tauriApi.listTrashNotes() : []
  trashDays.value = isTauri() ? (store.state.config.notes_trash_days ?? 30) : 30
  trashVisible.value = true
}

/**
 * 删除笔记 → 进回收站（v0.8.0 起软删）。
 *
 * ⚠️ 撤销走 `restore_note` **还原原笔记**，不再 `addNote + saveNote` 重建。
 *   重建会丢 id，于是标签关联、图片、双链全部断掉 —— 表现为「撤销回来的笔记
 *   标签没了、图也不见了」，而用户明明点的是「撤销」。
 */
async function onDeleteNote(id: number) {
  const target = store.state.notes.find((n) => n.id === id)
  if (!target) return
  await store.removeNote(id)
  if (activeNoteId.value === id) activeNoteId.value = null
  showToast('笔记已移入回收站', {
    label: '撤销',
    onClick: async () => {
      try {
        const n = await tauriApi.restoreNote(id)
        await store.refreshNotes()
        activeNoteId.value = n.id
        showToast('已还原笔记')
      } catch (e) {
        showToast(e instanceof Error ? e.message : String(e))
      }
    },
  })
}

function onSaveNote(id: number, title: string, content: string) {
  store.saveNote(id, title, content)
}

// ---- 二次确认（宿主层单实例）----
//
// ConfirmDialog 是「visible + confirm/cancel 事件」的组件，不是 promise 函数。
// 每个危险操作各挂一个组件会随调用点线性增长（且多实例焦点陷阱会互相抢），
// 所以这里用**一个实例 + 一个 resolve 句柄**包成 `await askConfirm(...)`。
const confirmState = ref({
  visible: false,
  title: '',
  message: '',
  confirmText: '确认',
})
let confirmResolve: ((ok: boolean) => void) | null = null

function askConfirm(opts: { title: string; message: string; confirmText?: string }): Promise<boolean> {
  return new Promise((resolve) => {
    // 连点两次时旧的 resolve 被替换，旧 Promise 拿不到答复 → 永远 pending。
    // 先兑现掉再替换，调用方的 await 不会悬着。
    confirmResolve?.(false)
    confirmResolve = resolve
    confirmState.value = {
      visible: true,
      title: opts.title,
      message: opts.message,
      confirmText: opts.confirmText ?? '确认',
    }
  })
}

function settleConfirm(ok: boolean) {
  const r = confirmResolve
  confirmResolve = null
  confirmState.value.visible = false
  r?.(ok)
}

/** 回收站里的三个操作。**每一步都要刷新回收站列表**，否则行不动 = 用户以为没点上。 */
async function onRestoreNote(id: number) {
  try {
    await tauriApi.restoreNote(id)
    trashNotes.value = await tauriApi.listTrashNotes()
    await store.refreshNotes()
    showToast('已还原')
  } catch (e) {
    showToast(e instanceof Error ? e.message : String(e))
  }
}

async function onPurgeNote(id: number) {
  const t = trashNotes.value.find((n) => n.id === id)
  const ok = await askConfirm({
    title: '彻底删除这条笔记？',
    // 点名具体是哪一条 + 说清不可逆：泛泛的「确定吗」用户会不看就点掉，
    // 而这里点错的代价是数据永久没了。
    message: `「${t?.title ?? '这条笔记'}」将被永久删除，无法还原。`,
    confirmText: '永久删除',
  })
  if (!ok) return
  try {
    await tauriApi.purgeNote(id)
    trashNotes.value = await tauriApi.listTrashNotes()
    if (activeNoteId.value === id) activeNoteId.value = null
    await store.refreshNotes()
  } catch (e) {
    showToast(e instanceof Error ? e.message : String(e))
  }
}

async function onEmptyTrash() {
  const n = trashNotes.value.length
  const ok = await askConfirm({
    title: `清空回收站（${n} 条）？`,
    // ⚠️ 必须带上条数并用模板串：写死「这 n 条」正是用户读到的字面量，
    //   他会以为是占位符没替换 —— 而这里恰好是「点错即数据全没」的确认框。
    message: `这 ${n} 条笔记将被永久删除，无法还原。`,
    confirmText: '全部删除',
  })
  if (!ok) return
  try {
    await tauriApi.emptyNoteTrash()
    trashNotes.value = []
    await store.refreshNotes()
    showToast(`已清空 ${n} 条`)
  } catch (e) {
    showToast(e instanceof Error ? e.message : String(e))
  }
}

function formatTrashTime(iso: string | null): string {
  if (!iso) return '未知时刻'
  // ⚠️ `parseTimestamp` 回的是**毫秒数**，不是 Date —— 直接 `.getFullYear()`
  //   会 TypeError（回收站每一行都炸）。返回 0 视为解析失败。
  const ms = parseTimestamp(iso)
  if (!ms) return '未知时刻'
  const d = new Date(ms)
  const pad = (x: number) => String(x).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

// ---- 全局搜索 / 设置 ----
const searchVisible = ref(false)
/** 统一捕获框（2026-09-29 新增） */
const captureVisible = ref(false)
const promptManageVisible = ref(false)
// 正在配置内容的工作台「自定义速达」槽位 id（suda1..suda4；null = 弹窗关闭）
const sudaCustomEditId = ref<string | null>(null)
const settingsSection = ref('')

function onOpenTodo(t: Todo) {
  searchVisible.value = false
  highlightTodoId.value = t.id
  activeView.value = 'dashboard'
}

/**
 * 从全局搜索跳到某条提示词（2026-09-29）。
 *
 * 走「打开提示词管理」而不是新造一个详情弹窗：管理面板已经能编辑/复制，
 * 再做一个只读详情就是第二套渲染同一份数据的地方，改一处忘一处。
 * 但要先把标题带过去并高亮，否则用户点完不知道跳到了哪一条。
 */
const promptHighlightId = ref<number | null>(null)
function onOpenSnippet(s: Snippet) {
  searchVisible.value = false
  promptHighlightId.value = s.id
  openPromptManage()
}

/** 关闭提示词管理时清掉高亮标记：否则下次从别处打开面板，
 *  上次搜的那一条还亮着，看起来像「莫名其妙被选中」。 */
function onClosePromptManage() {
  promptManageVisible.value = false
  promptHighlightId.value = null
}

/**
 * 从全局搜索跳到某个倒计时（2026-09-29）。
 *
 * 倒计时没有独立视图，它就是工作台上的一张卡。所以这里把视图切到工作台；
 * 卡片本身在不在版面上由用户自己决定，所以额外提示一句 ——
 * 静默切到一个看不到任何变化的界面，是最差的处理。
 */
function onOpenCountdown(c: Countdown) {
  searchVisible.value = false
  if (activeView.value !== 'dashboard') activeView.value = 'dashboard'
  if (!layout.placements.value.some((p) => p.id === 'countdown')) {
    showToast(
      `「${c.name}」在工作台版面上没有倒计时卡片，需要先加上`,
      { label: '去添加', onClick: openLayoutEditor },
    )
  } else {
    showToast(`已切到工作台：${c.name}`)
  }
}

/** 捕获保存成功后的提示：明确说「记到哪了」，而不是只说「已保存」。
 *  自动判断一定有猜错的时候，猜错时用户唯一能做的就是靠这句话回头找。 */
function onCaptured(dest: string) {
  const label: Record<string, string> = {
    note: '速记', todo: '待办', snippet: '提示词', countdown: '倒计时', url: '速达',
  }
  showToast(`已记到「${label[dest] ?? dest}」`)
}

function onSearchKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault()
    searchVisible.value = !searchVisible.value
  }
}

// ---- AI 对话抽屉（支持上下左右四个方位）----
const chatOpen = ref(false)
const chatWidth = ref(420)
const chatHeight = ref(380)
// 方位取自配置（设置页可切换），default 回退右侧
const chatSide = computed(() => (store.state.config.chat_panel_side || 'right') as 'left' | 'right' | 'top' | 'bottom')
// 左右方位用宽度、上下方位用高度（传给 dock 布局）
const chatDockStyle = computed(() =>
  chatSide.value === 'top' || chatSide.value === 'bottom'
    ? { height: chatHeight.value + 'px', width: '100%' }
    : { width: chatWidth.value + 'px', height: '100%' },
)

function toggleChat() {
  // 独立窗口形态（设置「AI 助手 → 以独立窗口打开」决定，与内嵌抽屉互斥）：
  // 唤起/收起后端常驻的对话小窗，抽屉状态不参与
  if (isTauri() && store.state.config.chat_window_mode) {
    void tauriApi.chatWindowToggle().catch(() => {})
    return
  }
  chatOpen.value = !chatOpen.value
  // 窗口开关状态不持久化：重启后始终默认收起，仅保存尺寸
  persistChatPanelSize()
}

function persistChatPanelSize() {
  if (!isTauri()) return
  void tauriApi.setChatPanel(chatWidth.value, chatHeight.value, chatOpen.value)
}

function onChatToggle() {
  toggleChat()
}

// 面板「去配置大模型」：跳转设置页并定位到 AI 助手分类
function onOpenChatSettings() {
  settingsSection.value = 'ai'
  if (chatOpen.value) toggleChat()
  activeView.value = 'settings'
}

/** 设置 → 扩展中心（本机源码目录的增删都在那里） */
function onOpenExtensionsView() {
  activeView.value = 'extensions'
}

/** 扩展中心「我的扩展」→「点击跳转」：跳设置页并定位到 Skills 分区（装扩展开发 Skill 的入口） */
function onOpenSkillsSettings() {
  settingsSection.value = 'skills'
  activeView.value = 'settings'
}

// ---- 扩展设置弹窗跳转（扩展页 mhub.openPermissions 的「去授权」落点） ----
// ExtensionCenter 不在 DOM 时（正看某个扩展的 view）无法直接弹它的设置弹窗：
// 这里切到扩展中心，把「要打开哪个扩展的设置」经 prop 递进去，列表加载完即弹。
const extensionSettingsJump = ref<{ id: string; nonce: number } | null>(null)
let extensionSettingsNonce = 0

function openExtensionSettings(extId: string) {
  extensionSettingsJump.value = { id: extId, nonce: ++extensionSettingsNonce }
  if (activeView.value !== 'extensions') activeView.value = 'extensions'
}

async function restoreChatPanel() {
  if (!isTauri()) return
  try {
    const [w, h] = await tauriApi.getChatPanel()
    chatWidth.value = w
    chatHeight.value = h
    // 启动时始终默认收起（开关状态不持久化）
    chatOpen.value = false
  } catch {
    // 忽略：命令未就绪时保持默认收起
  }
}

// 拖拽改尺寸后由 ChatPanel 回调同步本地状态（宽度/高度根据当前方位取对应值）
function onChatPanelResized(w: number, h: number) {
  chatWidth.value = w
  chatHeight.value = h
}

function onChatKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'k') {
    e.preventDefault()
    toggleChat()
  }
}

// ---- 点击抽屉外部收起（仅内嵌抽屉形态；独立小窗有自己的关闭逻辑）----
function closeChatDrawer() {
  if (!chatOpen.value) return
  chatOpen.value = false
  persistChatPanelSize()
}

// 用 pointerdown 的捕获阶段拿「按下」事件，比 click 更早，避免和抽屉内部按钮的
// click 抢时序。判定：
//   1) 抽屉内部（输入框/会话菜单/拖拽改尺寸手柄等）→ 不算外部；
//   2) 标记了 data-chat-opener 的对话入口（标题栏对话按钮）→ 不算外部，
//      交给它自己的 click 去切换，否则「按下先收起、click 再打开」会互相抵消。
function onDocPointerDown(e: PointerEvent) {
  if (!chatOpen.value) return
  const t = e.target as HTMLElement | null
  if (!t || typeof t.closest !== 'function') return
  // 抽屉内部 + 对话入口按钮都不算外部；
  // 另外两个是 Teleport 到 body 的浮层（会话菜单 / 模型下拉），它们不在 .chat-dock 里，
  // 但属于对话面板的操作——不豁免的话 pointerdown 会先把抽屉收起，导致「新建/删除对话」点了没反应。
  if (
    t.closest('.chat-dock') ||
    t.closest('[data-chat-opener]') ||
    t.closest('.cp-menu') ||
    t.closest('.app-select-menu')
  ) return
  closeChatDrawer()
}

async function onOpenResource(r: Resource) {
  searchVisible.value = false
  try {
    await store.launchResource(r.id)
  } catch (e) {
    showToast(`无法启动「${r.name}」：${String(e)}`)
  }
}

function onOpenNote(n: Note) {
  activeNoteId.value = n.id
  activeView.value = 'notes'
  searchVisible.value = false
}

// ---- 轻提示 ----
// 签名与两个可选形态的语义见 utils/toast.ts（唯一定义处）。
const toastMsg = ref('')
const toastAction = ref<{ label: string; onClick: () => void } | null>(null)
let toastTimer: ReturnType<typeof setTimeout> | null = null

function showToast(msg: string, opts?: ToastOptions) {
  // 按「有 label 即按钮」判定（不用 `duration` 在不在来判断）——两个形态都可能出现
  // 时，只有 label 是「一定意味着有个按钮」的那个字段。
  const action = opts?.label
    ? { label: opts.label, onClick: opts.onClick ?? (() => {}) }
    : null
  toastMsg.value = msg
  toastAction.value = action
  if (toastTimer) clearTimeout(toastTimer)
  toastTimer = setTimeout(() => {
    toastMsg.value = ''
    toastAction.value = null
  }, opts?.duration ?? (action ? 5000 : 2200))
}

provide('showToast', showToast)

</script>

<template>
  <div class="app-shell">
    <!-- 应用壁纸层：仅主窗口渲染，垫在全部内容之下、body 渐变之上（模糊作用于本层整体，见 ADR 0002） -->
    <div v-if="wallpaperSrc" class="wallpaper-layer" aria-hidden="true">
      <img
        class="wallpaper-img"
        :class="{ blur: store.state.config.wallpaper_blur && !store.state.config.wallpaper_immersive }"
        :src="wallpaperSrc"
        alt=""
        @error="onWallpaperError"
      />
      <!-- 壁纸蒙版：主题底色罩层，在壁纸鲜亮度与文字/图标对比度之间取平衡 -->
      <div class="wallpaper-veil" :style="{ opacity: wallpaperVeil }"></div>
      <div class="wallpaper-glow"></div>
    </div>
    <TitleBar
      @search="searchVisible = true"
      @chat="toggleChat"
      @focus="toggleFocusMode"
    />

    <div class="app-body" :class="{ collapsed: sidebarCollapsed }">
      <aside
        class="sidebar"
        :class="{ collapsed: sidebarCollapsed }"
        aria-label="应用侧栏"
      >
        <nav class="sidebar-nav" aria-label="主要导航">
          <button
            v-for="item in visibleNavigation"
            :key="item.id"
            class="sidebar-nav-item"
            :class="{ active: activeView === item.id }"
            :aria-current="activeView === item.id ? 'page' : undefined"
            :data-tip="item.label"
            type="button"
            @click="onNavClick(item.id)"
          >
            <span class="sidebar-nav-icon" aria-hidden="true">
              <component :is="item.icon" :size="16" :stroke-width="2" />
            </span>
            <span>{{ item.label }}</span>
          </button>
        </nav>

        <!-- 固定到侧栏的扩展：点击即在主区打开（view 形态） -->
        <div v-if="sidebarExtensions.length" class="sidebar-ext">
          <p class="sidebar-ext-label">扩展</p>
          <button
            v-for="ext in sidebarExtensions"
            :key="ext.id"
            class="sidebar-nav-item"
            :class="{ active: activeView === 'extension' && openedExtension?.id === ext.id }"
            :aria-current="activeView === 'extension' && openedExtension?.id === ext.id ? 'page' : undefined"
            :data-tip="ext.name"
            type="button"
            @click="openSidebarExtension(ext)"
          >
            <span class="sidebar-nav-icon" :class="{ 'has-img': !!ext.icon }" aria-hidden="true">
              <img
                v-if="ext.icon"
                class="sidebar-ext-img"
                :src="iconSrc(ext.icon)"
                :alt="ext.name"
                draggable="false"
              />
              <Puzzle v-else :size="15" :stroke-width="2" />
            </span>
            <span>{{ ext.name }}</span>
          </button>
        </div>

        <div class="sidebar-foot">
        <button
          class="sidebar-status"
          :class="{ active: activeView === 'extensions' }"
          type="button"
          aria-label="打开扩展中心"
          data-tip="扩展中心"
          @click="activeView = 'extensions'"
        >
          <Puzzle :size="15" :stroke-width="2" aria-hidden="true" />
          <span>扩展中心</span>
        </button>

        <button
          class="sidebar-status"
          :class="{ active: activeView === 'settings' }"
          type="button"
          aria-label="打开设置"
          data-tip="设置"
          @click="activeView = 'settings'"
        >
          <Settings :size="15" :stroke-width="2" aria-hidden="true" />
          <span>设置</span>
        </button>
        <button
          v-if="store.state.config.sidebar_toggle"
          class="sidebar-status sidebar-collapse"
          type="button"
          :aria-label="sidebarCollapsed ? '展开侧边栏' : '收起侧边栏'"
          :data-tip="sidebarCollapsed ? '展开侧边栏' : '收起侧边栏'"
          @click="toggleSidebar"
        >
          <component
            :is="sidebarCollapsed ? ChevronRight : ChevronLeft"
            :size="15"
            :stroke-width="2"
            aria-hidden="true"
          />
          <span v-if="!sidebarCollapsed">收起</span>
        </button>
        </div>
      </aside>

      <div class="main-area">
        <main class="workspace" aria-label="主工作区">
        <!-- 工作台：可自定义布局（12 列单元格网格，模块库编辑器） -->
        <!--
          `is-focus` 只在聚焦模式挂上：`.dash-wrap` 默认是 block（栅格版靠
          `gridTemplateRows: 1fr` 自己撑高），而聚焦栈需要 `flex: 1 1 auto`
          才能拿到剩余高度 —— 在 block 容器里那条 flex 完全无效，栈高会退化成
          内容高度，实测就是「两个折叠条 38px + 间隙把唯一的卡片挤成 0 高」。
          所以只在聚焦时改这个容器的 display，不动栅格那条路径。
        -->
        <div
          v-if="activeView === 'dashboard'"
          class="dash-wrap"
          :class="{ 'is-focus': focusOn }"
        >
          <!--
            聚焦模式：顶部切换条 + 竖排全宽的卡片栈。
            与下面栅格版**互斥**（v-if / v-else-if），两套布局不同时存在 ——
            同时存在的话折叠规则会打架（栅格版整段折叠、聚焦版逐个折叠）。
          -->
          <template v-if="focusOn">
            <FocusStrip
              :options="allModuleOptions"
              :pinned-ids="focusPins"
              closable
              @toggle="toggleFocusPin"
              @close="toggleFocusMode"
            />
            <div v-if="focusPins.length" class="focus-stack">
              <div
                v-for="id in focusPins"
                :key="id"
                class="focus-cell"
                :class="{ 'is-collapsed': isFocusCollapsed(id) }"
                :style="{ flexGrow: focusWeight(focusPlacement(id)!) }"
              >
                <DashCollapsedBar
                  v-if="isFocusCollapsed(id)"
                  :title="focusPlacement(id) ? dashPlacementTitle(focusPlacement(id)!) : id"
                  :hint="COLLAPSED_HINT[id] ?? '还没有内容'"
                  :action-label="`添加${id}内容`"
                  @action="onCollapsedAction(id)"
                />
                <component
                  :is="dashCardComponent(id)"
                  v-else-if="focusPlacement(id)"
                  v-bind="dashCardProps(focusPlacement(id)!)"
                  @go-suda="activeView = 'suda'"
                />
              </div>
            </div>
            <div v-else class="dash-empty">
              <p>聚焦模式下一个模块都没选，下面挑一个</p>
            </div>
          </template>

          <div
            v-else-if="layout.placements.value.length"
            class="dash-grid"
            :style="{
              gridTemplateRows: `repeat(${dashGridRows}, minmax(var(--dash-row-min), 1fr))`,
              '--dash-rows': dashGridRows,
            }"
          >
            <!--
              渲染用 `layout.effectivePlacements` 而不是 `placements`：
              空内容模块被压扁到 1 行、各段随后重排（见
              composables/dashLayoutGeometry.ts）。是**整段**折叠而非逐个折叠：
              部分折叠在手工栅格上必然留下空洞与错边。保存的布局不受影响，
              模块一有内容就长回原样。
            -->
            <div
              v-for="p in layout.effectivePlacements.value"
              :key="p.id"
              class="dash-cell"
              :class="{ 'is-collapsed': layout.collapsedIds.value.has(p.id) }"
              :style="dashCellStyle(p)"
            >
              <DashCollapsedBar
                v-if="layout.collapsedIds.value.has(p.id)"
                :title="dashPlacementTitle(p)"
                :hint="COLLAPSED_HINT[p.id] ?? '还没有内容'"
                :action-label="`在${dashPlacementTitle(p)}里添加`"
                @action="onCollapsedAction(p.id)"
              />
              <component
                :is="dashCardComponent(p.id)"
                v-else
                v-bind="dashCardProps(p)"
                @go-suda="activeView = 'suda'"
              />
            </div>
          </div>
          <div v-else class="dash-empty">
            <p>工作台还没有模块，去设置里自定义布局吧</p>
            <button class="pill-btn" type="button" @click="openLayoutEditor">自定义布局</button>
          </div>
        </div>

        <!-- 待办视图：标签筛选 / 月·周日历 / 周期待办（宽窗左右同屏，窄窗单栏） -->
        <TodoView v-else-if="activeView === 'todos'" />

        <!-- 速记：独立视图 -->
        <section v-else-if="activeView === 'notes'" class="view view-notes" tabindex="-1" aria-label="速记">
          <div class="notes-split">
            <!-- 文件夹树（发布说明 ①⑭）。收起时整栏消失，笔记列表补上它的宽度。 -->
            <NoteFolderTree
              v-if="foldersVisible"
              class="ns-tree"
              :active="activeNoteFolderId"
              @select="onSelectNoteFolder"
              @error="showToast($event)"
            />
            <div v-else class="ns-tree ns-tree--closed" aria-hidden="true" />

            <NoteList
              class="ns-list"
              :notes="visibleNotes"
              :active-id="activeNoteId"
              :folders-active="activeNoteFolderId !== null"
              :trash-count="trashNotes.length"
              :trash-open="trashVisible"
              :porting="notesPortBusy"
              @select="onSelectNote"
              @create="onCreateNote"
              @delete="onDeleteNote"
              @toggle-folders="foldersVisible = !foldersVisible"
              @open-trash="openTrash"
              @export="onExportNotes"
              @import="onImportNotes"
            />
            <NoteEditor
              class="ns-editor"
              :note="activeNote"
              @save="onSaveNote"
              @delete="onDeleteNote"
              @open-note="onOpenNoteFromBacklink"
            />
          </div>

          <!-- 回收站视图：盖在编辑器上（它是「另一个地方」，不是一篇笔记）。 -->
          <div v-if="trashVisible" class="trash-view">
            <header class="tv-header">
              <h2 class="tv-title">回收站</h2>
              <span class="tv-sub">
                保留 {{ trashDays }} 天<template v-if="trashDays === 0">（永久保留）</template>
              </span>
              <button class="tv-close" aria-label="关闭回收站" @click="trashVisible = false">
                <X :size="15" :stroke-width="2.2" />
              </button>
            </header>

            <p v-if="trashNotes.length === 0" class="tv-empty">回收站是空的</p>

            <div v-else class="tv-body">
              <div v-for="n in trashNotes" :key="n.id" class="tv-row">
                <div class="tv-main">
                  <span class="tv-name">{{ n.title }}</span>
                  <span class="tv-time">删除于 {{ formatTrashTime(n.deleted_at) }}</span>
                </div>
                <div class="tv-acts">
                  <button class="tv-act" @click="onRestoreNote(n.id)">还原</button>
                  <button class="tv-act danger" @click="onPurgeNote(n.id)">彻底删除</button>
                </div>
              </div>
            </div>

            <footer v-if="trashNotes.length > 0" class="tv-foot">
              <button class="tv-act danger" @click="onEmptyTrash">清空回收站</button>
            </footer>
          </div>

          <ConfirmDialog
            :visible="confirmState.visible"
            :title="confirmState.title"
            :message="confirmState.message"
            confirm-text="永久删除"
            tone="danger"
            @confirm="settleConfirm(true)"
            @cancel="settleConfirm(false)"
          />
        </section>

        <!-- 速达：独立视图 -->
        <section v-else-if="activeView === 'suda'" class="view view-suda" tabindex="-1" aria-label="速达">
          <Suda />
        </section>

        <!-- 速达网页内嵌面板（ADR 0011）：工具栏是 DOM，内容区是预创建子 webview 的空白位 -->
        <section
          v-else-if="activeView === 'suda-web'"
          class="view view-suda-web"
          tabindex="-1"
          aria-label="应用内浏览器"
        >
          <SudaWebPanel
            v-if="sudaWebPanel"
            :resource-id="sudaWebPanel.id"
            :url="sudaWebPanel.url"
            :name="sudaWebPanel.name"
            @close="closeSudaWebPanel"
          />
        </section>

        <!-- 扩展中心：独立视图 -->
        <section v-else-if="activeView === 'extensions'" class="view view-extensions" tabindex="-1" aria-label="扩展中心">
          <ExtensionCenter
            :jump-settings="extensionSettingsJump"
            @open="onOpenExtension"
            @open-surface="(ext, surface) => openExtensionSurface(ext.id, surface)"
            @changed="onExtensionsChanged"
            @open-skills="onOpenSkillsSettings"
          />
        </section>

        <!-- 扩展运行视图：主区渲染扩展入口（iframe + window.mhub 桥 API） -->
        <section v-else-if="activeView === 'extension'" class="view view-extension" tabindex="-1" aria-label="扩展">
          <div class="ext-toolbar">
            <span class="ext-toolbar-name">{{ openedExtension?.name ?? '扩展' }}</span>
            <div class="ext-toolbar-spacer" />
            <button class="icon-btn" type="button" title="在窗口打开" aria-label="在窗口打开" @click="openExtensionWindow">
              <AppWindow :size="15" :stroke-width="2" />
            </button>
            <button class="icon-btn" type="button" title="在抽屉打开" aria-label="在抽屉打开" @click="openExtensionDrawer">
              <PanelRight :size="15" :stroke-width="2" />
            </button>
          </div>
          <ExtensionView
            v-if="openedExtension"
            :ext-id="openedExtension.id"
            :surface="openedExtension.surface"
            :reload-key="extensionReloadTick"
            @close="closeExtension"
          />
        </section>

        <!-- 对话：独立视图（完整视图，与右侧面板共用会话数据） -->
        <section v-else-if="activeView === 'chat'" class="view view-chat" tabindex="-1" aria-label="对话">
          <div class="view-chat-hint">
            <MessageSquare :size="20" :stroke-width="1.8" />
            <p>抽屉面板已是最佳对话形态，可点击标题栏对话按钮或按 {{ chatShortcutLabel }} 唤起（方位可在设置 → AI 助手调整）。</p>
          </div>
        </section>

        <!-- 设置：独立视图 -->
        <section v-else-if="activeView === 'settings'" class="view view-settings" tabindex="-1" aria-label="设置">
          <SettingsView
            :initial-section="settingsSection"
            @open-layout-editor="openLayoutEditor"
            @open-extensions="onOpenExtensionsView"
          />
        </section>

        <!-- 自定义布局编辑器：独立视图（从设置进入，完成后回主页面） -->
        <section v-else class="view view-layout-editor" tabindex="-1" aria-label="自定义布局">
          <DashboardLayoutEditor @done="activeView = 'dashboard'" />
        </section>
        </main>

        <!-- AI 对话抽屉（覆盖式，悬浮在内容上方，可从上下左右滑入，尺寸可拖拽） -->
        <Transition :name="`chat-drawer-${chatSide}`">
          <div
            v-if="chatOpen"
            class="chat-dock"
            :class="`dock-${chatSide}`"
            :style="{
              opacity: store.state.config.chat_panel_opacity ?? 1,
              ...chatDockStyle,
            }"
          >
            <ChatPanel
              :side="chatSide"
              @toggle="onChatToggle"
              @open-model-settings="onOpenChatSettings"
              @resized="onChatPanelResized"
            />
          </div>
        </Transition>

        <!-- 扩展抽屉（覆盖式，右侧滑出） -->
        <Transition name="chat-drawer">
          <div v-if="drawerExtension" class="ext-drawer">
            <div class="ext-drawer-header">
              <span class="ext-drawer-title">{{ drawerExtension.name }}</span>
              <button
                class="ext-drawer-close"
                type="button"
                aria-label="关闭抽屉"
                @click="closeExtensionDrawer"
              >
                <ChevronRight :size="16" :stroke-width="2" aria-hidden="true" />
              </button>
            </div>
            <div class="ext-drawer-body">
              <ExtensionView
                :ext-id="drawerExtension.id"
                :surface="drawerExtension.surface"
                :reload-key="extensionReloadTick"
              />
            </div>
          </div>
        </Transition>
      </div>
    </div>

    <GlobalSearch
      :visible="searchVisible"
      @close="searchVisible = false"
      @open-resource="onOpenResource"
      @open-note="onOpenNote"
      @open-todo="onOpenTodo"
      @open-snippet="onOpenSnippet"
      @open-countdown="onOpenCountdown"
    />
    <QuickCapture
      :visible="captureVisible"
      @close="captureVisible = false"
      @saved="onCaptured"
    />
    <PromptManageDialog
      :visible="promptManageVisible"
      :highlight-id="promptHighlightId"
      @close="onClosePromptManage"
    />
    <SudaCustomEditDialog
      v-if="sudaCustomEditId"
      :slot-id="sudaCustomEditId"
      :visible="!!sudaCustomEditId"
      @close="sudaCustomEditId = null"
    />

    <Transition name="toast">
      <div v-if="toastMsg" class="toast">
        <span class="toast-msg" :title="toastMsg">{{ toastMsg }}</span>
        <button
          v-if="toastAction"
          class="toast-action"
          type="button"
          @click="toastAction.onClick()"
        >
          {{ toastAction.label }}
        </button>
      </div>
    </Transition>
  </div>
  <!-- 缩放边缘：必须是 .app-shell 的**兄弟**而非后代 —— 壳上有 contain:paint
       （圆角裁切所必需），后代会被它裁掉。详见组件头注释。 -->
  <WindowResizeHandles />
</template>

<style scoped>
/* 应用壁纸层：z-index -1 加入根层叠上下文负相位，盖过 body 渐变、垫在全部内容之下 */
.wallpaper-layer {
  position: fixed;
  /* 父级 .app-shell 的 contain:paint 已会裁到圆角，这里再声明一次是为了让
   「全出血层必须圆角」这条不变量自洽，不依赖对父级的具体实现 */
  border-radius: var(--window-radius);
  inset: 0;
  z-index: -1;
  overflow: hidden;
  pointer-events: none;
}
.wallpaper-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  object-position: center;
}
.wallpaper-img.blur {
  filter: blur(8px);
  /* 收进模糊产生的四周透明羽化 */
  transform: scale(1.06);
}
/* 壁纸蒙版：取主题中性底色（亮色近白/暗色近黑，随模式联动），文字对比度兜底 */
.wallpaper-veil {
  position: absolute;
  inset: 0;
  background: linear-gradient(150deg, var(--bg-base-a) 0%, var(--bg-base-b) 100%);
}
/* 主题光晕叠加：壁纸替换渐变背景但保留主题氛围（--glow-* 随模式/预设联动） */
.wallpaper-glow {
  position: absolute;
  inset: 0;
  background:
    radial-gradient(1200px 800px at 12% -8%, var(--glow-a), transparent 55%),
    radial-gradient(1000px 700px at 100% 4%, var(--glow-b), transparent 55%),
    radial-gradient(1200px 900px at 55% 118%, var(--glow-c), transparent 55%);
}

/* 根容器 —— 同时是「主窗口的窗口本体」。
 *
 * 页面底色（--app-bg / 渐变预设）从 body 搬到了这里，因为只有带 border-radius
 * 的元素才能把圆角外的像素裁掉；body 是铺满视口的矩形，留在那里会漏出方角。
 * 对应 tauri.conf.json 主窗的 `transparent: true`。
 *
 * `contain: paint` 是**必需**的，不是优化：壁纸层是 `position: fixed`（inset:0），
 * 而 fixed 元素默认相对视口定位，**不会被祖先的 overflow/border-radius 裁剪**——
 * 不加 contain 的话就是「内容圆了、壁纸还是方的」，圆角处露出窗口外的桌面。
 * `contain: paint` 让本元素成为 fixed 后代的包含块，壁纸随之被裁成同款圆角。
 *
 * 早先这里还有 `margin: var(--window-shadow-margin)`（自绘窗口阴影的容身处）：
 * AppKit 不给透明窗口画系统阴影（约定 69 已实机取色证伪 `setHasShadow`），
 * 而 CSS 阴影只能画在窗口以内 —— 窗口不外扩就没有落影的位置。
 * 但那圈带子是**透明**的，桌面的壁纸与图标会从窗口四周直接透进来形成一圈
 * 「玻璃框」。取舍变成「有阴影」与「不漏桌面」二选一，现取后者，
 * 外扩带连同 `WINDOW_SHADOW_MARGIN` 一起移除（2026-09-29）。
 * 现在最大化只需把圆角归零，见 style.css 的 `html[data-window-maximized]`。
 */
.app-shell {
  /* 铺满整个窗口：外扩带 2026-09-29 移除后，窗口 inner 尺寸**就是**可视区尺寸，
     不再有任何 margin / 高度扣减。曾经这里是
     `height: calc(100% - var(--window-shadow-margin) * 2)` + `margin` ——
     那圈 32px 透明带会让桌面壁纸从窗口四周直接透进来，形成一圈「玻璃框」。 */
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--app-bg, var(--bg-page-surface));
  border-radius: var(--window-radius);
  /* 不再有 box-shadow：透明窗口拿不到 AppKit 的系统阴影，而 CSS 阴影画在
     窗口以内 —— 要让阴影有地方画就必须留透明带，带子又必然漏桌面。
     「有阴影」与「不漏桌面」在透明窗下二选一，此处选了后者。 */
  overflow: hidden;
  position: relative;
  contain: paint;
}

.app-body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 220px minmax(0, 1fr);
  transition: grid-template-columns 0.18s ease-out;
}
.app-body.collapsed {
  grid-template-columns: 56px minmax(0, 1fr);
}
.sidebar {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  /* 顶部贴齐标题栏下沿，菜单与内容卡片头部对齐 */
  padding: 0 var(--space-3) var(--space-3);
  background: transparent;
  overflow: hidden;
  transition: padding 0.18s ease-out;
}
.sidebar.collapsed {
  padding: 0 8px var(--space-3);
}
/* 铬件（侧栏）始终全透明：与背景（渐变/壁纸）构成同一个连续平面，表面只属于卡片（ADR 0003） */
/* 侧栏 hover 气泡是瞬态表面（实底 + 自带 blur），不吃壁纸态的文字光晕 */
html[data-wallpaper='1'] .sidebar [data-tip]::after,
html[data-wallpaper='1'] .title-bar [data-tip]::after {
  text-shadow: none;
}
.sidebar-nav {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}
.sidebar-nav-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 36px;
  padding: 0 var(--space-2);
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-2);
  font-size: 0.8125rem;
  font-weight: 600;
  text-align: left;
  cursor: pointer;
  transition: background 150ms ease-out, color 150ms ease-out;
}
.sidebar-nav-item:hover,
.sidebar-nav-item.active {
  background: var(--brand-50);
  color: var(--brand-500);
}
.sidebar-nav-item.active {
  font-weight: 700;
}
.sidebar-nav-icon {
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  transition: background 150ms ease-out, color 150ms ease-out, box-shadow 150ms ease-out;
}
.sidebar-status {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 34px;
  padding: 0 var(--space-2);
  background: transparent;
  color: var(--text-3);
  font-size: 0.75rem;
  text-align: left;
  border: 0;
  cursor: pointer;
}
.sidebar-nav + .sidebar-status { margin-top: auto; }
.sidebar-foot {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}
/* 固定到侧栏的扩展组：独立区块，与主导航视觉一致，可滚动 */
.sidebar-ext {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  min-height: 0;
  overflow-y: auto;
}
.sidebar-ext-label {
  margin: 0;
  padding: 0 var(--space-2);
  font-size: 0.6875rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--text-3);
}
/* 扩展图标：无图回退（Puzzle）才用中性软底 + 细描边；有图时不加任何托底，按图片原样显示 */
.sidebar-ext .sidebar-nav-icon {
  background: var(--bg-card-soft);
  box-shadow: inset 0 0 0 1px var(--border-soft);
}
.sidebar-ext .sidebar-nav-icon.has-img {
  background: transparent;
  box-shadow: none;
}
.sidebar-ext-img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  padding: 4px;
}
.sidebar.collapsed .sidebar-ext {
  align-items: center;
  gap: 6px;
}
.sidebar.collapsed .sidebar-ext-label {
  display: none;
}
.sidebar.collapsed .sidebar-ext .sidebar-nav-icon {
  overflow: hidden;
}
.sidebar-status:hover { color: var(--text-1); }
.sidebar-status.active {
  background: var(--brand-50);
  color: var(--brand-500);
}

/* 收起态：只保留图标 */
.sidebar.collapsed .sidebar-nav-item,
.sidebar.collapsed .sidebar-status {
  justify-content: center;
  padding: 0;
}
.sidebar.collapsed .sidebar-nav-item {
  width: 40px;
  min-height: 40px;
  border-radius: 50%;
}
.sidebar.collapsed .sidebar-nav-item:hover {
  background: color-mix(in srgb, var(--brand-500) 8%, transparent);
}
.sidebar.collapsed .sidebar-nav-item.active,
.sidebar.collapsed .sidebar-nav-item.active:hover {
  background: color-mix(in srgb, var(--brand-500) 10%, transparent);
}
.sidebar.collapsed .sidebar-nav-item.active .sidebar-nav-icon {
  background: color-mix(in srgb, var(--brand-500) 10%, transparent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--brand-500) 10%, transparent);
}
.sidebar.collapsed .sidebar-nav-item > span:not(.sidebar-nav-icon),
.sidebar.collapsed .sidebar-status span {
  display: none;
}
.sidebar.collapsed .sidebar-status {
  min-height: 34px;
}
.sidebar.collapsed .sidebar-nav {
  align-items: center;
  gap: 6px;
}

/* 收起态：hover 图标时在右侧显示名称气泡 */
.sidebar.collapsed {
  overflow: visible;
}
.sidebar.collapsed .sidebar-status {
  position: relative;
}
.sidebar.collapsed [data-tip]::after {
  content: attr(data-tip);
  position: absolute;
  left: calc(100% + 12px);
  top: 50%;
  transform: translateY(-50%);
  padding: 4px 10px;
  font-size: 0.75rem;
  font-weight: 500;
  white-space: nowrap;
  color: var(--text-1);
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  box-shadow: var(--shadow-card);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.12s ease-out;
  z-index: 60;
}
.sidebar.collapsed [data-tip]:hover::after {
  opacity: 1;
  transition-delay: 0.3s;
}

/* 主工作区：抽屉悬浮在内容上方，不再挤压左侧内容 */
.main-area {
  position: relative;
  min-width: 0;
  min-height: 0;
  display: flex;
  align-items: stretch;
  overflow: hidden;
}
.main-area .workspace {
  flex: 1;
  min-width: 0;
}

/* 覆盖式抽屉：absolute 悬浮于工作区之上，支持上下左右四个方位滑入 */
.main-area .chat-dock {
  position: absolute;
  z-index: 40;
  pointer-events: none;
}
.main-area .chat-dock :deep(.chat-panel) {
  height: 100%;
  width: 100%;
  pointer-events: auto;
}
.main-area .chat-dock.dock-right {
  top: 0;
  right: 0;
  bottom: 0;
}
.main-area .chat-dock.dock-left {
  top: 0;
  left: 0;
  bottom: 0;
}
.main-area .chat-dock.dock-top {
  top: 0;
  left: 0;
  right: 0;
}
.main-area .chat-dock.dock-bottom {
  bottom: 0;
  left: 0;
  right: 0;
}

/* 四个方位的滑入/滑出过渡（配合 ChatPanel 内部尺寸拖拽，动画只做 transform） */
.chat-drawer-right-enter-active,
.chat-drawer-right-leave-active,
.chat-drawer-left-enter-active,
.chat-drawer-left-leave-active,
.chat-drawer-top-enter-active,
.chat-drawer-top-leave-active,
.chat-drawer-bottom-enter-active,
.chat-drawer-bottom-leave-active {
  transition: transform 0.24s ease-out, opacity 0.18s ease-out;
}
.chat-drawer-right-enter-from,
.chat-drawer-right-leave-to {
  transform: translateX(100%);
}
.chat-drawer-left-enter-from,
.chat-drawer-left-leave-to {
  transform: translateX(-100%);
}
.chat-drawer-top-enter-from,
.chat-drawer-top-leave-to {
  transform: translateY(-100%);
}
.chat-drawer-bottom-enter-from,
.chat-drawer-bottom-leave-to {
  transform: translateY(100%);
}

/* 工作台布局：12 列 fr 比例网格 + 行高 1fr 均分填满（缩放/分辨率只改每格像素值，布局结构不变）。
   行高带下限（--dash-row-min）：窗口够高时仍是 1fr 撑满、不滚动、不留白；窗口过矮时不再把每格
   压到几十像素后裁掉卡片内容，而是让 .dash-wrap 出现纵向滚动条兜底。 */
.dash-wrap {
  position: relative;
  height: 100%;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
}
.dash-grid {
  display: grid;
  grid-template-columns: repeat(12, minmax(0, 1fr));
  gap: var(--space-4);
  padding: 0 20px 20px 0;
  height: 100%;
  --dash-row-min: 36px;
  min-height: calc(var(--dash-row-min) * var(--dash-rows) + var(--space-4) * (var(--dash-rows) - 1) + 20px);
}
/* 聚焦模式（2026-09-29）：竖排卡片栈。
   刻意不用 12 列栅格：聚焦的价值就是每张卡拿到**全部**宽度，
   沿用栅格的话挑三张出来还是各占 1/12，等于没聚焦。
   高度用 flex-grow 比例（取自各 variant 的 idealH），于是「谁多高」由模块自己定。 */
/* 聚焦模式把 wrap 变成竖向 flex 容器，好让下面那个 flex:1 的栈拿到剩余高度。
   仅在 .is-focus 时生效，栅格版那条路径的布局完全不受影响。 */
.dash-wrap.is-focus {
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.focus-stack {
  display: flex;
  flex-direction: column;
  gap: var(--space-3, 12px);
  flex: 1 1 auto;
  min-height: 0;
  /* 兜底：选中的模块太多、或窗口太矮时改为滚动。
     没有这一条的话，多个 38px 的折叠条会先把空间吃光，
     真正有内容的卡片被挤成 0 高 —— 「看不见」是最坏的结果，宁可滚。 */
  overflow-y: auto;
}
.focus-cell {
  /* flex-basis: 0 + grow 比例 ⇒ 高度严格按比例分配，窗口缩放只改每张卡的像素高度，
     比例不变 —— 与栅格版「不滚动、不留白」的口径一致 */
  flex: 1 1 0;
  display: flex;
  flex-direction: column;
}
/* 有内容的卡片**必须**有下限高度：否则在极矮窗口里它会被折叠条挤成 0 高，
   用户看到的是「我明明选了这个模块，它却不见了」。 */
.focus-cell:not(.is-collapsed) {
  min-height: 140px;
}
/* 空内容模块压成一条：固定高度、不参与分配 */
.focus-cell.is-collapsed {
  flex: 0 0 auto;
  height: 38px;
  min-height: 0;
}

.dash-cell {
  min-width: 0;
  min-height: 0;
  position: relative;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  /* 容器查询容器：卡片内 cq 单位随格子缩放（形态化卡片的核心前提） */
  container-type: size;
}
.dash-cell > * {
  flex: 1;
  min-height: 0;
}
.dash-empty {
  height: 100%;
  min-height: 240px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-3);
  font-size: 0.8125rem;
}

/* 独立视图 */
.view {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* 速记视图：两栏布局（仅右下外边距，左上贴边与原版一致） */
.view-notes {
  padding: 0 20px 20px 0;
  /* 回收站面板是它的定位祖先（absolute inset 定位） */
  position: relative;
}
.view-suda {
  padding: 0 20px 20px 0;
}
.view-settings {
  padding: 0 20px 20px 0;
}
/* 扩展中心：覆盖组件内四边 padding，仅保留右下外边距（左上贴边与原版一致） */
.view-extensions :deep(.extension-center) {
  padding: 0 20px 20px 0;
}
/* 扩展运行视图：仅右下外边距 */
.view-extension {
  padding: 0 20px 20px 0;
}
.ext-toolbar {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 0 12px;
}
.ext-toolbar-spacer {
  flex: 1;
}
.ext-toolbar-name {
  font-size: 0.8125rem;
  font-weight: 650;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ext-toolbar .icon-btn {
  width: 30px;
  height: 30px;
  color: var(--text-3);
}

/* 扩展抽屉：absolute 悬浮于工作区之上，右侧滑入 */
.ext-drawer {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  z-index: 45;
  width: 480px;
  max-width: 82vw;
  display: flex;
  flex-direction: column;
  background: var(--bg-card-solid);
  border-left: 1px solid var(--border-strong);
  box-shadow: var(--shadow-dock);
  pointer-events: auto;
}
/* 壁纸态：抽屉是瞬态表面，改用真实取景模糊 + 玻璃底 ——
   否则那块 0.88 的不透明白会把壁纸切成一块死白（它不在 main 子树里，
   拿不到透底态对 --bg-card-solid 的覆盖，取的是 root 的 0.88 白）。 */
html[data-wallpaper='1'] .ext-drawer {
  background: var(--frost-surface);
  backdrop-filter: blur(18px) saturate(1.15);
  border-left-color: var(--border-soft);
}
/* 真实透底（玻璃透明度 < 0.9 或沉浸模式）：去掉白描边只留中性落影，文字加一档光晕（口径同 .card） */
html[data-wallpaper-clear='1'] .ext-drawer {
  border-left-color: transparent;
  box-shadow: -8px 0 30px rgba(0, 0, 0, 0.2);
  text-shadow: 0 1px 2px rgba(255, 255, 255, 0.5), 0 0 4px rgba(255, 255, 255, 0.3);
}
html[data-theme='dark'][data-wallpaper-clear='1'] .ext-drawer {
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5), 0 0 4px rgba(0, 0, 0, 0.3);
}
.ext-drawer-header {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border-soft);
}
.ext-drawer-title {
  font-size: 0.875rem;
  font-weight: 650;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ext-drawer-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.ext-drawer-close:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.ext-drawer-body {
  flex: 1;
  min-height: 0;
  display: flex;
}
.ext-drawer-body :deep(.extension-view) {
  flex: 1;
  min-width: 0;
  min-height: 0;
}
.view-layout-editor {
  padding: 0 20px 20px 0;
}
/* 三栏：文件夹树 / 笔记列表 / 编辑器。
   ⚠️ 必须用显式类名，不能沿用 `:first-child` / `:last-child` ——
   树那栏可以被收起（渲染成 .ns-tree--closed 的空壳），一旦子元素数量
   变化，伪类就会指到错误的栏上，表现为「收起树之后编辑器顶掉了列表」。 */
.notes-split {
  display: flex;
  gap: 14px;
  height: 100%;
  min-height: 0;
}
.notes-split .ns-tree {
  flex: 0 0 172px;
  min-width: 0;
}
/* 收起态保留一个 0 宽的占位节点，让「三栏」的结构恒定 */
.notes-split .ns-tree--closed {
  flex: 0 0 0;
  padding: 0;
  overflow: hidden;
}
.notes-split .ns-list {
  flex: 0 0 300px;
  min-width: 0;
}
.notes-split .ns-editor {
  flex: 1;
  min-width: 0;
}
/* 收起文件夹栏时把列表放宽，别留一条空隙 */
.notes-split:has(.ns-tree--closed) .ns-list {
  flex: 0 0 340px;
}

/* ── 回收站视图（盖在编辑器之上：它是「另一个地方」，不是一篇笔记）── */
.trash-view {
  position: absolute;
  inset: 0 0 0 auto;
  width: min(560px, 70%);
  display: flex;
  flex-direction: column;
  z-index: 20;
  padding: 18px;
  background: var(--bg-card-solid);
  border-left: 1px solid var(--border-soft);
  box-shadow: var(--shadow-card);
  border-radius: var(--radius-lg) 0 0 var(--radius-lg);
}
.tv-header {
  display: flex;
  align-items: baseline;
  gap: 10px;
  margin-bottom: 12px;
}
.tv-title {
  font-size: 16px;
  font-weight: 650;
  color: var(--text-1);
}
.tv-sub {
  font-size: 12px;
  color: var(--text-4);
}
.tv-close {
  margin-left: auto;
  display: flex;
  padding: 4px;
  border: none;
  border-radius: 6px;
  background: none;
  color: var(--text-3);
  cursor: pointer;
}
.tv-close:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.tv-empty {
  padding: 32px 0;
  font-size: 13px;
  color: var(--text-4);
  text-align: center;
}
.tv-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.tv-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: 8px;
  background: var(--bg-card-soft);
}
.tv-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.tv-name {
  font-size: 13px;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tv-time {
  font-size: 11px;
  color: var(--text-4);
}
.tv-acts {
  display: flex;
  gap: 6px;
  flex: 0 0 auto;
}
.tv-act {
  padding: 4px 10px;
  font-size: 12px;
  color: var(--text-2);
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-radius: 6px;
  cursor: pointer;
}
.tv-act:hover {
  border-color: var(--brand-500);
  color: var(--brand-500);
}
.tv-act.danger:hover {
  border-color: var(--c-red);
  color: var(--c-red);
}
.tv-foot {
  margin-top: 12px;
  display: flex;
  justify-content: flex-end;
}
.view-chat-hint {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  color: var(--text-3);
  text-align: center;
  padding: 0 var(--space-6);
}
.view-chat-hint svg {
  color: var(--text-3);
  opacity: 0.6;
}
.view-chat-hint p {
  font-size: 0.8125rem;
  line-height: 1.6;
  max-width: 360px;
}

@media (max-width: 1100px) {
  .workspace { padding: 0 10px 10px 0; }
  /* 窄窗口下视图外边距交给 workspace，避免叠加 */
  .dash-grid { padding: 0; }
  .view-notes { padding: 0; }
  .view-suda { padding: 0; }
  .view-settings { padding: 0; }
}

/* 窄窗口：保持 6 列（拖拽坐标与压缩算法固定按 6 列计算），仅收窄外边距 */
@media (max-width: 720px) {
  .app-body { grid-template-columns: 1fr; }
  .app-body.collapsed { grid-template-columns: 1fr; }
  .sidebar { position: relative; max-height: 280px; }
  .sidebar.collapsed { padding: 0 var(--space-3) var(--space-3); }
  .sidebar.collapsed .sidebar-nav-item,
  .sidebar.collapsed .sidebar-status {
    justify-content: flex-start;
    padding: 0 var(--space-3);
  }
  .sidebar.collapsed .sidebar-nav-item span,
  .sidebar.collapsed .sidebar-status span {
    display: initial;
  }
  .workspace { padding: 0 var(--space-2) var(--space-2) 0; overflow-y: auto; }
}

/* 轻提示 */
.toast {
  position: fixed;
  top: 56px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 500;
  display: flex;
  align-items: center;
  gap: 12px;
  background: var(--text-1);
  color: var(--text-on-accent);
  font-size: 0.8125rem;
  font-weight: 500;
  padding: 9px 18px;
  border-radius: var(--radius-pill);
  box-shadow: var(--shadow-dock);
  max-width: 70vw;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: auto;
}
.toast-msg {
  overflow: hidden;
  text-overflow: ellipsis;
}
.toast-action {
  flex-shrink: 0;
  border: none;
  background: color-mix(in srgb, var(--text-on-accent) 10%, transparent);
  color: inherit;
  font-size: 0.75rem;
  font-weight: 700;
  padding: 3px 10px;
  border-radius: var(--radius-pill);
  cursor: pointer;
  transition: background 0.15s;
}
.toast-action:hover {
  background: color-mix(in srgb, var(--text-on-accent) 14%, transparent);
}
.toast-enter-active,
.toast-leave-active {
  transition: opacity 0.2s ease-out, transform 0.2s ease-out;
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(-8px);
}
</style>
