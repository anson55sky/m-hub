<script setup lang="ts">
/**
 * 扫描桌面：把桌面上的东西挑着加入速达（2026-10-03）。
 *
 * ## 与「扫描已安装应用」的本质差别
 *
 * 那个扫的是 `/Applications`，选中的都是 `.app`，加进速达是**复制一条记录**，
 * 源文件完全不动。这个对话框面对的是**用户自己的桌面**，所以多了两件事：
 *
 * ① **可以顺手清掉桌面的快捷方式** —— 但只有快捷方式（`removable`）。
 *    文件、文件夹、`.app` 一律保留，且界面按 `removable` 禁用它们的勾选框
 *    并写明原因，不让用户勾上一个注定不会发生的事。
 *
 * ② **必须显示「已加入」的重复标记** —— 桌面上的东西大概率已经在速达里了，
 *    重复加入会得到两条一样的记录。扫完先比对已有列表，把已存在的标出来。
 */
import { computed, ref, watch } from 'vue'
import { AppWindow, FileText, Folder, Globe, Loader2 } from 'lucide-vue-next'
import type { DesktopEntry } from '../api/tauri'
import { isTauri, tauriApi } from '../api/tauri'
import { useStore } from '../stores/workbench'

const props = defineProps<{ visible: boolean }>()
// ⚠️ 调用签名式，同上（本工程 Vue 版本对元组式推导不对）
const emit = defineEmits<{
  (e: 'close'): void
  /** 选中要加入速达的条目 + 是否清理桌面快捷方式 */
  (e: 'picked', entries: DesktopEntry[], removeShortcuts: boolean): void
}>()

const store = useStore()
const entries = ref<DesktopEntry[]>([])
const loading = ref(false)
const error = ref('')
const selected = ref(new Set<string>())
const removeShortcuts = ref(false)

/** 已经在速达里的路径（小写比对：macOS 默认文件系统大小写不敏感） */
const existingPaths = computed(() => {
  const m = new Set<string>()
  for (const r of store.state.resources) {
    if (r.target) m.add(r.target.toLowerCase())
  }
  return m
})

function isDuplicate(e: DesktopEntry) {
  return existingPaths.value.has(e.path.toLowerCase())
}

const KIND_ICON = {
  app: AppWindow,
  folder: Folder,
  file: FileText,
  alias: Globe,
  webloc: Globe,
} as const

const kindLabel: Record<DesktopEntry['kind'], string> = {
  app: '应用',
  folder: '文件夹',
  file: '文件',
  alias: '快捷方式',
  webloc: '网页',
}

