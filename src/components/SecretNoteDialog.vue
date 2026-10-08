<script setup lang="ts">
/**
 * 速达资源的加密备注弹窗（v0.8.0 发布说明 ⑧）。
 *
 * 三条口径：
 *
 * 1. **名字明文、正文密文**。名字要在列表上显示（用户靠它分辨哪条资源写了备注），
 *    正文由后端 AES-256-GCM 加密、密钥只进系统钥匙串 —— 前端任何时候都拿不到密文。
 * 2. **打开即解密**。弹窗一开就把明文放进输入框，用户直接看到自己在写什么。
 *    这是「谁用这台机器」的取舍：能打开应用的人就能看备注，而把它做成
 *    「默认打码、每次再输一遍密码」只会让用户以为它是密码管理器。
 * 3. **清空 = 删除**。备注名与正文一起清（后端同一条 SQL），不留「有名字打不开」
 *    的空备注。保存前有二次确认，因为**不可逆**：库里只有密文，没有备份。
 */
import { computed, ref, toRef, watch } from 'vue'
import { Eye, EyeOff, Lock, Trash2 } from 'lucide-vue-next'
import { isTauri, tauriApi, type Resource } from '../api/tauri'
import { useFocusTrap } from '../composables/useFocusTrap'

const props = defineProps<{
  visible: boolean
  resource: Resource | null
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'saved', resource: Resource): void
}>()

const label = ref('')
const note = ref('')
/** 正在解密 / 正在保存 */
const busy = ref(false)
/** 就地常驻的错误（钥匙串、密文损坏、网络都没有可用的 toast 时机可言） */
const error = ref('')
/** 「显示 / 隐藏正文」。默认隐藏：备注里就是密码，而弹窗可能被旁人看到。 */
const revealed = ref(false)
/** 清空正文后保存 = 删除，需要过一次确认 */
const confirmingClear = ref(false)
const cardRef = ref<HTMLElement | null>(null)
const labelInputRef = ref<HTMLInputElement | null>(null)

useFocusTrap(toRef(props, 'visible'), cardRef, labelInputRef)

const hasNote = computed(() => props.resource?.secret_label != null && props.resource.secret_label !== '')

