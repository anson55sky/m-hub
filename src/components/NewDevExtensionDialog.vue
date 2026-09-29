<script setup lang="ts">
/**
 * 新建本机扩展对话框（2026-09-29 新增）。
 *
 * ## 为什么要有这个对话框，而不是给个「新建」按钮直接建
 *
 * 扩展要落到磁盘上的某个目录，而「哪个目录」是用户的选择 —— 扩展是要长期
 * 存在、还要进版本控制的代码，把它悄悄塞进应用数据目录是最糟的默认：
 * 用户找不到、备份应用数据时会连着一起搬走、换机就丢。所以位置显式选。
 *
 * ## 表单为什么这么少
 *
 * 只有四个字段：名称、ID、形态、保存位置。manifest 还有七八个字段，但它们不该
 * 在「新建」这一步问 —— 用户此刻还不知道 `surfaces` 与 `openIn` 的区别，
 * 让他填等于让他瞎填。剩下的字段由骨架用能跑的默认值写好，README 里逐项解释，
 * 用户想改随时改。**能在创建后改的，就不要在创建时问。**
 *
 * ## 校验为什么在输入时就做
 *
 * ID 会成为入口 URL 的一段（`/<id>/index.html`），也是资产作用域的匹配键，
 * 所以大写、斜杠、空格都会出问题。等提交后由后端报错，用户已经白填了一遍。
 * 这里按同一套规则（`scripts/` 之外的唯一实现是 Rust 侧的 `validate_id`）
 * 提前给出提示 —— 口径相同的规则写两遍有漂移风险，所以前端的判断刻意保守：
 * 只拦**一定非法**的，边界（长度等）交给后端。
 */