function toggle(id: string) {
  const next = new Set(selected.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  selected.value = next
}

function toggleAll() {
  const ids = entries.value.filter((e) => e.addable && !isDuplicate(e)).map((e) => e.id)
  const allOn = ids.length > 0 && ids.every((id) => selected.value.has(id))
  const next = new Set(selected.value)
  for (const id of ids) {
    if (allOn) next.delete(id)
    else next.add(id)
  }
  selected.value = next
}

const picked = computed(() => entries.value.filter((e) => selected.value.has(e.id)))

/** 勾了清理但选中项里没有可删的 —— 直接说明，不让用户以为会删文件 */
const pickedRemovableCount = computed(() => picked.value.filter((e) => e.removable).length)

async function load() {
  if (!isTauri()) return
  loading.value = true
  error.value = ''
  try {
    entries.value = await tauriApi.scanDesktop()
    if (!entries.value.length) {
      error.value = '桌面上没有可加入速达的项目（隐藏文件已略过）'
    }
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

watch(
  () => props.visible,
  (v) => {
    if (!v) return
    selected.value = new Set()
    removeShortcuts.value = false
    void load()
  },
  { immediate: true },
)
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="mask" @click.self="emit('close')">
      <div class="modal-card desktop-card" role="dialog" aria-label="扫描桌面">
        <header class="desktop-head">
          <h3>扫描桌面</h3>
          <button class="ghost-btn" type="button" @click="emit('close')">关闭</button>
        </header>

        <p class="desktop-hint">
          桌面上的<strong>应用、文件夹、文件</strong>都可以加入速达，磁盘上的原东西一个都不动。
        </p>

        <div v-if="loading" class="desktop-empty">
          <Loader2 :size="18" class="spin" />
          正在扫描桌面…
        </div>
        <div v-else-if="error" class="desktop-empty">{{ error }}</div>
        <div v-else-if="!entries.length" class="desktop-empty">桌面上没有可加入的项目</div>

        <template v-else>
          <div class="desktop-toolbar">
            <button class="ghost-btn" type="button" @click="toggleAll">全选 / 取消全选</button>
            <span class="desktop-count">已选 {{ picked.length }} 项</span>
            <!-- 清理只对快捷方式生效：文案必须说清「只清快捷方式」 -->
            <label class="desktop-clean" :title="'只删 .alias / .webloc / 软链接，文件本身一个都不删'">
              <input v-model="removeShortcuts" type="checkbox" />
              加入后清理桌面快捷方式
              <span class="desktop-clean-note">
                （只清快捷方式
                <template v-if="pickedRemovableCount === 0">，当前所选没有快捷方式</template>
              </span>
            </label>
          </div>

          <ul class="desktop-list">
            <li v-for="e in entries" :key="e.id" class="desktop-item">
              <input
                type="checkbox"
                :checked="selected.has(e.id)"
                :disabled="!e.addable || isDuplicate(e)"
                @change="toggle(e.id)"
              />
              <component :is="KIND_ICON[e.kind]" :size="14" :stroke-width="1.9" class="desktop-ic" />
              <span class="desktop-name" :title="e.path">{{ e.name }}</span>
              <span class="desktop-kind">{{ kindLabel[e.kind] }}</span>
              <span v-if="isDuplicate(e)" class="desktop-dup">已在速达</span>
              <span v-else-if="removeShortcuts && e.removable" class="desktop-clean-tag">将被清理</span>
              <span v-else-if="!e.removable" class="desktop-keep" title="不是快捷方式，清理时不动它">保留</span>
            </li>
          </ul>
        </template>

        <footer class="desktop-foot">
          <button
            class="primary-btn"
            type="button"
            :disabled="!picked.length"
            @click="emit('picked', picked, removeShortcuts)"
          >
            加入速达（{{ picked.length }}）
          </button>
        </footer>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.desktop-card {
  width: min(560px, calc(100vw - 64px));
  max-height: min(70vh, 620px);
  display: flex;
  flex-direction: column;
  padding: 0;
}
.desktop-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 18px 0;
}
.desktop-head h3 {
  margin: 0;
  font-size: 15px;
  font-weight: 650;
}
.desktop-hint {
  margin: 6px 18px 10px;
  font-size: 12.5px;
  color: var(--text-2);
  line-height: 1.55;
}
.desktop-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 34px 0;
  color: var(--text-3);
  font-size: 13px;
}
.desktop-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 18px 10px;
  flex-wrap: wrap;
}
.desktop-count {
  font-size: 12px;
  color: var(--text-2);
  margin-right: auto;
}
.desktop-clean {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 12.5px;
  color: var(--text-2);
  cursor: pointer;
}
.desktop-clean-note {
  color: var(--text-3);
}
.desktop-list {
  list-style: none;
  margin: 0;
  padding: 0 10px;
  overflow-y: auto;
  flex: 1;
  min-height: 120px;
}
.desktop-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 8px;
  border-radius: var(--radius-sm, 8px);
  font-size: 13px;
}
.desktop-item:hover {
  background: var(--bg-card-soft);
}
.desktop-item input:disabled {
  opacity: 0.4;
}
.desktop-ic {
  color: var(--text-3);
  flex-shrink: 0;
}
.desktop-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.desktop-kind,
.desktop-dup,
.desktop-keep,
.desktop-clean-tag {
  font-size: 11.5px;
  color: var(--text-3);
  flex-shrink: 0;
}
.desktop-dup {
  color: var(--text-2);
}
.desktop-clean-tag {
  color: var(--c-orange);
}
.desktop-foot {
  display: flex;
  justify-content: flex-end;
  padding: 12px 18px 16px;
}
.primary-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.spin {
  animation: desktop-spin 0.9s linear infinite;
}
@keyframes desktop-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>