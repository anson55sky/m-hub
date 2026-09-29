<script setup lang="ts">
/**
 * 统一捕获框（2026-09-29 新增）。
 *
 * ## 它解决什么
 *
 * m-hub 有五个去处（速记 / 待办 / 提示词 / 倒计时 / 速达），但每次记一件事
 * 都得先想「这该去哪」。这个框只让用户写内容，去向由 `utils/capture.ts` 判断。
 *
 * ## 为什么要「先显示落点、再回车」
 *
 * 自动判断一定有猜错的时候，而猜错的代价是「东西记到了别处、还有时间记错了」——
 * 用户不会发现。所以：
 *   · 落点与解析出的时间**在输入过程中就显示出来**，用户看得见、来得及改
 *   · 前缀（`!` `?` `~` `>`）永远压过自动判断
 *   · 解析不出时间就**不编一个**（宁可不猜）
 *
 * 一旦用户按了回车，事已落库。所以「改」的窗口必须在他按之前。
 */
import { computed, nextTick, ref, toRef, watch } from 'vue'
import { useStore } from '../stores/workbench'
import { parseCapture, DEST_LABEL, type CaptureDest } from '../utils/capture'
import { shortcutLabel } from '../utils/platform'
import { useFocusTrap } from '../composables/useFocusTrap'

const props = defineProps<{ visible: boolean }>()
const emit = defineEmits<{ (e: 'close'): void; (e: 'saved', dest: CaptureDest): void }>()

const store = useStore()
const text = ref('')
const cardRef = ref<HTMLElement | null>(null)
const inputRef = ref<HTMLTextAreaElement | null>(null)
const saving = ref(false)
const error = ref('')

useFocusTrap(toRef(props, 'visible'), cardRef, inputRef)

const captureShortcutLabel = computed(() =>
  shortcutLabel('capture', store.state.config.capture_shortcut),
)

/** 实时解析：用户每敲一个字就能看到「这会被记到哪、什么时间」 */
const plan = computed(() => parseCapture(text.value))

/** 时间显示。刻意**不**自己格式化：交给 toLocaleString，避免自己写一套「今天/明天」判断 */
const dueText = computed(() =>
  plan.value.dueAt == null ? null : new Date(plan.value.dueAt).toLocaleString('zh-CN'),
)

const canSave = computed(() => plan.value.text.length > 0 && !saving.value)

/** URL 落速达时，「标题」就是链接本身；其余落点用正文当标题 */
async function save() {
  if (!canSave.value) return
  saving.value = true
  error.value = ''
  const p = plan.value
  try {
    switch (p.dest) {
      case 'url': {
        // 速达（网页）：正文即链接，名字用域名兜底（与 Suda 既有创建路径一致）
        const host = (() => {
          try {
            return new URL(p.text).hostname
          } catch {
            return p.text
          }
        })()
        await store.addResource({ kind: 'web', name: host, target: p.text })
        break
      }
      case 'todo': {
        const t = await store.createTodo(p.text)
        // 有解析出的时间才补排期，且是**创建之后**补：scheduleTodo 要 todo 的 id，
        // 而 id 是 createTodo 返回的。先建后补的代价是最多多一次 UPDATE。
        // ⚠️ 只在**认得出**时补 —— 认不出就是 null，宁可没有时间也不要编一个
        // （约束见 utils/capture.ts 顶部「宁可不猜」）。
        if (p.dueAt != null && t?.id != null) {
          await store.scheduleTodo(t.id, p.dueAt, null)
        }
        break
      }
      case 'snippet':
        // 提示词需要「标题 + 内容」，而捕获框只有一行：前 20 字当标题，
        // 整行当内容。这与「复制一段话存成提示词」的常见用法一致。
        await store.addSnippet(p.text.slice(0, 20), p.text)
        break
      case 'countdown': {
        // 倒计时必须给 endAt / totalMs / repeatMode，这里统一记成一次性。
        // totalMs = endAt - now：这是「总共要跑多久」，与 endAt 是两个独立字段。
        const endAt = p.dueAt ?? Date.now() + 30 * 60_000
        await store.addCountdown({
          name: p.text || '倒计时',
          repeatMode: 'once',
          endAt,
          totalMs: Math.max(0, endAt - Date.now()),
        })
        break
      }
      default:
        await store.addNote(p.text.slice(0, 20))
    }
    emit('saved', p.dest)
    emit('close')
    text.value = ''
  } catch (e) {
    error.value = String(e)
  } finally {
    saving.value = false
  }
}

function onKeydown(e: KeyboardEvent) {
  if (!props.visible) return
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('close')
    return
  }
  // Enter 保存、Shift+Enter 换行。捕获框的输入常常不止一行
  // （比如要粘一段话存成速记），所以换行不能被 Enter 吃掉。
  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
    e.preventDefault()
    void save()
  }
}

