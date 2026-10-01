// 「能不能申请成为开发者」的**唯一**判定，纯函数，可离线回归。
//
// ## 为什么单独抽一个文件
//
// 2026-10-01 事故：这个判定在 `me.ts::publicUser` 与 `submissions.ts::applyStatus`
// 里**各写了一份**，其中 `me.ts` 那份**漏了这个字段**、而客户端偏偏读 `/me`
// 那个。Rust 的 `unwrap_or(false)` 把缺失兜成 `false`，于是界面同时显示
// 「已兑换邀请码」与「先兑换邀请码才能申请」—— 自相矛盾且**零报错**。
//
// 修法有两种：① 两处改成一致；② **消掉重复**。选了②，并把逻辑放进独立
// 纯函数，让测试能直接 import **真代码**。
//
// ## 为什么不是 `export function` 放在 routes 里
//
// 曾经试过让测试「解析源码文本」来拿逻辑（`shape.ts` 那套），结果守卫本身
// 连着错了三次：函数签名跨行 → 截断点猜错；类型标注里的 `{...}` → 花括号
// 配平提前结束；改成按分号截 → 又截过了头。
//
// **教训：靠解析源码来「调用」源码逻辑，是把编译器的活抢过来做，必然出错。**
// 直接 import 就完事了；只有在「守字段在不在」这类无法 import 的场景
// （比如断言一个对象字面量的键集合）才该去读文本。
/**
 * 入参形状用**结构化子集**而不是 `User` 本身：
 * 只声明本判定真正读到的两个字段，调用方传整个 `User` 也兼容
 * （TS 的多余属性检查对「已声明的变量」不生效）。
 *
 * ⚠️ `invite_redeemed` 是 **`number`**，不是 `boolean` —— SQLite 的布尔存 0/1。
 *    我第一版写成了 `boolean`，编译器直接报错（见 conversation 记录）。
 *    **这正是不该自己造类型的好处**：`User` 是真相，照抄它就不会错。
 */
export type DeveloperGateInput = {
  invite_redeemed: number
  developer_status: string
}

/**
 * 能申请成为开发者的条件：**已兑换邀请码** ∧ **尚未申请过**。
 *
 * - `invite_redeemed` 由 `POST /api/v1/me/redeem` 置位（兑换码本身由服务端
 *   `INVITE_CODE` secret 校验，不下发给客户端）。
 * - `developer_status` 取自 `users` 表；`pending` / `approved` / `rejected`
 *   都表示「已经申请过了」，不该再放行。
 *
 * ⚠️ 与 `apply()` 的**拒绝**条件必须一致：那边是
 * `!invite_redeemed` → `invite_required`，`approved` → `already_developer`。
 * 两边不同步就会出现「界面说能申请、提交却被拒」。
 */
export function canApplyDeveloper(u: DeveloperGateInput): boolean {
  return !!u.invite_redeemed && u.developer_status === 'none'
}