<script setup lang="ts">
// 数据与关于大类（存储路径 / 备份恢复 / 关于）
//
// 从 SettingsView.vue 拆出（见该文件顶部说明）：设置页按大类按需加载，
// 首次打开只需外壳 + 当前大类的代码，切大类时才加载对应面板。
import { computed, inject, onMounted, ref } from 'vue';
import { open } from '@tauri-apps/plugin-dialog';
import { Download, Eraser, FolderCog, Lock, Upload } from 'lucide-vue-next';
import { isTauri, tauriApi } from '../../api/tauri';
import type { DataPathInfo } from '../../api/tauri';
import AboutSection from '../AboutSection.vue';

const showToast = inject<(msg: string) => void>('showToast', () => {})

// ---- 数据存储路径 ----
const dataPathInfo = ref<DataPathInfo | null>(null)
const changeDataTarget = ref<string | null>(null)
const changeDataBusy = ref(false)

const dataPathLabel = computed(() => {
  const info = dataPathInfo.value
  if (!info) return '加载中…'
  if (info.mode === 'portable') return `便携版 · ${info.path}`
  return info.path
})

async function loadDataPath() {
  if (!isTauri()) return
  try {
    dataPathInfo.value = await tauriApi.getDataPath()
  } catch (e) {
    showToast(`读取数据路径失败：${String(e)}`)
  }
}

async function onChangeDataDir() {
  if (!isTauri() || dataPathInfo.value?.mode === 'portable') return
  const dir = await open({ multiple: false, directory: true })
  if (typeof dir !== 'string') return
  if (dir === dataPathInfo.value?.path) {
    showToast('所选目录与当前目录相同')
    return
  }
  changeDataTarget.value = dir
}

function cancelChangeDataDir() {
  changeDataTarget.value = null
}

async function confirmChangeDataDir() {
  if (!changeDataTarget.value || changeDataBusy.value) return
  changeDataBusy.value = true
  try {
    await tauriApi.changeDataDir(changeDataTarget.value)
    changeDataTarget.value = null
    showToast('数据已迁移，即将重启')
    setTimeout(() => void tauriApi.restartApp(), 700)
  } catch (e) {
    showToast(`迁移失败：${String(e)}`)
  } finally {
    changeDataBusy.value = false
  }
}

// ---- 自动备份（2026-09-29 新增）----
// 与手动备份的关系：手动是「想起来时做一次」，自动是「按间隔做」并轮转保留 N 份。
// 刻意**默认关闭且要求显式选目录**：备份放哪儿是用户的选择；猜一个位置的话，
// 备份可能和被备份的数据落在同一块盘上 —— 盘坏了就一起没了，看着有备份其实没有。
const autoBackupDir = ref('')
const autoBackupHours = ref(24)
const autoBackupKeep = ref(7)
const autoBackupSaving = ref(false)
const autoBackupLastText = ref('')

const HOUR_OPTIONS = [
  { v: 0, label: '关闭' },
  { v: 6, label: '每 6 小时' },
  { v: 12, label: '每 12 小时' },
  { v: 24, label: '每天' },
  { v: 72, label: '每 3 天' },
  { v: 168, label: '每周' },
]

async function loadAutoBackup() {
  try {
    const c = await tauriApi.getAutoBackupConfig()
    autoBackupDir.value = c.dir ?? ''
    autoBackupHours.value = c.hours ?? 24
    autoBackupKeep.value = c.keep ?? 7
    autoBackupLastText.value =
      c.lastMs > 0 ? new Date(c.lastMs).toLocaleString('zh-CN') : '尚未备份过'
  } catch (e) {
    showToast(`读取自动备份设置失败：${String(e)}`)
  }
}

async function saveAutoBackup() {
  autoBackupSaving.value = true
  try {
    await tauriApi.setAutoBackupConfig(
      autoBackupDir.value,
      Number(autoBackupHours.value),
      Number(autoBackupKeep.value),
    )
    showToast('自动备份设置已保存，并立即备份了一份')
    await loadAutoBackup()
  } catch (e) {
    showToast(`设置失败：${String(e)}`)
  } finally {
    autoBackupSaving.value = false
  }
}

async function pickAutoBackupDir() {
  // 复用本文件既有的目录选择方式（@tauri-apps/plugin-dialog 的 open），
  // 不新造第二套 —— 同一件事两套写法，将来改交互就会漏一处
  const dir = await open({ multiple: false, directory: true })
  if (typeof dir === 'string' && dir) autoBackupDir.value = dir
}

onMounted(() => {
  void loadAutoBackup()
})

