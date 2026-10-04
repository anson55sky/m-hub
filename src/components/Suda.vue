<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import {
  FilePlus,
  Globe,
  Loader2,
  Pencil,
  Plus,
  ScanSearch,
  Star,
  Trash2,
  Wrench,
  Bookmark,
  Check,
  CheckCheck,
  Monitor,
  ChevronLeft,
  ChevronRight,
} from 'lucide-vue-next'
import type { DesktopEntry } from '../api/tauri'
import { isTauri, tauriApi, type InstalledAppInfo, type InstalledBrowser, type Resource } from '../api/tauri'
import { categorize } from '../utils/categories'
import { useStore } from '../stores/workbench'
import { reportClientError } from '../utils/error-report'
import { accentOf, fileAccentOf, iconSrc, useResourceIcon } from '../composables/useResourceIcon'
import { useAdaptivePolling } from '../composables/useAdaptivePolling'
import { useSudaDrag } from '../composables/useSudaDrag'
import { ADMIN_LAUNCH_TERM } from '../utils/platform'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'
import SudaFormDialog from './SudaFormDialog.vue'
import SudaBookmarkDialog from './SudaBookmarkDialog.vue'
import SudaDesktopDialog from './SudaDesktopDialog.vue'
import SudaScanDialog from './SudaScanDialog.vue'

const store = useStore()
// ---- 批量管理（2026-10-03）----
//
// 多选态与浏览态**互斥**：进入多选后单击不再是「打开」而是「勾选」。
// ⚠️ 选中的 id 存 `Set` 而不是数组：勾选/取消是高频操作，几百条时数组的
//   `includes` 会明显卡；而「配合分类筛选全选」要的正是「并进来」，Set 更直白。
const selectMode = ref(false)
const selected = ref(new Set<number>())

function toggleSelect(id: number) {
  const next = new Set(selected.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  selected.value = next
}

/** 全选/取消只作用于**当前可见**的那些（跟着分类筛选走） */
function toggleSelectAllVisible() {
  const ids = visibleResources.value.map((r) => r.id)
  const allOn = ids.length > 0 && ids.every((id) => selected.value.has(id))
  const next = new Set(selected.value)
  for (const id of ids) {
    if (allOn) next.delete(id)
    else next.add(id)
  }
  selected.value = next
}

const allVisibleSelected = computed(
  () => visibleResources.value.length > 0 && visibleResources.value.every((r) => selected.value.has(r.id)),
)

function exitSelectMode() {
  selectMode.value = false
  selected.value = new Set()
}

/**
 * 批量删除：逐条删、**如实回报**成功与失败。
 *
 * ⚠️ 失败的那些**不静默**：只提示「已删除 N 条」的话，用户会以为全删干净了，
 *   而没删掉的条目还在列表里 —— 又一句与事实相反的话。故失败项留在选中态里，
 *   用户能立刻看到是哪几条。
 *
 * ⚠️ `removeResources` 内部是**串行**逐条删而不是 Promise.all：并发时中途失败
 *   会让已删的那些回滚不了，而调用方只收到一个异常、不知道删了几条。
 */
const batchBusy = ref(false)
async function batchRemove() {
  const ids = [...selected.value]
  if (!ids.length) return
  const label = ids.length === 1 ? '这一条' : `这 ${ids.length} 条`
  const ok = window.confirm(
    `删除${label}速达？\n\n只删速达里的这些记录，指向的应用 / 文件 / 文件夹本身不受影响。`,
  )
  if (!ok) return
  batchBusy.value = true
  try {
    const { ok: done, failed } = await store.removeResources(ids)
    if (!failed.length) {
      showToast(`已删除 ${done.length} 条`)
      exitSelectMode()
    } else {
      selected.value = new Set(failed.map((f) => f.id))
      showToast(`已删除 ${done.length} 条，${failed.length} 条失败：${failed[0]?.error ?? ''}`)
    }
  } finally {
    batchBusy.value = false
  }
}

/**
 * 「添加」的入口：在**当前正看着的那个分类**里点，就把新资源归入该分类（2026-10-04）。
 *
 * ⚠️ 三种视图要分清，不能一律预填：
 * · `activeSub` 是具体小类名 → 预填它（大类 `activeFilter` 也一并预填，
 *   否则会加出一个 kind 与所看大类不符的条目）；
 * · `activeSub === 'none'`（未归类）→ **不预填**：`category` 为 null 本身就是
 *   未归类，预填等于什么都不做；而预填一个大类里的默认小类反而会把它挪走；
 * · `activeSub === 'all'`（全部）→ **不预填**：那时替用户选一个分类是
 *   「自作主张」，用户可能就想放到未归类。
 */
function startAdd() {
  editing.value = null
  const kind = KIND_BY_FILTER[activeFilter.value]
  if (activeSub.value !== 'all' && activeSub.value !== 'none') {
    prefill.value = { category: activeSub.value, kind }
  } else {
    prefill.value = kind ? { kind } : null
  }
  formVisible.value = true
}

const KIND_BY_FILTER: Record<FilterKey, 'app' | 'web' | 'file' | undefined> = {
  全部: undefined,
  常用: undefined,
  应用: 'app',
  网页: 'web',
  文件: 'file',
}

// ---- 浏览器书签导入（2026-10-03）----
const bookmarkVisible = ref(false)

/**
 * 浏览器书签导入：按**文件夹层级**建速达小类（2026-10-04）。
 *
 * ⚠️ 层级原样保留（发布说明：「从浏览器导入书签时，文件夹的层级会原样保留
 *   (比如「开发前端」)」）。旧实现只取末级名，于是「书签栏/开发/前端」里
 *   的书签和「书签栏/生活/前端」里的混在同一个「前端」下。
 *
 * ⚠️ 逐条串行 + 中途建小类：建父级可能失败（重名/非法字符），那这一支的
 *   条目就归不到分类里 —— 逐条建并如实回报，用户才知道哪几条没进去。
 *   已存在的层级直接复用，不重复建。
 */
async function addBookmarks(
  items: { name: string; url: string; category: string }[],
) {
  let added = 0
  const failed: string[] = []
  const kind = 'web' as const

  /** 全路径 → 小类 id。逐段建（缺哪段建哪段），已有的直接取 id */
  async function ensurePath(browserPath: string): Promise<number | null> {
    // ⚠️ **剥掉浏览器最外层**（「书签栏」/「其他书签」/Firefox 的根）。
    //   那一层不是用户建的分类，留在速达里的话所有小类都被塞进一个
    //   「书签栏」底下，等于凭空多一级、还得先进它才能看到自己的分类。
    //   直接摊在书签栏根下的书签剥完为空 → 归「未归类」，与它们的实际位置一致。
    const segs = browserPath.split('/').filter(Boolean).slice(1)
    if (!segs.length) return null
    let parentId: number | undefined
    let acc = ''
    for (const seg of segs) {
      acc = acc ? `${acc}/${seg}` : seg
      const existing = store
        .subcategoriesOf(kind)
        .find((s) => s.name === acc)
      if (existing) {
        parentId = existing.id
        continue
      }
      try {
        // ⚠️ 用 `createSubcategoryChild(父 id, 段名)`：全路径由 Rust 那边拼。
        //   在前端自己拼「父路径/名字」的话，路径规则就有两份实现，
        //   一旦对不上就会生成挂在不存在节点下的孤儿小类（树上看不见、也删不掉）。
        const made = await store.addSubcategoryChild(kind, seg, parentId)
        parentId = made.id
      } catch {
        // 建不出来（非法字符等）：这一支的条目就交给用户自己归，
        // 不静默吞掉 —— 少分到一起比悄悄挪到别处好
        return null
      }
    }
    return parentId ?? null
  }

  for (const it of items) {
    try {
      let category: string | null = null
      if (it.category) {
        const id = await ensurePath(it.category)
        const sub = id ? store.subcategoriesOf(kind).find((s) => s.id === id) : undefined
        category = sub?.name ?? null
      }
      await store.addResource({
        kind,
        name: it.name,
        target: it.url,
        category,
      })
      added++
    } catch (err) {
      failed.push(`${it.name}：${String(err)}`)
    }
  }
  bookmarkVisible.value = false
  await store.refreshSubcategories()
  const parts = [`已导入 ${added} 条书签`]
  if (failed.length) parts.push(`${failed.length} 条失败：${failed[0]}`)
  showToast(parts.join(' · '))
}

// ---- 扫描桌面（2026-10-03）----
const desktopVisible = ref(false)

/** 桌面项 → 速达 kind。速达只有 app / web / file 三类：
 *  .app 与快捷方式都算「app」（都是可执行/可双击的东西），文件夹走 file。 */
function sudaKindFor(e: DesktopEntry): 'app' | 'file' {
  return e.kind === 'folder' ? 'file' : 'app'
}

/**
 * 把选中的桌面项加入速达。
 *
 * ⚠️ 逐条处理并**如实回报失败**：某条加不进去不能悄悄跳过 —— 用户不知道
 *   自己以为加了什么。
 *
 * ⚠️ 「清理桌面」在**全部加入之后**才执行，且只对 `removable` 为真的条目。
 *   反过来的话中途失败就两头都没了（约定 72）。即便如此，后端
 *   `remove_desktop_shortcut` 仍会**独立再判一次**是不是快捷方式 ——
 *   界面上的勾不是防线。
 */
async function addDesktopEntries(list: DesktopEntry[], removeShortcuts: boolean) {
  let added = 0
  const failed: string[] = []
  const removable: string[] = []

  for (const e of list) {
    try {
      await store.addResource({ kind: sudaKindFor(e), name: e.name, target: e.path })
      added++
      if (removeShortcuts && e.removable) removable.push(e.path)
    } catch (err) {
      failed.push(`${e.name}：${String(err)}`)
    }
  }

  let cleaned = 0
  const cleanFailed: string[] = []
  for (const path of removable) {
    try {
      await tauriApi.removeDesktopShortcut(path)
      cleaned++
    } catch (err) {
      cleanFailed.push(String(err))
    }
  }

  desktopVisible.value = false
  const parts = [`已加入 ${added} 条`]
  if (cleaned) parts.push(`清理了 ${cleaned} 个桌面快捷方式`)
  if (failed.length) parts.push(`${failed.length} 条失败：${failed[0]}`)
  if (cleanFailed.length) parts.push(`${cleanFailed.length} 个没清掉：${cleanFailed[0]}`)
  showToast(parts.join(' · '))
}

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)
const rootRef = ref<HTMLElement | null>(null)
const hasOverlayModal = computed(() => formVisible.value || menu.value.visible || scanVisible.value)
const { onIconError, showImageIcon, showWebFallbackIcon, iconText, fileIconOf } =
  useResourceIcon()

