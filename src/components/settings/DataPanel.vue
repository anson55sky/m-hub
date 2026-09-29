<script setup lang="ts">
// 数据与关于大类（存储路径 / 备份恢复 / 关于）
//
// 从 SettingsView.vue 拆出（见该文件顶部说明）：设置页按大类按需加载，
// 首次打开只需外壳 + 当前大类的代码，切大类时才加载对应面板。
import { computed, inject, onMounted, ref } from 'vue';
import { open } from '@tauri-apps/plugin-dialog';
import { Download, FolderCog, Lock, Upload } from 'lucide-vue-next';
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
