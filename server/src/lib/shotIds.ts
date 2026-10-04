// `submissions.shots_json` 的解析：它存的是「资产行 id 数组」的 JSON 文本。
//
// ## 为什么单独一个文件
//
// 两处要读它：`routes/submissions.ts`（继承截图时读来源那一版）与
// `lib/marketSign.ts`（重建清单时把 id 拼成公开 URL）。若定义放在
// `routes/submissions.ts`，`lib/` 就得反向依赖 `routes/` —— 依赖方向倒了，
// 下一个人加一条 `routes → lib` 的引用就会成环。
//
// ## 解析失败一律当「没有截图」，不抛错
//
// 这一列是**我们自己**写进去的，正常不会坏。但它坏时的正确表现是
// 「那张图不显示」，而不是「整个市场清单 500」—— 清单是全站功能，
// 为了一个字段的脏数据把整站打挂，代价与收益完全不成比例。
// 故这里宁可少图、不给错图。

export function parseShotIds(raw: string | null): number[] {
  if (!raw) return []
  try {
    const v: unknown = JSON.parse(raw)
    if (!Array.isArray(v)) return []
    // 必须是**正整数**：0 / 负数 / 字符串 / 对象 / null 一律剔除。
    // ⚠️ 不能只判 typeof number —— `[1.5]` 与 `[1e999]` 也是 number，
    //   前者会拼出 `/shot/1.5` 这种根本不存在的路由，后者会让比较全错。
    return v.filter((x): x is number => Number.isInteger(x) && x > 0 && x <= Number.MAX_SAFE_INTEGER)
  } catch {
    return []
  }
}