// ---- 拖拽导入：只预填弹窗，用户确认后才真正添加 ----

/**
 * 拖入的路径看起来是不是一个「程序」。
 *
 * 与 `commands.rs::parse_dropped_path` 的准入判据保持一致，否则会出现
 * 「先弹了遮罩、后端却拒收」的错位（用户看到遮罩闪一下就没了）。
 * - Windows：`.exe` / `.lnk`
 * - macOS：`.app` 包；**无扩展名的可执行文件在 JS 侧判不出来**
 *   （要看可执行位，浏览器拿不到），故只认 `.app`，
 *   由后端在 `parse_dropped_path` 里做最终的「带可执行位」判断。
 */
function looksLikeApp(file: string): boolean {
  const lower = file.toLowerCase()
  return lower.endsWith('.app') || lower.endsWith('.exe') || lower.endsWith('.lnk')
}

const dropping = ref(false)
// 程序解析期间的文件名。解析 .app 要读包内 Info.plist + 查 LaunchServices 图标缓存，
// 118 个应用实测约 1.5s，期间必须有遮罩，否则用户以为「拖了没反应」
const parsing = ref<string | null>(null)
const prefill = ref<{
  name?: string
  target?: string
  icon?: string | null
  kind?: 'app' | 'web' | 'file'
  category?: string | null
  isDir?: boolean
} | null>(null)
let unlistenDrop: (() => void) | null = null

onMounted(async () => {
  void installedBrowsers() // 预热浏览器列表，右键菜单即开即用
  if (!isTauri()) return
  const webview = getCurrentWebview()
  unlistenDrop = await webview.onDragDropEvent((event) => {
    const ev = event.payload
    if (hasOverlayModal.value || parsing.value) return
    if (ev.type === 'enter' || ev.type === 'over') {
      if (ev.type === 'over') return
      if (!ev.paths.length) return
      // 整个窗口均可拖拽导入：enter 即亮起全屏遮罩，
      // 释放时不再校验位置（遮罩提示居中，用户常移到提示处释放，二次校验会误丢 drop）
      dropping.value = true
    } else if (ev.type === 'leave') {
      dropping.value = false
    } else if (ev.type === 'drop') {
      dropping.value = false
      const file = ev.paths?.[0]
      if (file) {
        // 程序解析耗时较长：立刻切到“正在识别”遮罩，避免无反馈空白期
        if (looksLikeApp(file)) {
          parsing.value = file.split(/[\\/]/).pop() ?? file
        }
        void handleDrop(file)
      }
    }
  })
})