// ---- 数据备份 / 恢复 ----
const confirmRestore = ref(false)
let confirmTimer: ReturnType<typeof setTimeout> | null = null

async function backupData() {
  if (!isTauri()) return
  const dir = await open({ multiple: false, directory: true })
  if (typeof dir !== 'string') return
  try {
    const name = await tauriApi.backupData(dir)
    showToast(`备份完成：${name}`)
  } catch (e) {
    showToast(`备份失败：${String(e)}`)
  }
}

async function restoreData() {
  if (!isTauri()) return
  // 两段式确认：第二次点击才执行
  if (!confirmRestore.value) {
    confirmRestore.value = true
    if (confirmTimer) clearTimeout(confirmTimer)
    confirmTimer = setTimeout(() => {
      confirmRestore.value = false
    }, 3000)
    return
  }
  confirmRestore.value = false
  const file = await open({
    multiple: false,
    directory: false,
    filters: [{ name: '备份压缩包', extensions: ['zip'] }],
  })
  if (typeof file !== 'string') return
  try {
    await tauriApi.restoreData(file)
    showToast('恢复已暂存，重启应用后生效')
  } catch (e) {
    showToast(`恢复失败：${String(e)}`)
  }
}

// ---- 孤儿笔记图片清理（v0.8.1）----
//
// 「没有任何笔记引用的图片文件」。永久删除笔记时会自动清一部分，
// 但历史遗留的（早期版本 / 改过数据目录 / 导出过再删）要靠这个按钮收尾。
//
// 两段式：第一次点只**报数**（dry-run），第二次才删 —— 删的是磁盘上的文件，
// 不可逆（约定 53：不可逆操作必须先把后果交代在原地，而不是一条 toast）。
const orphanBusy = ref(false)
const orphanPreview = ref<{ files: number; bytes: number } | null>(null)
let orphanTimer: ReturnType<typeof setTimeout> | null = null

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

async function onPurgeOrphanImages() {
  if (!isTauri() || orphanBusy.value) return
  orphanBusy.value = true
  try {
    if (orphanPreview.value) {
      orphanPreview.value = null
      const s = await tauriApi.purgeOrphanNoteImages(false)
      showToast(
        s.files > 0
          ? `已清理 ${s.files} 个无引用图片（${formatBytes(s.bytes)}）`
          : '没有可清理的图片',
      )
      return
    }
    const s = await tauriApi.purgeOrphanNoteImages(true)
    if (s.files === 0) {
      orphanPreview.value = null
      showToast('没有无引用的图片，很干净')
      return
    }
    orphanPreview.value = { files: s.files, bytes: s.bytes }
    if (orphanTimer) clearTimeout(orphanTimer)
    orphanTimer = setTimeout(() => {
      orphanPreview.value = null
    }, 8000)
  } catch (e) {
    showToast(`清理失败：${String(e)}`)
  } finally {
    orphanBusy.value = false
  }
}

onMounted(() => {

  void loadDataPath()

})
</script>