/** 打开即拉取（解密在后端做）。每次打开都重拉，不复用上次的明文 —— */
watch(
  () => props.visible,
  async (v) => {
    if (!v) {
      revealed.value = false
      confirmingClear.value = false
      return
    }
    error.value = ''
    note.value = ''
    label.value = props.resource?.secret_label ?? ''
    busy.value = true
    try {
      if (isTauri() && props.resource) {
        const got = await tauriApi.getResourceSecret(props.resource.id)
        label.value = got.label || label.value
        note.value = got.note
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
    } finally {
      busy.value = false
    }
  },
)

/** 正文空 = 删除。按钮语义跟着「内容是否还在」变，避免用户对着空框点保存。 */
async function onSave() {
  if (!props.resource || busy.value) return
  const body = note.value.trim()
  if (!body) {
    // 什么都没有的「保存」是空动作，直接关掉而不是白跑一次 IPC
    if (!hasNote.value) {
      emit('close')
      return
    }
    if (!confirmingClear.value) {
      confirmingClear.value = true
      return
    }
  }
  busy.value = true
  error.value = ''
  try {
    const updated = await tauriApi.setResourceSecret(
      props.resource.id,
      label.value.trim(),
      body,
    )
    emit('saved', updated)
    emit('close')
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask" @click.self="emit('close')">
        <div
          ref="cardRef"
          class="modal-card secret-card"
          role="dialog"
          aria-modal="true"
          aria-label="加密备注"
        >
          <header class="secret-head">
            <h2 class="secret-title">
              <Lock :size="14" :stroke-width="2" />
              加密备注
              <span class="secret-target">{{ resource?.name }}</span>
            </h2>
            <p class="secret-hint">
              正文加密后保存在本机数据库里，密钥只放进系统钥匙串（不写进任何文件）。
              备注名不加密，这样列表上能看出哪条资源写了备注。
            </p>
          </header>

          <div class="secret-body">
            <label class="field-label">备注名</label>
            <input
              ref="labelInputRef"
              v-model="label"
              class="field-input"
              type="text"
              maxlength="40"
              placeholder="如：账号密码 / 备用入口"
              :disabled="busy"
            />

            <div class="secret-label-row">
              <label class="field-label secret-note-label">正文</label>
              <button
                class="secret-reveal"
                type="button"
                :aria-pressed="revealed"
                @click="revealed = !revealed"
              >
                <EyeOff v-if="revealed" :size="12" :stroke-width="2" />
                <Eye v-else :size="12" :stroke-width="2" />
                {{ revealed ? '隐藏' : '显示' }}
              </button>
            </div>

            <!--
              隐藏态靠 `-webkit-text-security` 而不是 `type="password"`：
              `<textarea>` 没有 type 属性，写上去只是个被忽略的属性 ——
              界面看起来「加了密」，实际是明文（约定 78 的那种「界面上没说的话」）。
              该属性在 WebKit 与 Chromium 上都生效（本工程两平台）。
            -->
            <textarea
              v-model="note"
              class="field-input secret-textarea"
              :class="{ masked: !revealed }"
              rows="6"
              maxlength="20000"
              placeholder="账号、密码、串行号……"
              spellcheck="false"
              :disabled="busy"
            />
          </div>

          <p v-if="error" class="secret-error">{{ error }}</p>
          <p v-else-if="confirmingClear" class="secret-warn">
            正文已空 —— 再点一次「保存」即永久删除这条备注（库里只有密文，没有备份）。
          </p>

          <footer class="secret-foot">
            <span class="secret-count">{{ note.length }} 字</span>
            <div class="secret-acts">
              <button class="ghost-btn" type="button" :disabled="busy" @click="emit('close')">
                取消
              </button>
              <button
                v-if="hasNote && note.trim()"
                class="ghost-btn secret-clear"
                type="button"
                :disabled="busy"
                title="清空正文并保存 = 永久删除这条备注"
                @click="note = ''"
              >
                <Trash2 :size="13" :stroke-width="2" />
                清空
              </button>
              <button class="pill-btn" type="button" :disabled="busy" @click="onSave">
                {{ confirmingClear && !note.trim() ? '确认删除' : '保存' }}
              </button>
            </div>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
/* 走约定 56 的 padding 口径：卡片自身 0、头/体/脚各自内边距 */
.secret-card {
  width: min(520px, calc(100vw - 48px));
  padding: 0;
  display: flex;
  flex-direction: column;
  max-height: calc(100vh - 80px);
  overflow-y: auto;
}

.secret-head {
  padding: 16px 18px 0;
}

.secret-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 1rem;
  font-weight: 600;
  color: var(--text-1);
}

.secret-target {
  font-size: 0.75rem;
  font-weight: 500;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.secret-hint {
  margin: 6px 0 12px;
  font-size: 0.6875rem;
  line-height: 1.6;
  color: var(--text-3);
}

.secret-body {
  padding: 0 18px;
}

.secret-label-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 10px;
}

.secret-note-label {
  margin-bottom: 6px;
}

.secret-reveal {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 8px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--text-3);
  font-size: 0.6875rem;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
}

.secret-reveal:hover {
  color: var(--brand-500);
  border-color: color-mix(in srgb, var(--brand-500) 45%, transparent);
}

.secret-textarea {
  height: auto;
  resize: vertical;
  font-family: inherit;
  line-height: 1.6;
}

/* 隐藏态的正文显示为掩码（`<textarea>` 无 type=password，只能走这条；
   WebKit 与 Chromium 都支持 `-webkit-text-security`） */
.secret-textarea.masked {
  -webkit-text-security: disc;
}

.secret-error {
  margin: 10px 0 0;
  font-size: 0.75rem;
  color: var(--c-red);
  line-height: 1.6;
}

.secret-warn {
  margin: 10px 0 0;
  font-size: 0.75rem;
  color: var(--c-orange);
  line-height: 1.6;
}

.secret-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-top: 14px;
  padding: 12px 18px;
  border-top: 1px solid var(--border-soft);
}

/* 遮罩淡入淡出（与其它弹窗同款；Transition 类名不带 scoped，改 global） */
:global(.mask-enter-active),
:global(.mask-leave-active) {
  transition: opacity 0.18s ease-out;
}

:global(.mask-enter-from),
:global(.mask-leave-to) {
  opacity: 0;
}

.secret-count {
  font-size: 0.6875rem;
  color: var(--text-4);
}

.secret-acts {
  display: flex;
  align-items: center;
  gap: 8px;
}

.secret-clear:hover {
  color: var(--c-red);
  border-color: color-mix(in srgb, var(--c-red) 45%, transparent);
}
</style>