onBeforeUnmount(() => {
  unlistenDrop?.()
})

// ---- 拖拽去重：目标路径与现有资源一致即视为重复（统一分隔符/大小写后比较） ----
function normalizeTarget(p: string): string {
  return p.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase()
}
function findDuplicateTarget(target: string): Resource | undefined {
  const key = normalizeTarget(target)
  return store.state.resources.find((r) => r.target && normalizeTarget(r.target) === key)
}

async function handleDrop(file: string) {
  // 命中已有资源的路径直接提示跳过：exe/lnk 还可省去 PowerShell 解析
  const direct = findDuplicateTarget(file)
  if (direct) {
    showToast(`「${direct.name}」已在速达中，已跳过重复添加`)
    return
  }
  if (looksLikeApp(file)) {
    parsing.value ||= file.split(/[\\/]/).pop() ?? file
    try {
      const info = await tauriApi.parseDroppedPath(file)
      // 解析出的真实目标可能已用别的方式添加过
      // （Windows：另一个指向同一程序的 .lnk；macOS：另一个软链/替身）
      const dup = findDuplicateTarget(info.target)
      if (dup) {
        showToast(`「${dup.name}」已在速达中，已跳过重复添加`)
        return
      }
      prefill.value = { name: info.name, target: info.target, icon: info.icon, kind: 'app' }
      editing.value = null
      formVisible.value = true
      showToast(`已识别「${info.name}」，请点击添加确认`)
    } catch (e) {
      showToast(String(e))
    } finally {
      parsing.value = null
    }
    return
  }
  try {
    const info = await tauriApi.inspectPath(file)
    const category = categorize(file, info.is_dir)
    prefill.value = { name: info.name, target: file, kind: 'file', category, isDir: info.is_dir }
    editing.value = null
    formVisible.value = true
    showToast(`已识别「${info.name}」，请点击添加确认`)
  } catch (e) {
    void reportClientError('速达拖拽解析失败', e)
    showToast(String(e))
  }
}

// ---- 运行状态检测：轮询进程名集合，应用已启动时名称左侧显示小绿点 ----
// 进程枚举是重量级操作（sysinfo 全量快照，单次几十毫秒）：可见且聚焦 5s、
// 失焦/滚出视口 15s 慢速档、隐藏完全停（收托盘后无意义且空烧发热），
// 从停止恢复时立即补采一轮。门控统一走 useAdaptivePolling。
const runningNames = ref<Set<string>>(new Set())
const RUNNING_ACTIVE_MS = 5000
const RUNNING_IDLE_MS = 15000

// 匹配「这个速达 app 是不是在跑」。
// 后端回传的是**一批可能对上的键**（进程名 + 每个运行中应用的包路径/包名/本地化名），
// 这里只做集合查表，不做字符串猜测 —— macOS 的 target 是 `.app` 包路径、
// 进程名却可能是 `WeChat` 甚至 `Code Helper`，猜是猜不全的（见 get_running_processes）。
function isRunning(r: Resource): boolean {
  if (r.kind !== 'app' || !r.target) return false
  const set = runningNames.value
  if (!set.size) return false
  const full = r.target.toLowerCase()
  // ① 完整包路径：macOS 上的精确匹配（`/applications/wechat.app`）
  if (set.has(full)) return true
  // ② 末段：Windows 的 `xxx.exe` 口径（进程名即文件名）
  const base = full.split(/[\\/]/).pop() ?? ''
  if (base && set.has(base)) return true
  // ③ 末段去掉 `.app`：兼容只回包名/本地化名的旧数据
  if (base.endsWith('.app')) {
    const stem = base.slice(0, -4)
    if (stem && set.has(stem)) return true
  }
  return false
}

async function refreshRunning() {
  if (!isTauri()) return
  try {
    const names = await tauriApi.getRunningProcesses()
    runningNames.value = new Set(names)
  } catch {
    // 静默失败，下一轮重试
  }
}

useAdaptivePolling(refreshRunning, {
  activeMs: RUNNING_ACTIVE_MS,
  idleMs: RUNNING_IDLE_MS,
  viewport: rootRef,
})

// ---- 分类筛选 ----
type FilterKey = '全部' | '常用' | '应用' | '网页' | '文件'
type SubFilter = 'all' | 'none' | string

const activeFilter = ref<FilterKey>('全部')
/** 大类内的小类筛选：all=全部，none=未归类，其余为小类名（ADR 0012） */
const activeSub = ref<SubFilter>('all')

// 小类筛选行只在大类视图出现；应用/网页/文件各有自己的小类库（允许同名不同义）
const SUB_KIND: Partial<Record<FilterKey, 'app' | 'web' | 'file'>> = {
  应用: 'app',
  网页: 'web',
  文件: 'file',
}

const subKind = computed(() => SUB_KIND[activeFilter.value])

/** 该大类的小类树（层级，2026-10-04） */
const subTabs = computed(() => (subKind.value ? store.subcategoryTree(subKind.value) : []))

/** 展开了哪些层级（存全路径，切大类时不必清 —— 路径自带大类语义之外的唯一性） */
const expandedSubs = ref<Set<string>>(new Set())

/** 树摊平成一行 chip：带层级、标出有子级且展开的父节点 */
interface SubChip {
  id: number
  fullPath: string
  label: string
  depth: number
  isDefault: boolean
  /** 有子级且当前收起 → 显示一个可点的展开箭头 */
  collapsible: boolean
  expanded: boolean
}

const subChips = computed<SubChip[]>(() => {
  const out: SubChip[] = []
  const walk = (nodes: ReturnType<typeof store.subcategoryTree>, depth: number) => {
    for (const n of nodes) {
      const hasKids = n.children.length > 0
      out.push({
        id: n.id,
        fullPath: n.fullPath,
        label: n.name,
        depth,
        isDefault: n.isDefault,
        collapsible: hasKids,
        expanded: hasKids && expandedSubs.value.has(n.fullPath),
      })
      if (hasKids && expandedSubs.value.has(n.fullPath)) walk(n.children, depth + 1)
    }
  }
  walk(subTabs.value, 0)
  return out
})

function toggleExpand(path: string) {
  const next = new Set(expandedSubs.value)
  if (next.has(path)) next.delete(path)
  else next.add(path)
  expandedSubs.value = next
}