<template>
        <section id="sv-sec-data" class="sv-sec" aria-label="数据">
          <h3 class="sv-sec-title">数据</h3>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">数据存储路径</span>
              <span class="setting-desc data-path">{{ dataPathLabel }}</span>
            </div>
            <button
              class="ghost-btn data-btn"
              :disabled="dataPathInfo?.mode === 'portable'"
              :title="dataPathInfo?.mode === 'portable' ? '便携版数据跟随程序目录，不可更改' : '更改数据存储目录'"
              @click="onChangeDataDir"
            >
              <FolderCog :size="14" :stroke-width="2" />
              更改
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">数据备份</span>
              <span class="setting-desc">数据库与图标，打包成压缩包</span>
            </div>
            <button class="ghost-btn data-btn" @click="backupData">
              <Download :size="14" :stroke-width="2" />
              备份
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">数据恢复</span>
              <span class="setting-desc">从备份压缩包恢复，重启后生效</span>
            </div>
            <button
              class="ghost-btn data-btn"
              :class="{ confirm: confirmRestore }"
              @click="restoreData"
            >
              <Upload :size="14" :stroke-width="2" />
              {{ confirmRestore ? '确认恢复？' : '恢复' }}
            </button>
          </div>

          <!-- 孤儿笔记图片（v0.8.1）：删掉的笔记会留下没人引用的图片文件 -->
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">清理无引用的笔记图片</span>
              <span class="setting-desc">
                笔记里已不再引用的图片文件（删过笔记、换过数据目录会留下）。
                永久删除笔记时会自动清一部分，这里收尾历史遗留。
              </span>
            </div>
            <button
              class="ghost-btn data-btn"
              :class="{ confirm: !!orphanPreview }"
              :disabled="orphanBusy"
              @click="onPurgeOrphanImages"
            >
              <Eraser :size="14" :stroke-width="2" />
              {{ orphanBusy ? '统计中…' : orphanPreview ? `确认清理 ${orphanPreview.files} 个（${formatBytes(orphanPreview.bytes)}）` : '检查并清理' }}
            </button>
          </div>

          <!-- 自动备份（2026-09-29 新增） -->
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">自动备份</span>
              <span class="setting-desc">
                定时备份到指定目录并轮转保留最近若干份。默认关闭 ——
                建议放在**与数据不同的盘**上，否则盘坏了备份一起没
              </span>
            </div>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">备份目录</span>
              <span class="setting-desc">{{ autoBackupDir || '未设置' }}</span>
            </div>
            <div class="data-btn-group">
              <button class="ghost-btn data-btn" @click="pickAutoBackupDir">更改</button>
            </div>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">备份频率</span>
              <span class="setting-desc">上次：{{ autoBackupLastText }}</span>
            </div>
            <select v-model.number="autoBackupHours" class="data-select">
              <option v-for="o in HOUR_OPTIONS" :key="o.v" :value="o.v">{{ o.label }}</option>
            </select>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">保留份数</span>
              <span class="setting-desc">超出后自动删除最旧的备份</span>
            </div>
            <input
              v-model.number="autoBackupKeep"
              class="data-input"
              type="number"
              min="1"
              max="999"
            />
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-desc">保存时会立即备份一份，便于确认是否生效</span>
            </div>
            <button
              class="ghost-btn data-btn"
              :class="{ confirm: autoBackupSaving }"
              @click="saveAutoBackup"
            >
              {{ autoBackupSaving ? '保存中…' : '保存' }}
            </button>
          </div>

          <p class="settings-foot">
            <Lock :size="12" :stroke-width="2" class="settings-lock" aria-hidden="true" />
            所有数据默认存储在本地，不会上传云端
          </p>
        </section>

        <!-- 更改数据存储路径确认弹窗 -->
        <Teleport to="body">
          <Transition name="mask">
            <div
              v-if="changeDataTarget"
              class="modal-mask"
              role="presentation"
              @click.self="cancelChangeDataDir"
            >
              <div
                class="modal-card data-move-card"
                role="dialog"
                aria-modal="true"
                aria-label="更改数据存储路径"
              >
                <h3 class="dm-title">迁移数据目录</h3>
                <p class="dm-desc">是否确认将 m-hub 的所有数据挪到以下目录？确认后将重启软件。</p>
                <div class="dm-paths">
                  <div class="dm-path">
                    <span class="dm-label">新目录</span>
                    <span class="dm-val">{{ changeDataTarget }}</span>
                  </div>
                </div>
                <footer class="dm-footer">
                  <button class="ghost-btn" type="button" @click="cancelChangeDataDir">取消</button>
                  <button
                    class="pill-btn"
                    type="button"
                    :disabled="changeDataBusy"
                    @click="confirmChangeDataDir"
                  >
                    {{ changeDataBusy ? '迁移中…' : '确认迁移' }}
                  </button>
                </footer>
              </div>
            </div>
          </Transition>
        </Teleport>

        <section id="sv-sec-about" class="sv-sec" aria-label="关于">
          <h3 class="sv-sec-title">关于</h3>
          <AboutSection />
        </section>
</template>

<style scoped>
/* 迁移数据弹窗：Teleport 到 body，不在 .settings-view 子树里，只能靠 scoped 命中 */
.data-move-card {
  width: 480px;
  max-width: calc(100vw - 48px);
}
.dm-title {
  margin: 0 0 10px;
  font-size: 1rem;
  font-weight: 700;
  color: var(--text-1);
}
.dm-desc {
  margin: 0 0 14px;
  font-size: 0.8125rem;
  line-height: 1.6;
  color: var(--text-2);
}
.dm-paths {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 16px;
}
.dm-path {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
}
.dm-label {
  font-size: 0.6875rem;
  font-weight: 700;
  color: var(--text-3);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.dm-val {
  font-size: 0.75rem;
  color: var(--text-2);
  overflow-wrap: anywhere;
}
.dm-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding-top: 12px;
  border-top: 1px solid var(--border-soft);
}

/* 弹窗遮罩过渡 */
.mask-enter-active,
.mask-leave-active {
  transition: opacity 0.18s ease-out;
}
.mask-enter-from,
.mask-leave-to {
  opacity: 0;
}
</style>
