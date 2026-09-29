<script setup lang="ts">
// 工作台大类（布局 / 提示音 / 语录 / 名言来源）
//
// 从 SettingsView.vue 拆出（见该文件顶部说明）：设置页按大类按需加载，
// 首次打开只需外壳 + 当前大类的代码，切大类时才加载对应面板。
import { inject, ref } from 'vue';
import { isTauri } from '../../api/tauri';
import AppSelect from '../AppSelect.vue';
import { useStore } from '../../stores/workbench';

const showToast = inject<(msg: string) => void>('showToast', () => {})
const store = useStore()
const emit = defineEmits<{ (e: 'open-layout-editor'): void }>()

function onToggleCountdownSound() {
  void store.setCountdownSound(!store.state.config.countdown_sound)
}

// ---- 时钟卡片语录（回车/失焦自动保存，清空则回退默认） ----
const clockQuote = ref(store.state.config.clock_quote ?? '')
const savedClockQuote = ref(clockQuote.value)

function commitClockQuote() {
  const value = clockQuote.value.trim()
  clockQuote.value = value
  if (value === savedClockQuote.value) return
  savedClockQuote.value = value
  void store.setClockQuote(value)
  showToast(value ? '时钟卡片语录已更新' : '时钟卡片语录已改为随机名言金句')
}

const QUOTE_SOURCE_OPTIONS = [
  { value: 'online', label: '在线名言（联网时随机，离线回退本地）' },
  { value: 'local', label: '本地语料（仅内置金句）' },
] as const

const quoteSource = ref(store.state.config.quote_source ?? 'online')

function onQuoteSourceChange(value: string) {
  quoteSource.value = value
  if (!isTauri()) return
  void store.setQuoteSource(value as 'online' | 'local').then(() => {
    showToast(value === 'online' ? '名言来源已设为在线' : '名言来源已设为本地语料')
  })
}
</script>

<template>
        <section id="sv-sec-workbench" class="sv-sec" aria-label="工作台">
          <h3 class="sv-sec-title">工作台</h3>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">自定义布局</span>
              <span class="setting-desc">拖拽排列主界面的模块位置与显隐（时钟、待办、提示词等），推荐布局为 12×15 棋盘，完成后回到主页面</span>
            </div>
            <button class="ghost-btn data-btn" @click="emit('open-layout-editor')">打开编辑器</button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">倒计时到点提示音</span>
              <span class="setting-desc">到点时额外播放提示音（默认关闭）</span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.countdown_sound"
              :class="{ on: store.state.config.countdown_sound }"
              @click="onToggleCountdownSound"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">时钟卡片语录</span>
              <span class="setting-desc">工作台时间卡片下方显示的一句话（留空则显示随机名言金句，点击可换一条）</span>
            </div>
            <div class="quote-edit">
              <input
                v-model="clockQuote"
                class="field-input"
                type="text"
                maxlength="50"
                placeholder="留空显示随机名言金句"
                spellcheck="false"
                @blur="commitClockQuote"
                @keydown.enter="commitClockQuote"
              />
            </div>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">名言来源</span>
              <span class="setting-desc">时钟卡片语录：在线随机名言，或仅用本地内置金句（点击语录可随机换一条）</span>
            </div>
            <AppSelect
              :model-value="quoteSource"
              :options="QUOTE_SOURCE_OPTIONS"
              aria-label="名言来源"
              class="quote-source"
              @update:model-value="onQuoteSourceChange"
            />
          </div>
        </section>
</template>

<style scoped>
/* 名言来源下拉：与上方语录输入框等宽（AppSelect 触发器通过 $attrs 接收 class，需 :deep 穿透） */
:deep(.quote-source) {
  min-width: 240px;
}
</style>