// ---- 横向滚动：两端箭头 + 滚轮（发布说明：小类太多时一行的两端会出现箭头按钮，也能用滚轮左右滚动）----
const subBar = ref<HTMLElement | null>(null)
/** 两端箭头只在**真的溢出**时出现 —— 否则行内凭空两个按不动的按钮 */
const canScrollLeft = ref(false)
const canScrollRight = ref(false)

function measureScroll() {
  const el = subBar.value
  if (!el) {
    canScrollLeft.value = canScrollRight.value = false
    return
  }
  canScrollLeft.value = el.scrollLeft > 1
  canScrollRight.value = el.scrollLeft + el.clientWidth < el.scrollWidth - 1
}

function scrollSubs(dir: 1 | -1) {
  const el = subBar.value
  if (!el) return
  el.scrollBy({ left: dir * Math.max(120, el.clientWidth * 0.6), behavior: 'smooth' })
}

/**
 * 滚轮横向滚动。
 *
 * ⚠️ 只在**纵向溢出为零**（也就是这一行本身不需要上下滚）时才拦竖向滚轮：
 *   小类多到需要换行时，用户的竖向滚动是「往下看内容」的正常意图，
 *   抢走它会让人以为页面卡住。
 */
function onSubBarWheel(e: WheelEvent) {
  const el = subBar.value
  if (!el) return
  measureScroll()
  if (!canScrollRight.value && !canScrollLeft.value) return
  const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY
  if (delta === 0) return
  const max = el.scrollWidth - el.clientWidth
  const atLeft = el.scrollLeft <= 0
  const atRight = el.scrollLeft >= max - 1
  // 已经滚到头还继续往回滚 → 交还给外层（页面滚动），别把用户困在这一行里
  if ((delta < 0 && atLeft) || (delta > 0 && atRight)) return
  el.scrollLeft += delta
  e.preventDefault()
  measureScroll()
}

/** 滚到选中项：层级展开后选中的那一项可能在很右边 */
function scrollActiveIntoView() {
  nextTick(() => {
    measureScroll()
    const el = subBar.value
    if (!el) return
    const node = el.querySelector<HTMLElement>('.is-active')
    if (node) node.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  })
}

onMounted(() => {
  measureScroll()
  window.addEventListener('resize', measureScroll)
})
onBeforeUnmount(() => window.removeEventListener('resize', measureScroll))
watch(() => subChips.value.length, () => nextTick(measureScroll))
watch(activeSub, scrollActiveIntoView)

// 切大类时重置小类筛选：同名不同义，跨大类沿用旧名会筛出错误集合
watch(activeFilter, () => {
  activeSub.value = 'all'
})

/**
 * 小类筛选：选中某一层 → 展示**它下面所有**网页（层级，2026-10-04）。
 *
 * ⚠️ 这里从 `===` 变成「含后代」（`store.subcategoryContains`）：
 *   资源挂在「开发/前端」时，点「开发」要能看见它 —— 这正是发布说明那句
 *   「选中某一层就展示它下面所有网页」。按段比较，筛「开发」不会带出「开发者」。
 */
function matchSub(r: Resource): boolean {
  if (activeSub.value === 'all') return true
  if (activeSub.value === 'none') return r.category == null
  return store.subcategoryContains(r.category, activeSub.value)
}

const visibleResources = computed<Resource[]>(() => {
  const all = store.state.resources
  if (activeFilter.value === '全部') return [...all]
  if (activeFilter.value === '常用') {
    return all
      .filter((r) => r.last_launched_at)
      .slice()
      .sort(
        (a, b) =>
          new Date(b.last_launched_at!).getTime() - new Date(a.last_launched_at!).getTime(),
      )
  }
  const kind = SUB_KIND[activeFilter.value]
  if (kind) return all.filter((r) => r.kind === kind && matchSub(r))
  return []
})

const FILTER_TABS: FilterKey[] = ['全部', '常用', '应用', '网页', '文件']

const emptyTitle = computed(() => {
  if (activeFilter.value === '全部') return '还没有速达资源'
  if (activeSub.value === 'none') return `暂无未归类的${activeFilter.value}`
  if (typeof activeSub.value === 'string' && activeSub.value !== 'all') {
    return `暂无「${activeSub.value}」小类资源`
  }
  return `暂无「${activeFilter.value}」资源`
})

// ---- 长按拖拽排序（#6）：「常用」按最近使用排序，不开放手动排序 ----
const gridRef = ref<HTMLElement | null>(null)
const { draggingId, dragOffset, dragOrigin, dropBeforeId, dropAtEnd, onCardPointerDown, swallowClick } =
  useSudaDrag({
    gridRef,
    items: visibleResources,
    enabled: () => activeFilter.value !== '常用',
    reorder: (ids) => void onReorderVisible(ids),
  })

/** 可见项新顺序 → 全表顺序：可见项占住它在全表里的原有槽位，其余项不动 */
function onReorderVisible(visibleIds: number[]) {
  const all = store.state.resources
  const pos = new Set(visibleIds)
  const slots: number[] = []
  all.forEach((r, i) => {
    if (pos.has(r.id)) slots.push(i)
  })
  const result = all.map((r) => r.id)
  visibleIds.forEach((id, k) => {
    const slot = slots[k]
    if (slot != null) result[slot] = id
  })
  void store.reorderResources(result)
}

/** 被拖卡片跟手飞行的位移；非拖拽态不给 inline 样式，让位给 hover 位移。
 *  卡片拖拽时是 absolute（脱离流，见 .suda-card.is-dragging），所以要先平移到原位再叠加位移。
 *  这里刻意用独立的 `translate` 属性而不是 `transform`：位移必须即时跟手（不能进过渡列表），
 *  而「浮起」的放大交给独立 `scale` 属性做短过渡 —— 两者分开，才能一个即时、一个柔和。 */
function dragStyleOf(r: Resource) {
  if (draggingId.value !== r.id) return {}
  const x = dragOrigin.value.x + dragOffset.value.x
  const y = dragOrigin.value.y + dragOffset.value.y
  return { translate: `${x}px ${y}px` }
}

function onCardClick(r: Resource) {
  if (swallowClick()) return
  void onOpen(r)
}

// ---- 右键菜单 ----
const menu = ref({ visible: false, x: 0, y: 0, items: [] as ContextMenuItem[] })

