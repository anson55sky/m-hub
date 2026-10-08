/**
 * 轻提示（toast）的调用签名 —— **唯一定义处**。
 *
 * 约定 12：`index.vue` `provide('showToast', ...)`，子组件 `inject` 使用。
 * 签名放在这里而不是各组件各写一遍，是因为它有两个可选形态而**靠猜区分必然踩错**：
 *
 * - `{ label, onClick }` → 顺带一个撤销按钮，默认显示 5s；
 * - `{ duration }`      → 只延长停留时间（话比较多的结果，比如导出/导入的清单，
 *   2.2 秒读不完）；
 * - 两者都不给 → 2.2s 普通提示。
 *
 * 早先第二个参数是「必带 label 的撤销结构」，于是「我只想多显示几秒」必须去造一个
 * 假的 label/onClick 占位。加了 `duration` 之后两种意图都不必写占位字段。
 */
export interface ToastOptions {
  /** 毫秒。给了就按它算；没给时「带撤销按钮」是 5000、纯提示是 2200。 */
  duration?: number
  /** 撤销/次要动作的按钮文案。给了就算「带按钮」，即便没给 onClick。 */
  label?: string
  onClick?: () => void
}

/** 子组件里 `inject('showToast')` 之后拿到的就是它。 */
export type ShowToast = (msg: string, opts?: ToastOptions) => void