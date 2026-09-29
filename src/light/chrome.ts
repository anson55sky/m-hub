// 速达「应用内浏览器」chrome 顶栏的轻量入口（内存优化 P1）。
// 完整 SPA 的「活堆」（Vue 应用实例 + 全组件树 + store 拷贝）是隐藏窗口 renderer
// 的内存大头（~100MB，MemoryUsageTargetLevel=Low 只能吐缓存吐不掉活堆）；
// chrome 页只需要顶栏一条，换轻量入口后隐藏态降到 ~20-30MB。
// 只挂 BrowserChrome + 主题自举；splash / 滚动条 hover 联动 / UpdateCheckDialog
// 等主窗启动环境都不需要。
import { createApp } from 'vue'
import '../style.css'
import BrowserChrome from '../components/BrowserChrome.vue'
import { applyTheme } from '../composables/useTheme'
import { isTauri, tauriApi } from '../api/tauri'

// BrowserChrome 内部的 useTheme 只能 watch 到本窗口 store 的默认值（轻量入口
// 不加载全量配置），这里用 getThemeConfig 补一次真实主题（悬浮球同款自举口径）。
// 时序：mount 先以默认主题渲染（窗口隐藏态无感），本 Promise 随后覆盖为真实主题。
if (isTauri()) {
  tauriApi
    .getThemeConfig()
    .then((t) => applyTheme({ mode: t.mode, preset: t.preset, accent: t.accent }))
    .catch(() => {})
}

createApp(BrowserChrome).mount('#app')