function openMenu(e: MouseEvent, items: ContextMenuItem[]) {
  // 必须延迟到当前事件派发结束后再置位：ContextMenu 在 window 上监听 contextmenu/click
  // 用于点击别处关闭菜单，若在同一事件派发内同步置位，紧跟的全局关闭监听会在
  // props 更新后立即把菜单关掉（表现为右键无反应）；已开时右键另一资源也无法重定位
  setTimeout(() => {
    menu.value = { visible: true, x: e.clientX, y: e.clientY, items }
  }, 0)
}

async function onDeleteResource(r: Resource) {
  await store.removeResource(r.id)
  showToast(`已删除「${r.name}」`, {
    label: '撤销',
    onClick: async () => {
      await store.addResource({
        kind: r.kind,
        name: r.name,
        target: r.target,
        category: r.category,
        icon: r.icon,
        args: r.args,
      })
      showToast('已恢复')
    },
  })
}

// ---- 指定浏览器打开（网页资源）：列表来自本机已安装浏览器（Rust 注册表枚举） ----
let browserCache: InstalledBrowser[] | null = null

async function installedBrowsers(): Promise<InstalledBrowser[]> {
  if (browserCache === null) {
    try {
      browserCache = isTauri() ? await tauriApi.listInstalledBrowsers() : []
    } catch {
      browserCache = []
    }
  }
  return browserCache
}

async function onOpenWithBrowser(r: Resource, b: InstalledBrowser) {
  try {
    await store.openResourceInBrowser(r.id, b.exe)
  } catch (e) {
    showToast(`无法用「${b.name}」打开：${String(e)}`)
  }
}

async function onOpenInWindow(r: Resource) {
  try {
    await store.openResourceInWindow(r.id)
  } catch (e) {
    showToast(String(e))
  }
}

/** 以管理员身份运行（UAC 确认）：仅「程序」资源有提权语义 */
async function onOpenAsAdmin(r: Resource) {
  try {
    await store.launchResourceAsAdmin(r.id)
  } catch (e) {
    showToast(`${ADMIN_LAUNCH_TERM.replace(/（.*/, '')}「${r.name}」失败：${String(e)}`)
  }
}

async function onResourceContext(e: MouseEvent, r: Resource) {
  e.preventDefault()
  const items: ContextMenuItem[] = [{ label: '打开', onClick: () => onOpen(r) }]
  const isApp = r.kind === 'app'
  if (isApp) {
    items.push({ label: ADMIN_LAUNCH_TERM, onClick: () => void onOpenAsAdmin(r) })
  }
  let isWeb = false
  if (r.kind === 'web') {
    isWeb = true
    // 显式覆盖默认打开方式（默认方式见 设置 → 功能 → 速达）
    items.push({ label: '在内嵌面板打开', dividerBefore: true, onClick: () => store.openWebPanel(r.id) })
    items.push({ label: '在独立窗口打开', onClick: () => void onOpenInWindow(r) })
    const browsers = await installedBrowsers()
    for (const b of browsers) {
      items.push({ label: `用 ${b.name} 打开`, onClick: () => void onOpenWithBrowser(r, b) })
    }
  }
  items.push({
    label: '编辑',
    dividerBefore: isWeb,
    onClick: () => {
      editing.value = r
      formVisible.value = true
    },
  })
  items.push({
    label: '删除',
    danger: true,
    onClick: () => void onDeleteResource(r),
  })
  openMenu(e, items)
}

// ---- 弹窗 ----
const formVisible = ref(false)
const editing = ref<Resource | null>(null)

async function onOpen(r: Resource) {
  try {
    await store.launchResource(r.id)
  } catch (e) {
    showToast(`无法打开「${r.name}」：${String(e)}`)
  }
}

function onFormSubmit(payload: {
  id?: number
  kind: 'app' | 'web' | 'file'
  name: string
  target: string
  category?: string | null
  icon?: string | null
  args?: string | null
}) {
  if (payload.id != null) {
    void store.editResource({ ...payload, id: payload.id })
    showToast(`已更新「${payload.name}」`)
  } else {
    void store.addResource(payload)
    showToast(`已添加「${payload.name}」`)
  }
  prefill.value = null
}

// ---- 扫描已安装应用导入 ----
const scanVisible = ref(false)
const importing = ref(false)

async function onScanImported(apps: InstalledAppInfo[]) {
  if (importing.value) return
  importing.value = true
  let added = 0
  let skipped = 0
  for (const a of apps) {
    // 二次去重保护：目标路径已存在则跳过（弹窗中已禁用，这里兜底）
    if (
      store.state.resources.some(
        (r) => r.kind === 'app' && r.target.toLowerCase() === a.target.toLowerCase(),
      )
    ) {
      skipped++
      continue
    }
    try {
      await store.addResource({
        kind: 'app',
        name: a.name,
        target: a.target,
        category: null,
        icon: a.icon,
        args: null,
      })
      added++
    } catch (e) {
      void reportClientError('速达扫描导入失败', e)
    }
  }
  importing.value = false
  showToast(skipped > 0 ? `已添加 ${added} 个应用，跳过 ${skipped} 个已存在` : `已添加 ${added} 个应用`)
}

// ---- 图标渲染（统一在 useResourceIcon composable） ----

function kindLabel(r: Resource): string {
  // 有小类显示小类名（应用/网页/文件统一），否则回退大类名
  return r.category ?? (r.kind === 'app' ? '应用' : r.kind === 'web' ? '网页' : '文件')
}

function cardAccentStyle(r: Resource) {
  if (r.kind === 'file') {
    const a = fileAccentOf(r.category ?? '其他')
    return {
      '--suda-accent-soft': a.soft,
      '--suda-accent': a.strong,
      '--suda-accent-ink': a.ink,
    }
  }
  const a = accentOf(r.name)
  return {
    '--suda-accent-soft': a.soft,
    '--suda-accent': a.strong,
    '--suda-accent-ink': a.text,
  }
}
</script>

