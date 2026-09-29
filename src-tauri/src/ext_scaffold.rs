//! 本机扩展脚手架（2026-09-29 新增）
//!
//! # 补的是哪个缺口
//!
//! 「我的扩展」此前只��**接管**一个已存在的目录 —— 没有创建路径。
//! 于是第一个本机扩展必须先在别处手工搭出目录与 `manifest.json`，
//! 而 manifest 有 11 个字段、其中 `entry` / `surfaces` / `openIn` 的取值组合
//! 写错一个就得到一个「invalid」扩展。这类「先读文档再手写」的开销，
//! 正是「一等公民」的反面。
//!
//! # 三条设计约束
//!
//! ① **绝不覆盖已有目录**。用户选中的父目录下若已存在同名子目录，一律拒绝。
//!    脚手架的失败模式必须是「什么都没做」，而不是「把你手写的代码覆盖了」。
//!
//! ② **骨架必须开箱能跑**，且尽量少踩坑。`index.html` 把样式与脚本**全部内联** ——
//!    入口 URL 是 `/<id>/index.html`，外部资源的相对路径历史上反复出问题
//!    （见 tests/extensions/nested-import 里那段注释：`../assets/…` 曾被规范化成
//!    `/assets/…` 导致 404）。全内联直接绕开整类问题。
//!
//! ③ **最小权限起步**。生成的 `permissions` 是空数组，而不是塞几个看起来有用的。
//!    权限是「按需申请」的设计：骨架替你申请了权限，就等于替你做了这个决定。

use std::path::{Path, PathBuf};

/// 支持的形态。刻意**不含 service**：`runtime: "service"` 要多一个 Node 后端
/// 进程、一套 `mhub.service.*` 桥 API、以及额外的授权流程，骨架给不出能跑的
/// 最小示例 —— 给一个跑不起来的骨架比不给更糟。文档里写明怎么改。
const KINDS: &[&str] = &["module", "view"];

/// 扩展 id 的合法性 —— **直接复用发布预检的判定**。
///
/// 这里原先自己写了一份更严的规则（要求以字母/数字开头、上限 64、不许下划线），
/// 结果和 `precheck::id_ok` 当场漂移：脚手架放行 `111`（纯数字、没有点），
/// 预检却要求「小写反向域名」—— 用户能把扩展建出来，到发布那一步才被拦下。
/// 那是**脚手架在骗人**：它生成的 id 自己都不认。
///
/// 现在只有一个真源（`precheck::id_ok`），本函数只负责把「不合法」翻译成
/// 一句能照着改的提示。
pub fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("INVALID_ARGUMENT: 扩展 ID 不能为空".to_string());
    }
    if crate::precheck::id_ok(id) {
        return Ok(());
    }
    Err(format!(
        "INVALID_ARGUMENT: 扩展 ID「{id}」不合法 —— 需要是**小写反向域名**，至少含一个点，\
         例如 local.myext 或 com.example.myext（只允许小写字母、数字、点、下划线与连字符）。\n\
         为什么必须是这个形状：id 会成为入口 URL 的一段（/<id>/index.html），\
         也是资产作用域的匹配键，所以不能有空格、斜杠与大写；而反向域名是为了\
         全局唯一 —— 别人发布同名扩展时不会撞上你。"
    ))
}

/// 目录名取 id 的最后一段（`local.my-ext` → `my-ext`）。
///
/// 用 id 而不是用户填的「名称」：名称允许中文与空格，而目录名会被拼进
/// 入口 URL 与资产作用域匹配，中文/空格在那里要额外转义。
pub fn dir_name_from_id(id: &str) -> String {
    id.rsplit('.').next().filter(|s| !s.is_empty()).unwrap_or(id).to_string()
}

/// 把任意「名称」转成可用作 id 片段的 slug：非 `[a-z0-9-]` 一律丢掉。
///
/// 中文名会得到空串 —— 这是**如实**的行为，不做假音译。
/// 一段假音译出来的 id 比让用户自己填更难排查（他不知道那个 id 是什么从哪来的）。
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if c == '-' || c == '_' || c == ' ' {
            // 连续分隔符合成一个：逐字符替换会让「a  b」变成 `a--b`，
            // 目录名带双连字符虽然合法、但读起来像是笔误（实测踩到了）
            if !out.ends_with('-') {
                out.push('-');
            }
        }
        // 其它字符（中文、标点）直接丢掉，见函数注释「不做假音译」
    }
    out.trim_matches('-').to_string()
}