import { computed, ref, toRef, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { isTauri, tauriApi } from '../api/tauri'
import { useFocusTrap } from '../composables/useFocusTrap'

const props = defineProps<{ visible: boolean }>()
const emit = defineEmits<{ (e: 'close'): void; (e: 'created', dir: string): void }>()

const cardRef = ref<HTMLElement | null>(null)
const nameRef = ref<HTMLInputElement | null>(null)
useFocusTrap(toRef(props, 'visible'), cardRef, nameRef)

const name = ref('')
const id = ref('')
/** 用户是否手动改过 id —— 改过之后就不再跟着名称自动生成 */
const idTouched = ref(false)
const kind = ref<'module' | 'view'>('module')
const parentDir = ref('')
const busy = ref(false)
const error = ref('')

/** 默认位置：`~/Documents/m-hub-extensions`。可改，只是个起点 */
function defaultParent(): string {
  return '~/Documents/m-hub-extensions'
}

/** 名称 → id 片段。只保留 ASCII，非 ASCII 一律丢掉（后端 slugify 同一口径） */
function slugOf(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
}

/**
 * id 的本地校验。**只拦一定非法的**，长度/格式边界交给后端 ——
 * 两边规则写全必然漂移，而这里的作用只是「别让用户白填一遍」。
 */
const idError = computed(() => {
  const v = id.value.trim()
  if (!v) return '扩展 ID 不能为空'
  if (!/^[a-z0-9]/.test(v)) return '须以小写字母或数字开头'
  if (!/^[a-z0-9.-]+$/.test(v)) return '只允许小写字母、数字、点和连字符'
  return ''
})

const canSubmit = computed(
  () => !!name.value.trim() && !idError.value && !!parentDir.value.trim() && !busy.value,
)

watch(name, (v) => {
  if (idTouched.value) return
  const slug = slugOf(v)
  id.value = slug ? `local.${slug}` : ''
})

watch(
  () => props.visible,
  (v) => {
    if (!v) return
    name.value = ''
    id.value = ''
    idTouched.value = false
    kind.value = 'module'
    parentDir.value = defaultParent()
    error.value = ''
    // `~/…` 不在这里展开：前端没有可靠的 home 路径（拿 `~` 去问对话框会得到
    // 字面量 `~` 目录）。改由后端 `create_dev_extension` 统一展开 ——
    // 一处规则，所有调用方都受益，前端不必各自实现一遍。
  },
)

async function pickDir() {
  if (!isTauri()) return
  try {
    const picked = await open({ directory: true, multiple: false, title: '选择保存位置' })
    if (typeof picked === 'string' && picked) parentDir.value = picked
  } catch (e) {
    error.value = String(e)
  }
}

async function submit() {
  if (!canSubmit.value) return
  busy.value = true
  error.value = ''
  try {
    const r = await tauriApi.createDevExtension(
      parentDir.value.trim(),
      id.value.trim(),
      name.value.trim(),
      kind.value,
    )
    if (r.registered) {
      emit('created', r.dir)
      emit('close')
    } else {
      // 目录已建好、但没能自动挂上：如实说清楚，不假装成功也不让用户重填一遍
      emit('created', r.dir)
      emit('close')
    }
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('close')
  } else if (e.key === 'Enter' && !e.isComposing) {
    e.preventDefault()
    void submit()
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="search-fade">
      <div v-if="visible" class="modal-mask nde-mask" @mousedown.self="emit('close')">
        <div
          ref="cardRef"
          class="modal-card nde-card"
          role="dialog"
          aria-modal="true"
          aria-label="新建本机扩展"
          @keydown="onKeydown"
        >
          <h3 class="nde-title">新建本机扩展</h3>
          <p class="nde-lead">
            在你指定的位置生成一个能直接跑的扩展目录（manifest + 入口页 + README），
            并立即接入「我的扩展」。改代码会自动热重载。
          </p>

          <label class="nde-field">
            <span class="nde-label">名称</span>
            <input
              ref="nameRef"
              v-model="name"
              class="nde-input"
              type="text"
              placeholder="例如：番茄钟"
              maxlength="40"
            />
          </label>

          <label class="nde-field">
            <span class="nde-label">
              扩展 ID
              <span class="nde-hint">同时是入口 URL 的一部分，创建后不建议再改</span>
            </span>
            <input
              v-model="id"
              class="nde-input"
              :class="{ invalid: !!id && !!idError }"
              type="text"
              spellcheck="false"
              placeholder="local.pomodoro"
              @input="idTouched = true"
            />
            <span v-if="id && idError" class="nde-err">{{ idError }}</span>
          </label>

          <div class="nde-field">
            <span class="nde-label">形态</span>
            <div class="nde-kinds" role="radiogroup" aria-label="扩展形态">
              <button
                class="nde-kind"
                :class="{ on: kind === 'module' }"
                type="button"
                role="radio"
                :aria-checked="kind === 'module'"
                @click="kind = 'module'"
              >
                <b>工作台卡片</b>
                <small>放进工作台的一格，适合小组件</small>
              </button>
              <button
                class="nde-kind"
                :class="{ on: kind === 'view' }"
                type="button"
                role="radio"
                :aria-checked="kind === 'view'"
                @click="kind = 'view'"
              >
                <b>独立页面</b>
                <small>在主区整页打开，适合有内容的功能</small>
              </button>
            </div>
            <span class="nde-hint">先不改也行，创建后改 manifest 里的 kind 即可。</span>
          </div>

          <div class="nde-field">
            <span class="nde-label">
              保存位置
              <span class="nde-hint">会新建一个以 ID 末段命名的子目录；已存在则拒绝，不会覆盖</span>
            </span>
            <div class="nde-path">
              <span class="nde-path-text" :title="parentDir">{{ parentDir || '未选择' }}</span>
              <button class="nde-pick" type="button" @click="pickDir">更改…</button>
            </div>
          </div>

          <p v-if="error" class="nde-error" role="alert">{{ error }}</p>

          <div class="nde-foot">
            <button class="nde-btn" type="button" @click="emit('close')">取消</button>
            <button class="nde-btn primary" type="button" :disabled="!canSubmit" @click="submit">
              {{ busy ? '创建中…' : '创建' }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
/*
 * 遮罩层刻意**不**自己写，直接用共享的 `.modal-mask` / `.modal-card`：
 *
 * 第一版只抄了 `background: var(--bg-card-solid)`，结果在暗色主题下几乎透明 ——
 * `--bg-card-solid` 在暗色是 `rgba(255,255,255,0.2)`，那是**常驻玻璃卡片**的口径
 * （ADR 0003：表面只属于卡片，弹层才用真 blur）。让 20% 的底色独自承担
 * 「对话框」的可读性，后面的内容就直接透了过来（用户反馈「看不清」）。
 *
 * 真正让这个底色可读的是 `.modal-card` 上的
 * `backdrop-filter: blur(18px) saturate(160%)` —— 共享类已经带了。
 * 自己重写一遍等于把 blur 弄丢，而且下次改共享样式时这里不会跟着变。
 * 共享类还顺带给了 `--scrim` 遮罩、`--window-radius` 圆角与入场动画，
 * 都在 check-rounded-window.mjs 的守��范围内 —— 自己写就得重复这些约定。
 *
 * 弹层 z-index 高于普通内容（100 > 常驻卡片），符合共享约定，故不覆写。
 */
.nde-card {
  /* 尺寸与排布是本对话框自己的，底色/模糊/圆角/动画一律继承共享类 */
  width: 520px;
  max-width: calc(100vw - 48px);
  max-height: calc(100vh - 96px);
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 18px;
}
.nde-title {
  margin: 0;
  font-size: 0.9375rem;
  font-weight: 600;
}
.nde-lead {
  margin: 0;
  font-size: 0.75rem;
  line-height: 1.6;
  color: var(--text-3);
}
.nde-field {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.nde-label {
  display: flex;
  align-items: baseline;
  gap: 6px;
  flex-wrap: wrap;
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-2);
}
.nde-hint {
  font-size: 0.6875rem;
  font-weight: 400;
  color: var(--text-4);
}
.nde-input {
  width: 100%;
  padding: 7px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm, 8px);
  background: var(--bg-input, transparent);
  color: var(--text-1);
  font-size: 0.8125rem;
}
.nde-input:focus-visible {
  outline: 2px solid var(--brand-500);
  outline-offset: -1px;
}
.nde-input.invalid {
  border-color: var(--window-close, #f05a67);
}
.nde-err {
  font-size: 0.6875rem;
  color: var(--window-close, #f05a67);
}
.nde-kinds {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}
.nde-kind {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm, 8px);
  background: transparent;
  color: var(--text-2);
  text-align: left;
  font-size: 0.75rem;
  cursor: pointer;
  transition: border-color 0.14s ease-out, background 0.14s ease-out;
}
.nde-kind b {
  font-size: 0.8125rem;
  color: var(--text-1);
}
.nde-kind small {
  font-size: 0.6875rem;
  color: var(--text-4);
}
.nde-kind:hover {
  background: var(--bg-card-hover, var(--bg-card-solid));
}
.nde-kind.on {
  border-color: color-mix(in srgb, var(--brand-500) 45%, transparent);
  background: color-mix(in srgb, var(--brand-500) 8%, transparent);
}
.nde-path {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 8px 5px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm, 8px);
  background: var(--bg-input, transparent);
}
.nde-path-text {
  flex: 1 1 auto;
  min-width: 0;
  font-family: var(--font-mono, ui-monospace, monospace);
  font-size: 0.6875rem;
  color: var(--text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.nde-pick {
  flex: 0 0 auto;
  padding: 3px 8px;
  border: 1px solid var(--border-soft);
  border-radius: 6px;
  background: transparent;
  color: var(--text-2);
  font-size: 0.6875rem;
  cursor: pointer;
}
.nde-error {
  margin: 0;
  padding: 6px 10px;
  border-radius: var(--radius-sm, 8px);
  background: var(--window-close, #f05a67);
  color: var(--text-on-accent, #fff);
  font-size: 0.75rem;
}
.nde-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding-top: 2px;
}
.nde-btn {
  padding: 6px 14px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm, 8px);
  background: transparent;
  color: var(--text-2);
  font-size: 0.8125rem;
  cursor: pointer;
}
.nde-btn.primary {
  border-color: transparent;
  background: var(--brand-500);
  color: var(--text-on-accent, #fff);
}
.nde-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