<template>
  <section ref="rootRef" class="card suda">
    <header class="suda-header">
      <h2 class="suda-title">速达</h2>
      <div class="suda-header-actions">
        <button
          v-if="isTauri()"
          class="icon-btn scan"
          title="从浏览器导入书签"
          aria-label="从浏览器导入书签"
          @click="bookmarkVisible = true"
        >
          <Bookmark :size="15" :stroke-width="2.2" />
        </button>
        <button
          class="icon-btn batch"
          :class="{ on: selectMode }"
          :title="selectMode ? '退出批量管理' : '批量管理（勾选一批删除）'"
          aria-label="批量管理"
          @click="selectMode ? exitSelectMode() : (selectMode = true)"
        >
          <CheckCheck :size="15" :stroke-width="2.2" />
        </button>
        <button
          v-if="isTauri()"
          class="icon-btn scan"
          title="扫描桌面"
          aria-label="扫描桌面"
          @click="desktopVisible = true"
        >
          <Monitor :size="15" :stroke-width="2.2" />
        </button>
        <button
          v-if="isTauri()"
          class="icon-btn scan"
          title="扫描已安装应用"
          aria-label="扫描已安装应用"
          @click="scanVisible = true"
        >
          <ScanSearch :size="15" :stroke-width="2.2" />
        </button>
        <button
          class="icon-btn add"
          title="添加"
          @click="startAdd()"
        >
          <Plus :size="15" :stroke-width="2.2" />
        </button>
      </div>
    </header>

    <!-- 批量管理操作条：只在该模式下出现，且明写「删的是记录不是文件」 -->
    <div v-if="selectMode" class="suda-batch-bar">
      <button class="ghost-btn" type="button" @click="toggleSelectAllVisible">
        {{ allVisibleSelected ? '取消全选' : `全选当前 ${visibleResources.length} 条` }}
      </button>
      <span class="suda-batch-count">已选 {{ selected.size }} 条</span>
      <button
        class="ghost-btn danger"
        type="button"
        :disabled="!selected.size || batchBusy"
        @click="batchRemove"
      >
        {{ batchBusy ? '删除中…' : '删除所选' }}
      </button>
      <button class="ghost-btn" type="button" @click="exitSelectMode">退出</button>
    </div>

    <!-- 分类 tabs -->
    <nav class="filter-tabs suda-tabs" aria-label="速达分类">
      <button
        v-for="f in FILTER_TABS"
        :key="f"
        class="filter-tab filter-tab--primary"
        :class="{ active: activeFilter === f }"
        @click="activeFilter = f"
      >
        {{ f }}
      </button>
    </nav>

<!-- 大类小类筛选（ADR 0012）：应用/网页/文件各有小类库；未归类=category 为空 -->
<!--
  小类筛选（层级，2026-10-04）。

  ⚠️ 结构上把「滚动容器」和「两端箭头」**拆成三个兄弟**：箭头在容器外，
     才能在溢出时压在两端而不被内容顶走；容器自己负责 overflow-x 与滚轮。

  ⚠️ 容器上的 `min-width: 0` 不能省（见样式段）：它是 flex/grid 子项默认
     `min-width:auto` 的唯一解法 —— 少了它这一行永远「不溢出」，横向滚动
     与两端箭头就都不会出现，症状是「小类多了以后全挤在一行还滚不动」。
-->
<div v-if="subKind" class="suda-sub-wrap">
  <button
    v-if="canScrollLeft"
    class="suda-sub-arrow is-left"
    type="button"
    aria-label="小类列表向左滚动"
    @click="scrollSubs(-1)"
  >
    <ChevronLeft :size="14" :stroke-width="2.2" aria-hidden="true" />
  </button>

  <nav ref="subBar" class="filter-tabs suda-cat-tabs" aria-label="小类筛选" @wheel="onSubBarWheel">
    <button
      class="filter-tab filter-tab--tag"
      :class="{ active: activeSub === 'all' }"
      @click="activeSub = 'all'"
    >
      全部{{ activeFilter }}
    </button>
    <button
      class="filter-tab filter-tab--tag"
      :class="{ active: activeSub === 'none' }"
      @click="activeSub = 'none'"
    >
      未归类
    </button>
    <!--
      层级小类：有子级时名字左边多一个展开/收起箭头（点它只折叠、不改筛选），
      chip 本身按 depth 缩进，点它则筛出**整棵子树**。
    -->
    <span
      v-for="c in subChips"
      :key="c.id"
      class="sub-chip"
      :style="{ '--sub-depth': c.depth }"
    >
      <button
        v-if="c.collapsible"
        class="sub-chip-twisty"
        type="button"
        :aria-label="(c.expanded ? '收起 ' : '展开 ') + c.label"
        :aria-expanded="c.expanded"
        @click.stop="toggleExpand(c.fullPath)"
      >
        <ChevronRight :size="11" :stroke-width="2.4" :class="{ 'is-open': c.expanded }" aria-hidden="true" />
      </button>
      <span v-else class="sub-chip-twisty is-spacer" aria-hidden="true" />
      <button
        class="filter-tab filter-tab--tag"
        :class="{ 'is-active': activeSub === c.fullPath }"
        :title="c.fullPath === c.label ? c.label : `「${c.label}」及其下级`"
        @click="activeSub = c.fullPath"
      >
        <Star
          v-if="c.isDefault"
          class="sub-default-star"
          :size="10"
          :stroke-width="2.4"
          title="默认小类：新增资源未指定小类时自动归入；删除小类时条目也改挂到这里（在 设置 → 功能 → 小类管理 更换）"
          aria-hidden="true"
        />
        {{ c.label }}
      </button>
    </span>
  </nav>

  <button
    v-if="canScrollRight"
    class="suda-sub-arrow is-right"
    type="button"
    aria-label="小类列表向右滚动"
    @click="scrollSubs(1)"
  >
    <ChevronRight :size="14" :stroke-width="2.2" aria-hidden="true" />
  </button>