/// 生成骨架的 `manifest.json`。
///
/// 字段顺序与 spec §4 对齐；注释无法进 JSON，所以「各字段什么意思」
/// 放在同目录的 README.md 里（见 [`readme`]）。
pub fn manifest_json(id: &str, name: &str, kind: &str) -> String {
    // `openIn` 只列**真的有 entry 兜底**的形态。
    //
    // 原先两种形态都写 `"openIn": ["view"]`，而 module 骨架的 entry 只有
    // `{"module": …}` —— 于是扩展设置里的「打开方式」会给用户一个 view 选项，
    // 点了才报「扩展 X 没有 view 入口」。而 openIn 正是后端
    // `entry.get(surface).or_else(|| entry.get("view"))` 的输入，
    // 列一个没有 entry 的形态等于**承诺一个必然失败的操作**。
    //
    // 所以：view 形态可开成 view；module 形态只能待在工作台那一格，就不列。
    // ⚠️ 这两个变量别都叫 open_in：切片叫 `open_modes`、字符串叫 `open_in`，
    // 否则 format! 里的 `{open_in}` 会绑到那个 &[&str] 上（它不实现 Display，
    // 编译期就报）。曾在这里卡了一次。
    let open_modes: &[&str] = if kind == "view" { &["view"] } else { &[] };
    let open_in = if open_modes.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            open_modes
                .iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        r#"{{
  "id": "{id}",
  "name": {name},
  "version": "0.1.0",
  "runtime": "web",
  "kind": "{kind}",
  "surfaces": ["{kind}"],
  "openIn": {open_in},
  "entry": {{ "{kind}": "./index.html" }},
  "permissions": []
}}
"#,
        name = json_string(name),
    )
}

