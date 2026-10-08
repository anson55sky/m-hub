/**
 * 笔记内嵌图片 URL 的**两种形态**（同一套 Tauri 自定义协议在不同平台上的 URL 语法）。
 *
 * ⚠️ 单独一个文件、且**不 import 任何东西**：`noteImage.ts` 依赖后端命令，
 *   而单测要在 `node --test` 里直接加载这个纯函数（node 的类型剥离要求显式扩展名，
 *   拉进来的 `../api/tauri` 会因找不到模块而整个测试文件挂掉）。
 *   纯逻辑与有后端依赖的调用分开，是让「值不值得测」这件事没有争议的前提。
 *
 * 判据来自 Tauri / wry 的源码，不是猜的：
 * · Windows / Android —— wry 用「URL 前缀改写」把自定义协议映射到
 *   `http://<scheme>.localhost/…`（`custom_protocol_workaround::work_around_uri_prefix`），
 *   WebView2 只拦这个形态；
 * · macOS / Linux —— wry 直接给 WKWebView 注册 `WKURLSchemeHandler(handler, "mhub-note")`，
 *   只认**裸 scheme** 的 `mhub-note://…`。
 *
 * 也就是说 `http://mhub-note.localhost/<名>` 在 macOS 上是一次**真实的 HTTP 请求**
 * （打到一台并不存在的 `.localhost` 主机），图片必然裂 —— 而类型检查、后端测试、
 * 构建全绿，只看得到图裂本身。
 */
export const NOTE_IMAGE_HTTP_PREFIX = 'http://mhub-note.localhost/'
export const NOTE_IMAGE_NATIVE_PREFIX = 'mhub-note://localhost/'

/**
 * 把正文里的笔记图片 URL 归一到**当前平台可用**的形态（幂等）。
 *
 * 为什么在**读取时**归一、而不是去库里做数据迁移：正文里的地址早于这次修复，
 * 迁移要 `UPDATE notes SET content = replace(...)`，代价是一次全表重写，
 * 且万一判错条件就把用户的笔记弄坏了（约定 72：要么可回退，要么别在库里动手）。
 * 在编辑器载入时归一则是**自愈**的：笔记下次保存就写成当前平台的形态。
 *
 * ⚠️ `mac` **没有默认值**，调用点必须显式传 `IS_MAC`。给个默认值（多半是 `false`）
 *   的话，「忘了传」会静默退化成 Windows 行为 —— 而那个错在 macOS 上的症状
 *   恰好是「图还是裂」，看起来像修复没生效，很难往签名上想。
 */
export function normalizeNoteImageUrls(markdown: string, mac: boolean): string {
  if (!markdown) return markdown
  const from = mac ? NOTE_IMAGE_HTTP_PREFIX : NOTE_IMAGE_NATIVE_PREFIX
  const to = mac ? NOTE_IMAGE_NATIVE_PREFIX : NOTE_IMAGE_HTTP_PREFIX
  return markdown.includes(from) ? markdown.split(from).join(to) : markdown
}