</div>

    <!-- 资源网格（5 列） -->
    <div class="suda-body">
      <div v-if="visibleResources.length > 0" ref="gridRef" class="suda-grid">
        <template v-for="r in visibleResources" :key="r.id">
          <div v-if="dropBeforeId === r.id" class="suda-drop-slot" aria-hidden="true" />
        <div
          class="suda-card"
          :data-id="r.id"
          :title="r.target"
          role="button"
          tabindex="0"
          :class="{ 'is-dragging': draggingId === r.id, 'is-selecting': selectMode, 'is-checked': selected.has(r.id) }"
          :style="[cardAccentStyle(r), dragStyleOf(r)]"
          @click="selectMode ? toggleSelect(r.id) : onCardClick(r)"
          @pointerdown="onCardPointerDown(r, $event)"
          @dragstart.prevent
          @keydown.enter="onOpen(r)"
          @keydown.space.prevent="onOpen(r)"
          @contextmenu="onResourceContext($event, r)"
        >
          <span v-if="selectMode" class="suda-check" aria-hidden="true">
            <Check v-if="selected.has(r.id)" :size="12" :stroke-width="3" />
          </span>
          <span class="suda-kind" :class="r.kind">{{ kindLabel(r) }}</span>
          <div class="suda-actions">
            <button
              class="suda-action"
              title="编辑"
              aria-label="编辑"
              @click.stop="editing = r; formVisible = true"
            >
              <Pencil :size="11" :stroke-width="2" />
            </button>
            <button
              class="suda-action del"
              title="删除"
              aria-label="删除"
              @click.stop="onDeleteResource(r)"
            >
              <Trash2 :size="11" :stroke-width="2" />
            </button>
          </div>
            <div
              class="suda-icon"
              :class="{ 'web-default': showWebFallbackIcon(r) }"
              :style="
                showImageIcon(r)
                  ? {}
                  : { background: 'var(--suda-accent-soft)' }
            "
          >
            <img
              v-if="showImageIcon(r)"
              class="suda-img"
              :src="iconSrc(r.icon!)"
              alt=""
              draggable="false"
              @error="onIconError(r)"
            />
            <Globe
              v-else-if="showWebFallbackIcon(r)"
              class="suda-file-icon"
              :size="25"
              :stroke-width="1.7"
              :style="{ color: 'var(--c-green-ink)' }"
            />
            <component
              v-else-if="r.kind === 'file'"
              :is="fileIconOf(r)"
              class="suda-file-icon"
              :size="25"
              :stroke-width="1.7"
              :style="{ color: 'var(--suda-accent)' }"
            />
            <span
              v-else
              class="suda-letter"
              :style="{ color: 'var(--suda-accent-ink)' }"
            >
              {{ iconText(r) }}
            </span>
          </div>
          <span class="suda-name">
            <span v-if="isRunning(r)" class="suda-dot" title="运行中" />
                <span class="suda-name-text" :title="r.name">{{ r.name }}</span>
          </span>
        </div>
        </template>
        <div v-if="dropAtEnd" class="suda-drop-slot" aria-hidden="true" />
      </div>

      <div v-else class="empty-state">
        <Wrench :size="24" :stroke-width="1.7" aria-hidden="true" />
        <p>{{ emptyTitle }}</p>
        <p style="font-size: 0.75rem; color: var(--text-4)">
          拖拽本地文件/程序到窗口，或手动添加快捷链接
        </p>
        <button
          class="pill-btn"
          style="margin-top: 6px"
          @click="editing = null; prefill = null; formVisible = true"
        >
          添加
        </button>
      </div>
    </div>

    <ContextMenu
      :visible="menu.visible"
      :x="menu.x"
      :y="menu.y"
      :items="menu.items"
      @close="menu.visible = false"
    />
    <SudaFormDialog
      :visible="formVisible"
      :editing="editing"
      :prefill="prefill"
      @close="formVisible = false"
      @submit="onFormSubmit"
    />
    <SudaBookmarkDialog
      :visible="bookmarkVisible"
      @close="bookmarkVisible = false"
      @picked="addBookmarks"
    />
    <SudaDesktopDialog
      :visible="desktopVisible"
      @close="desktopVisible = false"
      @picked="addDesktopEntries"
    />
    <SudaScanDialog
      :visible="scanVisible"
      @close="scanVisible = false"
      @imported="onScanImported"
    />

    <!-- 拖拽导入遮罩（dropping = 拖拽中；parsing = 正在识别程序） -->
    <Teleport to="body">
      <Transition name="drop">
        <div v-if="dropping || parsing" class="drop-overlay">
          <div class="drop-hint">
            <Loader2 v-if="parsing" :size="34" :stroke-width="1.5" class="spin" />
            <FilePlus v-else :size="34" :stroke-width="1.5" />
            <p v-if="parsing">正在识别…</p>
            <p v-else>释放以添加</p>
            <span v-if="parsing" :title="parsing">{{ parsing }}</span>
            <span v-else>支持本地程序 / 网页 / 任意文件或文件夹</span>
          </div>
        </div>
      </Transition>
    </Teleport>
  </section>
</template>

<style scoped>
.suda {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px;
  min-height: 0;
}
.suda-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}
.suda-title {
  font-size: 1rem;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
}
.suda-header-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
.icon-btn.add {
  width: 30px;
  height: 30px;
  background: var(--brand-50);
  color: var(--brand-500);
}
.icon-btn.add:hover {
  background: var(--brand-500);
  color: var(--text-on-accent);
}
.icon-btn.scan {
  width: 30px;
  height: 30px;
  background: var(--bg-card-soft);
  color: var(--text-3);
}
.icon-btn.scan:hover {
  background: var(--brand-500);
  color: var(--text-on-accent);
}

.suda-tabs {
  margin-bottom: 10px;
}
.suda-cat-tabs {
  margin-bottom: 14px;
  padding-bottom: 4px;
  border-bottom: 1px solid var(--border-soft);
}
/* 默认小类星标（含义见 tooltip，设置里可改默认）：置于小类名前；Tailwind preflight 把 svg 置为 block，必须恢复行内否则掉到文字下一行 */
.sub-default-star {
  display: inline-block;
  vertical-align: -1px;
  margin-right: 3px;
  color: var(--c-yellow);
}

.suda-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
.suda-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, 124px);
  justify-content: space-between;
  gap: 10px;
  /* 被拖卡片拖拽期间 position:absolute，这里当它的定位上下文 */
  position: relative;
}
.suda-card {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 7px;
  width: 124px;
  min-width: 0;
  padding: 15px 8px 12px;
  /* 比外层玻璃面板更实的底 + 描边 + 投影，与面板拉开层次 */
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  box-shadow: var(--shadow-card);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: transform 0.18s, box-shadow 0.18s;
}
.suda-card:hover .suda-kind,
.suda-card:focus-within .suda-kind {
  color: var(--text-1);
}
.suda-card:hover {
  transform: translateY(-2px);
  box-shadow: var(--shadow-hover);
}
.suda-icon {
  width: 46px;
  height: 46px;
  border-radius: 14px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.18s ease-out, background 0.18s ease-out;
}
.suda-card:hover .suda-icon {
  transform: scale(1.06);
}
.suda-file-icon {
  background: transparent;
}
.suda-icon.web-default {
  background: var(--c-green-soft);
}
.suda-letter {
  font-size: 1.25rem;
  font-weight: 700;
}
.suda-img {
  width: 46px;
  height: 46px;
  border-radius: 14px;
  object-fit: contain;
}
.suda-name {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  max-width: 100%;
  font-size: 0.75rem;
  font-weight: 500;
  color: var(--text-2);
}
.suda-name-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.suda-dot {
  flex-shrink: 0;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--c-green);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--c-green) 22%, transparent);
}
.suda-kind {
  position: absolute;
  top: 6px;
  left: 6px;
  display: inline-flex;
  align-items: center;
  min-height: 0;
  padding: 0;
  font-size: 0.625rem;
  font-weight: 600;
  line-height: 1;
  color: var(--text-3);
}
.suda-kind.app {
  color: var(--c-blue-ink);
}
.suda-kind.web {
  color: var(--c-green-ink);
}
.suda-kind.file {
  color: var(--c-purple-ink);
}