/// 极简 JSON 字符串转义。名称是用户输入的，直接插进 JSON 会产出**解析失败**的
/// manifest —— 而症状是「扩展 invalid，报 manifest 解析失败」，与真正的原因
/// （名字里有个引号）隔了好几层。这里只处理 JSON 必须转义的几种。
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 生成骨架的 `index.html`。
///
/// 全部内联（`<style>` + `<script>`），不引用任何外部资源 —— 见文件头约束 ②。
/// `module` 形态要放进工作台的一格，所以用 `box-sizing: border-box` + 满高，
/// 让它跟着格子高度走；`view` 形态是整页，给一个居中的容器。
pub fn index_html(id: &str, name: &str, kind: &str) -> String {
    let mount_note = if kind == "module" {
        "module 形态会放进工作台的一格，高度由宿主的格子决定，所以下面用满高 + 纵向 flex。"
    } else {
        "view 形态是整页，下面用居中容器限制阅读宽度。"
    };
    let layout = if kind == "module" {
        r#"html, body { height: 100%; }
body { display: flex; flex-direction: column; gap: 8px; padding: 2px; }"#
    } else {
        r#"body { display: grid; place-items: center; padding: 24px; }"#
    };
    let container_class = if kind == "module" { "card" } else { "page" };
    let title_size = if kind == "module" { "13px" } else { "16px" };

    format!(
        r#"<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{title}</title>
    <!--
      由 m-hub「新建本机扩展」生成。
      id: {id}    形态: {kind}

      {mount_note}

      样式与脚本**全部内联**，不引用外部资源：扩展入口 URL 是
      `/<id>/index.html`，相对路径的资源引用历史上反复出问题
      （`../assets/x.js` 会被浏览器规范化成 `/assets/x.js` 而 404）。
      需要拆文件时，把引用写成相对**当前目录**的 `./xxx`，并留意这一点。
    -->
    <style>
      *, *::before, *::after {{ box-sizing: border-box; }}
      html, body {{ margin: 0; }}
      body {{
        font: 13px/1.6 system-ui, -apple-system, "PingFang SC", sans-serif;
        color: #1f2430;
        background: transparent;
        {layout}
      }}
      .card, .page {{
        display: flex;
        flex-direction: column;
        gap: 6px;
        {container_style}
      }}
      h1 {{ font-size: {title_size}; margin: 0; font-weight: 600; }}
      .id {{ font-size: 11px; opacity: .55; font-family: ui-monospace, Menlo, monospace; }}
      button {{
        align-self: flex-start;
        padding: 4px 10px;
        border: 1px solid rgba(0, 0, 0, .15);
        border-radius: 6px;
        background: transparent;
        color: inherit;
        font: inherit;
        cursor: pointer;
      }}
      button:active {{ transform: translateY(1px); }}
      code {{ font-family: ui-monospace, Menlo, monospace; font-size: 11px; }}
    </style>
  </head>
  <body>
    <div class="{container_class}">
      <h1>{title}</h1>
      <p class="id">{id}</p>
      <!--
        桥 API：宿主注入 window.mhub。
        这里演示 runtime.info —— 它不需要任何权限，是验证「宿主连上了」
        最省事的一步（否则你得先申请权限才能确认扩展本身是通的）。
      -->
      <p>宿主桥：<code id="probe">连接中…</code></p>
      <button type="button" id="ping">ping</button>
      <p id="out"></p>
    </div>
    <script>
      // ⚠️ 桥 API 是**异步**的：window.mhub 由宿主在文档加载后注入。
      // 直接在脚本顶层同步读会拿到 undefined —— 这是新手最常踩的坑，所以这里
      // 轮询等它出现，并给出明确提示而不是静默失败。
      const out = document.getElementById('out')
      const probe = document.getElementById('probe')

      function whenBridge(cb, timeoutMs = 3000) {{
        const t0 = Date.now()
        const tick = () => {{
          if (window.mhub) return cb(window.mhub)
          if (Date.now() - t0 > timeoutMs) {{
            probe.textContent = '没等到宿主桥（window.mhub）'
            return
          }}
          setTimeout(tick, 50)
        }}
        tick()
      }}

      whenBridge(async (mhub) => {{
        probe.textContent = '已连上'
        const info = await mhub.runtime.info()
        probe.textContent = '已连上 · ' + JSON.stringify(info)
      }})

      document.getElementById('ping').addEventListener('click', () => {{
        whenBridge(async (mhub) => {{
          const t = Date.now()
          const r = await mhub.runtime.info()
          out.textContent = 'ping 用时 ' + (Date.now() - t) + 'ms → ' + JSON.stringify(r)
        }})
      }})
    </script>
  </body>
</html>
"#,
        title = html_escape(name),
        container_style = if kind == "module" { "flex: 1 1 auto; min-height: 0;" } else { "max-width: 460px;" },
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 同目录的 README：把「各字段什么意思」「怎么改成 service」写成看得懂的短文。
///
/// 单��一个注释块，而不是多份散落在代码里的文档 —— 骨架是给别人看的，
/// 而「字段含义」这件事真正的读者是三天后的自己。
pub fn readme(id: &str, name: &str, kind: &str, dir: &str) -> String {
    format!(
        r#"# {name}

由 m-hub「新建本机扩展」生成（{kind} 形态）。

- 扩展 id：`{id}`
- 目录：`{dir}`
- 改动保存后**自动热重载**（宿主每 1.5 秒比对一次源码内容戳），不用重启。

## 文件

| 文件 | 作用 |
| --- | --- |
| `manifest.json` | 扩展的声明：id、名字、版本、形态、入口、要申请的权限 |
| `index.html` | 入口。样式与脚本**全部内联**（见文件内注释，外部相对路径有坑） |

## manifest 各字段

| 字段 | 含义 |
| --- | --- |
| `id` | 唯一标识，反向域名风格（`local.my-ext`）。**改它等于换一个扩展**，因为它同时是入口 URL 的一段 |
| `name` | 界面上的显示名，可随意改（含中文） |
| `version` | 版本号。热重载不看它，只有发布到市场时才用 |
| `runtime` | `web`（纯前端）或 `service`（带 Node 后端）。本骨架是 `web` |
| `kind` | 默认形态。`module` = 工作台卡片；`view` = 独立页面 |
| `surfaces` | 声明支持哪些形态：`module` / `view` / `window` / `drawer` |
| `openIn` | 允许的打开方式：`view`（在主区打开）/ `window`（独立窗口） |
| `entry` | 形态 → 入口文件的相对路径。键要与 `surfaces` 里的形态对应 |
| `permissions` | 要申请的能力，**按需申请**，不给就不给。本骨架是空的 |

## 桥 API

宿主注入 `window.mhub`，全是**异步**方法 —— 直接在脚本顶层同步读会拿到 `undefined`。
`index.html` 里的 `whenBridge()` 就是处理这件事的。

不需要权限就能用的：`runtime.info()`（确认宿主连上了）。

## 想加能力（权限）

往 `manifest.json` 的 `permissions` 里加，例如 `data.notes`、`clipboard.read`。
加完在 m-hub 的扩展设置里授权。**权限是按需申请的**，所以只在真要用时才加。

## 想改成 service 形态（带 Node 后端）

1. `manifest.json` 里把 `runtime` 改成 `service`，并给 `entry` 加 `service` 入口
2. 后端脚本要用 CommonJS 或 ESM，由宿主以子进程拉起
3. 前端通过 `mhub.service.*` 调用它；后端启动失败时前端仍能打开，
   但 `runtime.info()` 会返回 `serviceReady: false`
"#
    )
}

/// 脚手架的结果。
#[derive(Debug)]
pub struct Scaffold {
    pub dir: PathBuf,
    pub files: Vec<String>,
}

/// 把 `~` / `~/x` 展开成真实的 home 路径。
///
/// 放这里而不是前端：前端没有可靠的 home（`~` 不是路径，`~` 在 Tauri 的
/// 文件对话框里是字面量），而这是**所有**调用方都要用的一条规则 ——
/// 写在后端一处，比让每个前端入口各自实现一遍更不容易漂移。
pub fn expand_tilde(p: &str) -> std::path::PathBuf {
    if p == "~" || p.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
            let mut out = PathBuf::from(home);
            if let Some(rest) = p.strip_prefix("~/") {
                out.push(rest);
            }
            return out;
        }
    }
    PathBuf::from(p)
}