watch(
  () => props.visible,
  (v) => {
    if (v) {
      text.value = ''
      error.value = ''
      nextTick(() => inputRef.value?.focus())
    }
  },
)

/** 前缀提示：让用户知道有这些约定，而不是靠猜 */
const HINTS: { mark: string; desc: string }[] = [
  { mark: '!', desc: '速记' },
  { mark: '?', desc: '提示词' },
  { mark: '>', desc: '待办' },
  { mark: '~', desc: '倒计时（需带时间）' },
]
</script>

<template>
  <Teleport to="body">
    <Transition name="search-fade">
      <div v-if="visible" class="capture-mask" @mousedown.self="emit('close')">
        <div ref="cardRef" class="capture-card" role="dialog" aria-modal="true" aria-label="统一捕获">
          <textarea
            ref="inputRef"
            v-model="text"
            class="capture-input"
            rows="3"
            placeholder="记一下…（明天下午3点交周报 / https://… / !只当速记 / ?存成提示词）"
            aria-label="捕获内容"
            @keydown="onKeydown"
          />

          <!-- 落点预览：这是本组件的核心 —— 判断结果在回车之前就摆在用户眼前 -->
          <div class="capture-preview" role="status" aria-live="polite">
            <span v-if="plan.text.length > 0 || plan.dueAt != null" class="preview-dest">
              记到 <b>{{ DEST_LABEL[plan.dest] }}</b>
              <span v-if="dueText" class="preview-due">· {{ dueText }}</span>
            </span>
            <span v-else class="preview-idle">输入即自动判断去处</span>
            <span v-if="plan.dueAt != null && plan.dest === 'todo'" class="preview-warn">
              时间是从文字里认出来的，存前请核对
            </span>
          </div>

          <p v-if="error" class="capture-error" role="alert">{{ error }}</p>

          <div class="capture-foot">
            <div class="capture-hints">
              <span v-for="h in HINTS" :key="h.mark" class="hint">
                <kbd>{{ h.mark }}</kbd>{{ h.desc }}
              </span>
            </div>
            <div class="capture-keys">
              <span><kbd>↵</kbd> 存 · <kbd>⇧↵</kbd> 换行</span>
              <span><kbd>{{ captureShortcutLabel }}</kbd> 随时呼出</span>
              <span><kbd>Esc</kbd> 关闭</span>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.capture-mask {
  position: fixed;
  /* 铺满整个窗口。外扩带已移除，窗口内容就是整个窗口，
     所以圆角与内容圆角落在同一个「窗口角」，两者严格重合 ——
     不声明圆角的话，这一帧会用方形遮罩把主窗口的圆角盖回去
     （约定 69 第四条，check-rounded-window.mjs 守着）。 */
  border-radius: var(--window-radius);
  inset: 0;
  z-index: 200;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  /* 顶部偏移：贴着标题栏下方，像 Raycast 那样「从上面落下来」 */
  padding-top: 12vh;
  background: var(--scrim);
}
.capture-card {
  width: 620px;
  max-width: calc(100vw - 48px);
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px;
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-dock);
}
.capture-input {
  width: 100%;
  min-height: 76px;
  padding: 10px 12px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  background: var(--bg-input, transparent);
  color: var(--text-1);
  font-size: 0.9375rem;
  line-height: 1.6;
  resize: vertical;
}
.capture-input:focus {
  outline: 2px solid var(--brand-500);
  outline-offset: -1px;
  border-color: transparent;
}
.capture-preview {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  min-height: 20px;
  font-size: 0.75rem;
  color: var(--text-2);
}
.preview-dest b {
  color: var(--brand-500);
  font-weight: 600;
}
.preview-due {
  color: var(--text-3);
}
/* 时间是「猜」出来的，必须显眼地提示核对 —— 这是写入用户数据的字段 */
.preview-warn {
  margin-left: auto;
  color: var(--text-3);
  font-size: 0.6875rem;
}
.preview-idle {
  color: var(--text-4);
}
.capture-error {
  margin: 0;
  font-size: 0.75rem;
  color: var(--text-on-accent, #fff);
  background: var(--window-close, #f05a67);
  padding: 6px 10px;
  border-radius: var(--radius-sm, 8px);
}
.capture-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
  font-size: 0.6875rem;
  color: var(--text-4);
}
.capture-hints,
.capture-keys {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
kbd {
  display: inline-block;
  padding: 1px 5px;
  border: 1px solid var(--border-soft);
  border-radius: 4px;
  background: var(--bg-card-hover, var(--bg-card-solid));
  font-family: var(--font-mono, ui-monospace, monospace);
  font-size: 0.6875rem;
  color: var(--text-2);
}
</style>