.suda-actions {
  position: absolute;
  top: 5px;
  right: 5px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  opacity: 0;
  transition: opacity 0.15s;
}
.suda-card:hover .suda-actions,
.suda-card:focus-within .suda-actions {
  opacity: 1;
}
.suda-action {
  width: 28px;
  height: 28px;
  border: none;
  background: var(--bg-card);
  border-radius: 7px;
  box-shadow: var(--shadow-card);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}
.suda-action:hover {
  color: var(--brand-500);
  background: var(--brand-50);
}
.suda-action.del:hover {
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
}

/* 拖拽导入遮罩 */
.drop-overlay {
  position: fixed;
  /* 拖拽遮罩同理：拖拽时整窗变方形是最扎眼的一次（见 check-rounded-window.mjs） */
  border-radius: var(--window-radius);
  inset: 0px;
  z-index: 250;
  background: color-mix(in srgb, var(--brand-500) 10%, transparent);
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}
.drop-hint {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 32px 48px;
  background: var(--bg-card);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-dock);
  border: 2px dashed var(--brand-500);
  color: var(--brand-500);
}
.drop-hint p {
  font-size: 0.9375rem;
  font-weight: 600;
}
.drop-hint span {
  font-size: 0.75rem;
  color: var(--text-3);
  max-width: 420px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.spin {
  animation: drop-spin 0.9s linear infinite;
}
@keyframes drop-spin {
  to {
    transform: rotate(360deg);
  }
}

.drop-enter-active,
.drop-leave-active {
  transition: opacity 0.15s ease-out;
}
.drop-enter-from,
.drop-leave-to {
  opacity: 0;
}

/* 长按拖拽排序（#6）：跟手飞行 + 落点虚线插槽 */
.suda-card.is-dragging {
  /* 脱离文档流：腾出来的格子由 .suda-drop-slot 补上，网格项数恒为 N。
     否则插槽会多占一格，把后面的卡片整片挤到下一行（拖拽时网格整体跳动）。 */
  position: absolute;
  top: 0;
  left: 0;
  /* 位移走独立 translate 属性（inline），不进过渡列表 —— 跟手必须即时。
     只有「浮起」的放大与阴影走短过渡：否则卡片会在指针处瞬间变大，
     看起来像凭空冒出来（用户反馈的「从右下角飘出来」）。 */
  transition: scale 0.12s ease-out, box-shadow 0.12s ease-out;
  scale: 1.04;
  z-index: 60;
  cursor: grabbing;
  box-shadow: var(--shadow-dock);
}
.suda-card.is-dragging:hover {
  /* 拖拽中不再叠加 hover 的上浮位移，位置完全由指针决定 */
  transform: none;
}
.suda-drop-slot {
  width: 124px;
  /* 跟随同行卡片的高度（不写死，避免比卡片高把整行撑起来）；
     单独占一行时仍保留一个可见的虚线框 */
  align-self: stretch;
  min-height: 88px;
  border: 2px dashed var(--brand-500);
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--brand-500) 6%, transparent);
}
:global(body.suda-dragging) {
  cursor: grabbing;
  user-select: none;
  -webkit-user-select: none;
}

/* ---------------- 批量管理（2026-10-03） ---------------- */

/* 多选态下卡片不再「点开」，只勾选：光标与 hover 都要说清楚当前是哪种模式，
   否则用户会以为点不动了（点一下什么都没发生是最容易被当成 bug 的交互）。 */
.suda-card.is-selecting {
  cursor: default;
}
.suda-card.is-selecting:hover {
  transform: none;
  box-shadow: none;
}
.suda-card.is-checked {
  outline: 2px solid var(--brand-500);
  outline-offset: -2px;
}

.suda-check {
  position: absolute;
  top: 7px;
  right: 7px;
  width: 16px;
  height: 16px;
  border-radius: 5px;
  border: 1.5px solid var(--border-soft);
  background: var(--bg-card-solid);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--brand-600);
}
.suda-card.is-checked .suda-check {
  border-color: var(--brand-500);
  background: var(--brand-500);
  color: #fff;
}

.suda-batch-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  margin-bottom: 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  background: var(--bg-card-soft);
}
.suda-batch-count {
  font-size: 12px;
  color: var(--text-2);
  margin-right: auto;
}
.suda-batch-bar .ghost-btn.danger {
  color: var(--c-red);
  border-color: var(--c-red);
}
.suda-batch-bar .ghost-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.icon-btn.batch.on {
  color: var(--brand-600);
  background: var(--brand-50);
}

/* ---- 小类筛选：层级 chip + 横向滚动（2026-10-04） ---- */
.suda-sub-wrap {
  display: flex;
  align-items: center;
  gap: 2px;
  /* 关键：默认 min-width:auto 的 flex 子项永远不「溢出」，滚动与箭头就都不会出现 */
  min-width: 0;
}
.suda-cat-tabs {
  flex: 1;
  min-width: 0;
  overflow-x: auto;
  overflow-y: hidden;
  scrollbar-width: none;
  /* 层级 chip 之间的横向 padding 由 flex gap 承担，这里去掉旧的横向内边距 */
  padding-bottom: 2px;
}
.suda-cat-tabs::-webkit-scrollbar {
  display: none;
}
.suda-sub-arrow {
  flex-shrink: 0;
  display: grid;
  place-items: center;
  width: 20px;
  height: 20px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm, 6px);
  background: var(--bg-card-soft);
  color: var(--text-2);
  cursor: pointer;
}
.suda-sub-arrow:hover {
  color: var(--text-1);
  background: var(--bg-card);
}
.sub-chip {
  display: inline-flex;
  align-items: center;
  /* 层级缩进：每层 12px。顶到第三层也就是 24px，再多就该换「面包屑」而不是缩进了 */
  padding-left: calc(var(--sub-depth, 0) * 12px);
}
.sub-chip-twisty {
  display: grid;
  place-items: center;
  width: 14px;
  height: 14px;
  flex-shrink: 0;
  border: 0;
  background: none;
  padding: 0;
  color: var(--text-3);
  cursor: pointer;
}
.sub-chip-twisty.is-spacer {
  cursor: default;
}
.sub-chip-twisty svg {
  transition: transform 0.15s ease;
}
.sub-chip-twisty svg.is-open {
  transform: rotate(90deg);
}
</style>