/// 在 `parent` 下创建一个骨架扩展目录。
///
/// **拒绝覆盖**：目标目录已存在就报错返回，一个字节都不写。
/// 这是脚手架唯一不可让步的一条 —— 失败必须是「什么都没做」。
pub fn create(
    parent: &Path,
    id: &str,
    name: &str,
    kind: &str,
) -> Result<Scaffold, String> {
    validate_id(id)?;
    if !KINDS.contains(&kind) {
        return Err(format!(
            "INVALID_ARGUMENT: 形态「{kind}」不支持（可选：{}）",
            KINDS.join(" / ")
        ));
    }
    if name.trim().is_empty() {
        return Err("INVALID_ARGUMENT: 名称不能为空".to_string());
    }

    let dir = parent.join(dir_name_from_id(id));
    // 先查存在性再动手：目录存在就**直接返回**，不 create_dir_all、不写任何文件。
    if dir.exists() {
        return Err(format!(
            "ALREADY_EXISTS: 目录已存在，未改动任何文件：{}",
            dir.display()
        ));
    }
    // 父目录不存在就建（用户可能选了还没建的路径）
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("IO_ERROR: 创建父目录失败：{e}"))?;

    std::fs::create_dir(&dir).map_err(|e| format!("IO_ERROR: 创建扩展目录失败：{e}"))?;

    let dir_str = dir.to_string_lossy().into_owned();
    let mut files = Vec::new();
    for (name_on_disk, content) in [
        ("manifest.json", manifest_json(id, name.trim(), kind)),
        ("index.html", index_html(id, name.trim(), kind)),
        ("README.md", readme(id, name.trim(), kind, &dir_str)),
    ] {
        let p = dir.join(name_on_disk);
        if let Err(e) = std::fs::write(&p, content) {
            // 写失败就把半成品目录删掉：留一个只有 manifest 的目录比不留更糟，
            // 因为它会被扫成一个 invalid 扩展，出现在「我的扩展」列表里占位。
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!("IO_ERROR: 写入 {} 失败：{e}", p.display()));
        }
        files.push(name_on_disk.to_string());
    }

    Ok(Scaffold {
        dir,
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /**
     * 一个**保证独占**的临时目录。
     *
     * ⚠️ 这里原先只拿 `SystemTime::now().as_nanos()` 当目录名，看起来唯一，
     * 实际不是：Rust 测试默认**多线程并行**，而系统时钟的分辨率并不保证
     * 纳秒级 —— 两个线程在同一 tick 内取时间会拿到**相同的值**，
     * 于是共用一个目录，其中一个测试 `remove_dir_all` 把另一个的脚手架删掉。
     *
     * 症状极具迷惑性：变异测试里「给入口页加一行外部脚本」这种明显无关的变异，
     * 报出来的失败测试名每次都不一样，重跑又好了 —— 看起来像是守卫在乱报，
     * 其实是我的测试自己在打架。**flaky 测试比没有测试更糟**：它会训练人忽略失败。
     *
     * 修法：用 `create_dir`（不是 `create_dir_all`）原子地「创建或失败」，
     * 失败就加后缀重试。`create_dir` 在目录已存在时返回错误，这正是要的语义 ——
     * 没有「先查再建」的 TOCTOU 窗口。
     */
    fn tmp() -> PathBuf {
        let base = std::env::temp_dir();
        for n in 0..10_000u32 {
            let d = base.join(format!("mhub_scaffold_{}_{}", std::process::id(), n));
            if std::fs::create_dir(&d).is_ok() {
                return d;
            }
        }
        panic!("连续 10000 次都建不出临时目录");
    }

    /// 把一份真实骨架写到 `target/scaffold-sample/`，供人眼检查。
    ///
    /// `#[ignore]`：不进常规测试跑（它有副作用、要写盘），只在需要时手动跑：
    ///     cargo test --lib ext_scaffold -- --ignored --nocapture
    /// 存在的理由是「别用复述代替实物」—— 描述骨架长什么样很容易，写错也不报错；
    /// 而骨架是要被复制几十次的模板，看一眼真实产物比读注释可靠。
    #[test]
    #[ignore]
    fn dump_sample_scaffold() {
        let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/scaffold-sample");
        let _ = std::fs::remove_dir_all(&out);
        let s = create(&out, "local.pomodoro", "番茄钟", "module").unwrap();
        println!("已生成：{}", s.dir.display());
        for f in &s.files {
            println!("  - {f}");
        }
    }

    #[test]
    fn id_rules() {
        // 通过：必须与发布预检同口径（反向域名、至少一个点）
        assert!(validate_id("local.myext").is_ok());
        assert!(validate_id("com.example.myext").is_ok());
        assert!(validate_id("local.my-ext").is_ok());
        // 「下划线」预检允许，所以这里也必须允许 —— 旧版脚手架拒绝它，
        // 而预检放行，同样是漂移的一个方向
        assert!(validate_id("local.my_ext").is_ok(), "预检允许下划线，脚手架不该比它更严");

        // 拒绝：这些预检都不认
        assert!(validate_id("").is_err(), "空 id 要拒绝");
        assert!(validate_id("111").is_err(), "纯数字没有点，不是反向域名 —— 用户实测踩过");
        assert!(validate_id("Local.ext").is_err(), "大写要拒绝（URL 与作用域都吃不下）");
        assert!(validate_id("local/ext").is_err(), "斜杠要拒绝（会拼坏入口 URL）");
        assert!(validate_id(".local.ext").is_err(), "不能以点开头（会落到隐藏目录）");
        assert!(validate_id("local..ext").is_err(), "不能有连续的点");
        assert!(validate_id(&"a".repeat(129)).is_err(), "过长要拒绝");
    }

    /// 脚手架放行的 id，**必须**发布预检也放行。
    ///
    /// 这是上面那个 bug 的通用防线。原来的 `id_rules` 只测了脚手架**自己**
    /// 认可的几个例子，而「脚手架的规则」与「预检的规则」是两份独立实现 ——
    /// 两份都自洽，合起来却不一致（脚手架放 `111`、预检不认）。
    /// 单侧测试永远发现不了这种漂移，必须**拿真源来对**。
    #[test]
    fn every_accepted_id_also_passes_publish_precheck() {
        let candidates = [
            "local.myext", "com.example.myext", "local.my-ext", "local.my_ext",
            "a.b", "local.1", "111", "", "Local.ext", "local/ext", ".local.x",
            "local..x", "local.", "local ext", "local.mé", &"a".repeat(129),
            "local.x", "com.m-hub.tool",
        ];
        for id in candidates {
            let scaffold_ok = validate_id(id).is_ok();
            let precheck_ok = crate::precheck::id_ok(id);
            assert_eq!(
                scaffold_ok, precheck_ok,
                "id「{id}」两处判定不一致：脚手架={scaffold_ok}、预检={precheck_ok}。\
                 脚手架在骗人 —— 它放行的 id 自己都不认。"
            );
        }
    }

    #[test]
    fn slugify_is_honest_about_chinese() {
        assert_eq!(slugify("My Ext"), "my-ext");
        assert_eq!(slugify("计时器"), "", "中文不做假音译，如实返回空串");
        assert_eq!(slugify("  a  b  "), "a-b");
    }

    #[test]
    fn dir_name_comes_from_id_last_segment() {
        assert_eq!(dir_name_from_id("local.my-ext"), "my-ext");
        assert_eq!(dir_name_from_id("local"), "local");
    }

    /// `openIn` 里的每个形态都必须有对应的 `entry` 键。
    ///
    /// 这条是上面那个 bug 的一般化：后端按 `entry.get(surface).or_else(entry.get("view"))`
    /// 解析入口，所以 `openIn` 列一个没有 entry 的形态 = 承诺一个必然失败的打开操作。
    /// 症状是「设置里能选、点了报错」，隔了好几层才看得出是 manifest 的问题。
    #[test]
    fn open_in_never_promises_an_unbacked_surface() {
        for kind in KINDS {
            let m: serde_json::Value =
                serde_json::from_str(&manifest_json("local.x", "名", kind)).unwrap();
            let entry = m["entry"].as_object().expect("entry 必须是对象");
            let open_in = m["openIn"].as_array().expect("openIn 必须是数组");
            for v in open_in {
                let surface = v.as_str().unwrap();
                assert!(
                    entry.contains_key(surface),
                    "{kind} 骨架的 openIn 列了「{surface}」，但 entry 里没有这个键 —— \
                     用户会在设置里选到它、点下去才报「没有 {surface} 入口」。entry 现有：{:?}",
                    entry.keys().collect::<Vec<_>>()
                );
            }
            // 反向也成立：surfaces 里的每个形态都该有 entry
            for v in m["surfaces"].as_array().unwrap() {
                let surface = v.as_str().unwrap();
                assert!(entry.contains_key(surface), "surfaces 的「{surface}」缺 entry");
            }
        }
    }

    /// 生成的 manifest 必须能被**真实解析器**读进去 —— 用 serde_json 而不是字符串比较。
    /// 字符串比较只能证明「长得像」，解析才能证明「真能用」。
    #[test]
    fn generated_manifest_is_parseable_and_complete() {
        for kind in KINDS {
            let text = manifest_json("local.x", "测试 \"引号\" & 符号", kind);
            let m: serde_json::Value = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("生成的 manifest 无法解析（{kind}）：{e}\n{text}"));
            assert_eq!(m["id"], "local.x");
            assert_eq!(m["name"], "测试 \"引号\" & 符号", "名称里的引号必须被转义");
            assert_eq!(m["kind"], *kind);
            assert_eq!(m["surfaces"][0], *kind);
            assert_eq!(m["entry"][*kind], "./index.html", "entry 的键要匹配 kind");
            assert!(m["permissions"].as_array().unwrap().is_empty(), "起步必须零权限");
            assert_eq!(m["runtime"], "web");
        }
    }

    /// 用**宿主自己的**解析器验证 —— 骨架的价值就在于「生成的能被加载」。
    #[test]
    fn generated_manifest_loads_through_the_real_reader() {
        let dir = tmp();
        let s = create(&dir, "local.probe", "探针", "module").unwrap();
        let m = crate::extension::read_manifest(&s.dir).expect("宿主解析器必须能读骨架 manifest");
        assert_eq!(m.id, "local.probe");
        assert_eq!(m.name, "探针");
        assert_eq!(m.kind, "module");
        assert!(m.permissions.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 入口页**不得加载任何外部资源**。
    ///
    /// 这是文件头约束 ② 的可执行版本。扩展入口 URL 是 `/<id>/index.html`，
    /// 而相对路径的资源引用在这一套协议下反复出过问题：
    /// 浏览器会把 `../assets/x.js` 规范化成 `/assets/x.js`（丢掉 `<id>` 段）而 404，
    /// tests/extensions/nested-import 里那段注释记的就是这件事。
    ///
    /// 为什么值得守：骨架是给人当起点的，日后「把内联样式拆成 style.css、
    /// 脚本拆成 app.js」是很自然的一次重构 —— 而那正好会把 404 请回来。
    /// 骨架是复制得最多的代码，一份会 404 的模板会被复制到几十个扩展里。
    ///
    /// ⚠️ 检查的是「一切资源加载构造」，不只是 `<script src>` 标签。
    /// 第一版只查标签，结果漏了一种：脚手架里用
    /// `document.createElement('script')` + `.src = '../assets/app.js'`
    /// **动态**挂脚本 —— 文本里根本没有 `<script src=` 这几个字，纯标签扫描
    /// 完全看不见（这是变异测试实测出来的，不是想出来的）。
    #[test]
    fn entry_page_loads_no_external_resources() {
        // (说明, 关键片段) —— 覆盖标签与动态两种写法
        const BANNED: &[(&str, &str)] = &[
            ("script 标签的 src", "<script src="),
            ("link 标签", "<link "),
            ("样式表引用", "stylesheet"),
            ("动态建 script 元素", "createElement('script')"),
            ("动态建 script 元素（双引号）", "createElement(\"script\")"),
            ("赋 .src", ".src ="),
            ("动态 import()", "import("),
            ("fetch", "fetch("),
            ("Worker", "new Worker("),
        ];
        for kind in KINDS {
            let html = index_html("local.x", "示例", kind);
            for (what, needle) in BANNED {
                assert!(
                    !html.contains(needle),
                    "{kind} 入口页不该有{what}（`{needle}`）—— 相对路径在扩展协议下会 404，\
                     骨架必须全部内联。命中行：\n{}",
                    html.lines()
                        .filter(|l| l.contains(needle))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
        }
    }

    #[test]
    fn creates_three_files() {
        let dir = tmp();
        let s = create(&dir, "local.demo", "示例", "view").unwrap();
        assert_eq!(s.files.len(), 3);
        for f in ["manifest.json", "index.html", "README.md"] {
            assert!(s.dir.join(f).is_file(), "缺 {f}");
        }
        // 查**文件内容**而不是路径 —— 目录名取自 id 的最后一段（这里是 `demo`），
        // 路径里压根不会出现形态名。原断言查路径，是个恒假的假测试。
        let text = std::fs::read_to_string(s.dir.join("manifest.json")).unwrap();
        assert!(text.contains("\"kind\": \"view\""), "manifest 里应写着 view：{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 拒绝覆盖，且**一个字节都不写**。
    ///
    /// 这条是脚手架最不可让步的约束：用户可能已经在那儿写了半天代码，
    /// 脚手架把它覆盖掉是不可接受的。断言里特意检查目录内容没变。
    #[test]
    fn refuses_to_overwrite_existing_directory() {
        let parent = tmp();
        let target = parent.join("demo");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("我的代码.txt"), "别删我").unwrap();

        let err = create(&parent, "local.demo", "示例", "view").unwrap_err();
        assert!(err.contains("ALREADY_EXISTS"), "错误码要能区分：{err}");
        assert!(target.join("我的代码.txt").is_file(), "原有文件必须还在");
        assert!(!target.join("manifest.json").exists(), "不该写进任何骨架文件");

        // 再验证一次：反复调用也不会有任何副作用
        assert!(create(&parent, "local.demo", "另一个名字", "module").is_err());
        assert!(!target.join("index.html").exists());

        let _ = std::fs::remove_dir_all(&parent);
    }

    #[test]
    fn rejects_unsupported_kind() {
        let dir = tmp();
        let err = create(&dir, "local.x", "名", "service").unwrap_err();
        assert!(err.contains("service 形态") || err.contains("不支持"), "{err}");
        assert!(!dir.join("x").exists(), "被拒时不该留下目录");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_bad_id_before_touching_disk() {
        let dir = tmp();
        assert!(create(&dir, "Bad/Id", "名", "view").is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "校验失败不该建任何目录");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
