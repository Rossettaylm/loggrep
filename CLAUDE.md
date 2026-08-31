# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

alnav（App/Android Log Navigator）— 轻量级 Android logcat 日志过滤与分析工具（Rust）。Cargo workspace，两个 member crate：

- **`alnav-core`** — 解析/过滤/格式化库（`[lib] name = "alnav"`，`use alnav::...`）。提供 `run_cli`；不发布独立 bin。
- **`alnav`** — 统一发布包：默认 `alnav` 进 TUI；`alnav grep` 走 CLI。兼容别名（过渡一版，下版移除）：`aloggrep`/`alg` ≡ `alnav grep`，`aloggrep-tui` ≡ `alnav`。支持 `-f` 静态文件与 `--hdc` / `--adb` 实时流。配置硬切 `$ALNAV_HOME` / `~/.config/alnav`。

## Build & Test

```bash
cargo build --workspace                       # 构建全部 workspace 成员
cargo build -p alnav-core                     # 仅构建 core lib
cargo build -p alnav                          # 构建统一二进制（TUI + CLI + 兼容别名）
cargo test --workspace                        # 运行全部测试
cargo test -p alnav-core --test parser        # 仅运行 parser 集成测试
cargo test -p alnav --bin alnav app::         # 仅运行 alnav 的 app 模块测试
cargo run -p alnav --bin alnav -- grep -f app.log --tag "MyApp"  # 运行 CLI
cargo run -p alnav --bin alnav -- -f app.log  # 运行 TUI（需要真实 TTY）
cargo run -p alnav --bin alnav -- --hdc       # TUI HarmonyOS 实时抓取
cargo run -p alnav --bin alnav -- --adb       # TUI Android 实时抓取
```

测试分布（`alnav-core`，位于 `alnav-core/`）：
- `tests/*.rs`（12 个文件；CLI 集成测试在 `alnav/tests/`）— 集成测试，通过 pub API 测试，占绝大多数
- `src/histogram.rs` — 3 个单元测试（访问私有 `snap_secs()` 和 `buckets` 字段）
- `src/cli_run.rs` — 6 个单元测试（`run_follow()` 与 live 源校验）
- `src/hdc.rs` / `src/adb.rs` / `src/live.rs` — 后端命令与启动历史过滤测试
- `src/expr.rs` — `Expr::from_filters` 相关测试

测试分布（`alnav`，位于 `alnav/`）：全部为源文件内 `#[cfg(test)]` 单元测试，覆盖 `model`/`filter_model`/`highlight_model`/`ingest`/`app`/`ui`/`input`/`main`（`dispatch_tests`）各模块。

## Architecture

```
alnav-core/src/
├── cli_run.rs     # `run_cli`：输入调度（stdin/文件/live），主循环
├── clearkey.rs    # live 模式 Ctrl-L 清屏：cbreak 模式读键盘 + KeypressGate 包装行迭代器
├── live.rs        # 后端无关 LiveFilter/LiveSession：启动时间过滤与子进程行会话
├── adb.rs         # --adb 设备时间查询 + `adb logcat -v threadtime`
├── hdc.rs         # --hdc 设备时间查询 + `hdc hilog --no-block`；保留 Hdc* 兼容别名
├── parser.rs      # LogEntry 结构体 + 解析器（支持 hilog/threadtime/xlog/brief 四种格式）
├── filter.rs      # FilterChain：多条件组合过滤（同类 OR，跨类 AND），支持 pid/tid
├── expr.rs        # -e 布尔表达式：tokenizer + 递归下降 parser + AST evaluator；Expr::from_filters 将 CLI 标量过滤条件编译为 AST，供 TUI 复用
├── multiline.rs   # MultilineMerger：多行合并迭代器适配器（合并续行如栈追踪）
├── crash.rs       # CrashDetector：崩溃识别 + CrashInfo 结构化提取
├── dedupe.rs      # Normalizer（消息归一化）+ Deduper（去重分组）
├── sampler.rs     # Sampler：输出采样（--tail 尾部 / --sample 均匀抽样）
├── histogram.rs   # Histogram：时间窗口聚合（--histogram 10s/1m/5m），JSON 输出
├── formatter.rs   # 输出格式化：text（彩色）/ json / csv，支持 --fields 字段选择
├── logcolor.rs    # 颜色语义数据（RGB 常量 + Badge 映射），不依赖 colored/ratatui，供 CLI formatter.rs 使用（TUI 不读）
└── summary.rs     # 聚合统计：级别分布、Top tags、Top errors、崩溃计数

alnav/src/
├── main.rs         # argv0 分发（alnav / alnav grep / 兼容别名）+ TUI 事件循环
├── model.rs        # EntryRow：拥有所有权的行模型，from_line() 解析，as_log_entry() 借出给 core 匹配逻辑复用
├── filter_model.rs # Group（chips + AND Expr + enabled；same_as 去重）+ GroupList（组间 OR；全禁用≡空列表≡全可见）+ TimeBound（全局时间窗匹配）
├── time_panel.rs   # `-f` 全局时间窗面板（`ts`）：日期候选自 `rows` + HH:MM:SS 键入/夹紧
├── highlight_model.rs # HighlightGroup（单 pattern）/HighlightGroupList（组间高亮 OR）+ Highlight compose 逻辑
├── input.rs        # ChipField/Chip/InputBox（Enter 两段式：收 pill / 提交组）/Popup + build_group（复用 Expr::from_filters）
├── app.rs          # App 状态机：rows/matched/visible/groups/highlight_groups/time_bound/Focus/following/pending_leader/picker
├── keymap.rs       # ActionId / ActionMeta / KeymapStore（绑定 + keymap.toml / `--init`）
├── action.rs       # ActionStore：`when` / catalog / `dispatch`（按键与命令面板共用）
├── command_palette.rs # `:` 命令面板会话（TextField query；不复用 Picker）
├── picker.rs       # fzf PickerSession：ActionList（cm 后置 Filter|Highlight）/Manage/New/Edit/删除确认
├── ui.rs           # 渲染：log/strip + fzf 左右面板/确认框/Preview/Time 面板 + status_bar
├── help.rs         # H6 键位：两级 Help（Home Active≤4 + 七区页面）+ `/` 子串搜索；Esc/`?` 关面板不 resume；`h`/Backspace 回 Home；status 空闲仅 1–2 键；flash 为中间填充 pill（3s）
├── export.rs       # H10：当前 Filter/Exclude/lock/time_bound → 一行 `alnav grep` CLI（`yc`）
├── bookmark.rs     # 对照盘：快照钉（上限 16）；ma 开关 / md 删；mm 大面板；顶栏一行摘要
├── config.rs       # 配置目录解析 + config.toml（含 theme=）+ theme.toml overlay（坏文件回退）
├── palette.rs      # Palette、name fold、mix、contrast_fg
├── theme_builtins.rs # 九套内置 Palette 常量
├── theme.rs        # UI 颜色映射唯一入口：Palette → UiTokens + style fns（theme.toml overlay）；TUI 日志色来自 UiTokens
├── store.rs         # RowStore：FileStore（mmap+行索引+惰性解析）/ StreamStore（ADB/HDC VecDeque）/ RowRef
└── ingest.rs        # spawn_live_ingest（ADB/HDC 共用 DropOldestRing）；spawn_file_ingest 仅供测试
```

**CLI 数据流：** stdin/file → 逐行读取 → [MultilineMerger] → `LogEntry::parse()` → `FilterChain::matches()` → [CrashDetector] → `Formatter::write_entry()` / `Summary::record()`

**TUI 数据流：**
- **`-f`：** `FileStore::open` → mmap + 后台行索引 → `Visible::All { len }`；filter 时后台扫描 → `Visible::Subset(命中行号)`；渲染经 `App::row_at` 惰性解析（无全文件 owned `EntryRow`，无 `max_lines` 淘汰）。
- **`--hdc` / `--adb`：** 后台线程 → `EntryRow::from_line()` → `DropOldestRing` → `App::drain()` → `StreamStore`；filter 未激活时 `Visible::All` 索引 `rows`；激活时命中双写 `matched`、`Visible::All` 索引 `matched`。

### Key Design Decisions

- **`LogEntry<'a>` 零拷贝解析**：所有字段（timestamp, pid, tid, tag, pkg, msg）均为 `&'a str`，直接引用原始行，避免堆分配。`parse()` 依次尝试 hilog → threadtime → xlog → brief 四种格式。
- **`FilterChain::from_cli(&Cli)`** 是 CLI 唯一的过滤器构建入口，将 CLI 参数（tag/msg/level/pid/tid/since/until/-e）统一转换为内部过滤链。TUI 不用 `FilterChain`，改用更轻量的 `Expr::from_filters` + `Group`/`GroupList`（见下）。
- **main.rs `dispatch_lines!` 宏**：根据 `--multiline`/`--crashes` 标志决定是否用 `MultilineMerger` 包裹迭代器，避免运行时分支开销。ADB/HDC 创建 `LiveSession` 后复用同一处理路径。
- **输出路径分支**：`run_simple`（常规快速路径）vs `run_with_context`（-C/-A/-B 上下文行缓冲）vs `run_time_context`（--time-context 两遍扫描）vs `run_follow`（--follow-pid/tid 两遍扫描）。
- **live Ctrl-L 清屏（CLI）**：`--hdc` / `--adb` 仅在 stdin/stdout 都是 tty 时启用；用 cbreak 模式（保留 `ISIG`）而非标准 raw mode，避免破坏现有 Ctrl+C 依赖的 `SIGINT` 语义。按键上报走 channel + `KeypressGate` 迭代器分发。仅支持 Unix，Windows 上静默不可用。已知权衡：若进程被 `SIGTERM`/`SIGKILL` 直接杀死（而非 Ctrl+C），termios 不会被恢复，终端会卡在 cbreak 模式，需手动 `stty sane`/`reset`——这与 vim/less 等直接操作终端的工具在被强杀时的行为一致，未特殊处理。
- **`alnav` 的 chip 过滤模型**：`Vec<Group>`，`Group` 内 chip 之间 AND（内部编译为一个 `Expr`），`Vec<Group>` 之间 OR。Input：`Space` 进草稿（可含空格）；有草稿时 `Enter` 收成 pill；无草稿且已有 pill 时 `Enter` 提交组。提交前按 chip 多重集（ignore-case）去重，重复则不 push。启动 CLI 过滤转为第 0 组（可 `dd`/`di`）。chip 编译走 `Expr::from_filters(..., SameFieldOp::And)`；启动 `initial_group` 仍用 `SameFieldOp::Or`。**TUI 过滤/搜索一律 ignore-case**。LogList 另有 **H7 光标→Chip**：operator `c`+字段字母（`t/m/g/p/T/l`，与 `YankField` 对齐）从当前行推单 chip 组；`c`+`m` 开 msg 切词候选（无条数上限）→ 选中后 ActionList 选 Filter（默认）或 Highlight；`C`+`m` 切词后直接 Exclude；`y`+`m` 同壳切词后 yank 选中片段（`Y` 仍整段 msg）；成功后 `following=false`，Esc 只清 pending/关面板不 resume。**H8 会话 lock**：`App.lock_pid`/`App.lock_tid` 互斥，在 chip 过滤后 AND；operator `f`+`p`/`t`/`u`（toggle 同值清除）；status：follow 常驻（on=success pill / off=DIM glyph）、device 常驻 source/disconnect pill；lock/time 为填充 pill+短值（无 FOLLOWING/LOCK/TIME 单词）；Esc resume 不清除 lock。**全局时间窗（仅 `-f`）**：`App.time_bound: Option<TimeBound>` 与 Filter 组正交，在 chip/exclude/lock 之后 AND；启动 `--since`/`--until` 写入全局窗（不再挂 Group）；operator `t`+`s` 开靠上 Time 面板（日期候选自 `rows` 去重、只能选自候选；时间 `HH:MM:SS` 键入并夹到该日缓冲 min/max，保证 since≤until；允许只设一端，端内须日+时成对），`t`+`u` 清除；无日期候选时 `ts` flash 拒绝；`--hdc` / `--adb` 硬隐藏 `t`/`ts`/`tu`；status 时间窗图标+短值；计入 `filter_active`；`yc` 导出 `--since`/`--until`；打开/提交/`tu` → `following=false`，面板 Esc 不 resume。
- **`alnav` 的统一 fzf Picker**：Normal `Space` 进入 Leader；`Space Space` 打开 Unified Manage（Filter / Highlight / Exclude）；裸键 New：`;`→Filter、`/`→Highlight、`` ` ``→Exclude；`mm`→Bookmark Manage；`?`→只读两级 Help（Home+七区页面，`/` 搜索，`h`/Backspace 回 Home，Esc 关面板；非 Highlight New）；`Space f/s/m/x` 历史别名已收敛。Picker 为左右 4:6（可由 `config.toml` 的 `picker_left_ratio` 调整），左侧候选+底部检索（mode 前缀图标：Manage 搜索镜 / New `＋` / Edit `✎`），右侧 Preview；Manage 下键入无匹配时自动切 New（query→draft；清空草稿回退 Manage）；手动进 New 不清空也不回退；Esc / 提交成功 Enter 一律关闭面板（不回 Manage）；Manage 内 Ctrl-X 编辑、Delete/Ctrl-Backspace 删除选中（二次确认）、Ctrl-T 一键全开/全关（有 Tab 勾选则只作用勾选项，否则作用 query 可见项；范围内有任一启用则全禁用，全禁用则全启用）；草稿行支持中间光标与 ←/→/Home/End/Ctrl-A/E/Ctrl-U（New/Edit 另有 Ctrl-Backspace 删词）。**Filter/Highlight/Exclude/Bookmark 统一**：候选为空时打开即 New；有候选且未强制 New 则 Manage；msg-chip 也复用 Picker 壳。
- **`alnav` 的命令面板（`C-p`）**：LogList / Filter·Exclude·Highlight strip 上 `C-p` 打开顶居中面板（不是 Picker）。`action::dispatch` 是动作执行唯一入口（`match`，与按键共用）；KeymapStore 只管绑定。空查询只有输入条；输入后向下最多 10 行，nucleo 搜 `palette_title`；`j`/`k` 打字、Up/Down 移动。Esc/Ctrl+C 关面板不 resume following。不可用命令直接隐藏。Idle 状态栏不加第三条 `C-p` 提示（仍 `? help` + `; filter`）。
- **`alnav` 的 RowStore**：`App.store` 为 `File`（`-f` mmap）或 `Stream`（`--hdc` / `--adb`）。Stream 的 `rows` 按 `max_lines`（默认 500_000，`--max-lines` 仅 live）淘汰；File 无淘汰、可浏览全文件。`Visible::All`（身份映射）用于 Stream 与未过滤 File；File 过滤用 `Visible::Subset(行号)`。读路径统一 `App::row_at` → `RowRef`（Stream 借出 / File 惰性 Owned）。
- **`alnav` 的匹配行保留（Stream）**：filter active 时命中双写 `rows`+`matched`，`Visible::All` 索引 `matched`；`matched` 硬上限 `MATCHED_HARD_CAP`（1_000_000）。File 不过滤进 `matched`，只维护 `Subset`。书签：Stream 查 `rows`/`matched`；File 用稳定 `row_id = line_index+1`。preview 对 Stream 扫 `rows`、对 File 惰性 `row_at`。
- **`alnav` 的 Following**：离开底部的手动操作（`k`/`K`/`g`、向上滚轮/翻页、`n`/`N`、Visual、搜索跳转等）一律 `following=false`；**移到可见列表最后一行**（`j`/`J`/`G`、向下滚轮/翻页等经 `move_cursor_manual` 落地，或 `G`→`resume_following`）自动恢复 following；`Esc`（及同等取消路径）仍可显式 `resume_following`（钉底并恢复）。
- **`alnav` 的 LogList 滚动跟随**：`ui::render_log_list` 每帧用持久化的 `App.list_offset` 驱动 ratatui `List` 视口。
- **`alnav` 的 LogList 作为行动原点**：`Esc` / Insert 取消 / 提交 Filter 组 → `Focus::LogList` 并恢复 following；HighlightBox `Enter` 上屏后跳到首命中（退出 following）；`dd` 删光 strip 后回 LogList。popup 打开时 `Esc` 只关 popup。
- **`alnav` 的 LogList 快速移动与鼠标滚轮**：`Shift+J`/`Shift+K` 各 7 行（`help::FAST_SCROLL_STEP`，Help 面板滚动共用）；滚轮各 3 行，始终作用于日志列表。
- **`alnav` 的终端生命周期**：panic hook 在 `main()` 最开始安装；ADB/HDC 子进程经 `LiveChildGuard` RAII 清理。
- **`alnav` 的 Ctrl+C 语义**：Normal 退出；Insert 有 popup 只关 popup，否则重置 Input 并回 LogList；Search 模态编辑中取消草稿并回 LogList（不 resume following）。
- **`alnav` 的五个可聚焦分区**：`Focus` 为 `ChipStrip`/`ExcludeStrip`/`HighlightStrip`/`LogList`/`Input`（数字键 1–5）；Filter/Exclude/Highlight strip **为空则折叠**。持久化条件的新增/编辑统一走 Leader fzf Picker，不再用 `a`/`i`/`o` 打开旧 Input 面板。布局：Filter → Exclude → Highlight → Log（Fill）→ status。**H9 排除**：`GroupList.excludes` 全局 AND NOT；`C`+字段（字母表同 H7）；Exclude strip 与 Filter 同构 `h/l/dd/di`。
- **跨 crate 的日志颜色统一（仅 CLI）**：`alnav-core::logcolor` 是 CLI 唯一的颜色数据源（纯 RGB/枚举，不依赖 `colored`/`ratatui`），`formatter.rs` 将其转成 `colored` 类型。TUI 日志色（时间戳/level 徽标/关键词高亮）来自已安装的 `UiTokens`（Palette 映射 + `theme.toml` overlay），不再读 `logcolor`。`USER_HIGHLIGHT` 仍为 CLI `--highlight` 的 8 档色阶；TUI highlight 走 Palette 映射的 8-slot ramp。
- **`alnav` 日志区默认多行展示**：`ui.rs::wrap_ranges` 是唯一的换行实现（贪婪按空白断行，单词超宽则硬切），操作字节区间而非 `Cow<str>`（为了跟 `render_entry_lines` 里已经用 `Regex::find_iter` 算好的高亮命中区间对齐——顺序是"先算高亮区间，再换行"，换行只是把同一份区间数据切成多个 `Span` 分布到多个 `Line`，不会把一个高亮命中切碎到两半）。`ListItem` 内可以放多个 `Line`，`ListState` 选中/滚动天然按整个 item 处理，翻页逻辑（`PAGE_SIZE`/`move_cursor_manual`）不需要感知 item 内部行数。
- **`alnav` 靠上模态 + Preview（H1）**：Input / Search 用 `top_modal_rect`（靠上，非垂直居中）；垂直栈为模态正文 → 字段/历史候选 → **Preview**（`preview.rs` 采样约 10 条，不改主 `visible`/`following`）。Search 淡高亮走 `theme::preview_highlight_style`。msg-chip 面板仍居中。
- **`alnav` 字段详情 / Pretty overlay（H4/H5）**：LogList `p` 开关浮层（开→Fields，关→Closed）；`P` 开 Pretty 或在 Fields↔Pretty 间切换；靠上 `render_modal_shell`；Pretty 对 msg（失败再试 raw）做 JSON 缩进，非法则原文 + `not JSON`；内容随 `current_row`；Esc **只关浮层**不 `resume_following`；浮层内 `j`/`k`/`c`/`C`+字段仍可用。
- **`alnav` 导出 CLI（H10）**：LogList `y` `c` 将当前启用 Filter 组（组内 AND、组间 OR）、启用 Excludes（`not …`）、H8 lock（`--pid`/`--tid`）、全局时间窗（`--since`/`--until`）编码为一行 `alnav grep -f…|-i` / `--hdc` / `--adb` 命令（统一 `-e` + `-i`；不含 Search / `di` 禁用项）；复用 yank 剪贴板与 `YANKED` status。近似一致即可（环形缓冲截断可接受）。
- **`alnav` 边轨 minimap（H3）**：Log 边框内侧 1 列只读轨；比例基准为 `visible`；标记严重(E/F/crash)、启用 search 命中、当前视口淡段；重叠时严重优先；`visible` 非空即画极淡轨；样式走 `theme::minimap_*`；每帧扫描预算约 4000。
- **`alnav` 书签（对照盘）**：钉住时拷贝 `EntryRow` 快照；`ma` 开关当前行、`md` 按当前 origin 删除，`mm` 打开对照大面板（空则 `NO BOOKMARKS`）。Log 顶一行摘要 `★ N` + 时间跨度（空则折叠）；按日志时间排序并显示相对上一条的 Δt；上限 16。面板键仅 `j/k` `g/G` `yy` `dd` `Enter` `Esc`。原行仍在 `visible` 才能跳；过滤/淘汰标 ☆，快照仍可对照。进程退出丢弃。
- **`alnav` 配置外置**：启动时从配置目录读取 `theme.toml` 与 `config.toml`（默认 `~/.config/alnav`，`$ALNAV_HOME`/`--config-path DIR` 可覆盖）；`config.toml` 的 `theme` 选内置 Palette（`default` / `onedark` / `dracula` / `everforest` / `tokyo-night` / `catppuccin-mocha` / `gruvbox-dark` / `nord` / `kanagawa`），并配置 `picker_left_ratio` 等；`theme.toml` 是 overlay（`[palette]` 先合并，语义 token 后覆盖）；`logcolor` 仅服务 CLI。示例见 `alnav/examples/`。
- **`alnav` 边轨 minimap（H3）**：Log 边框内侧右侧 1 列；比例相对当前 `visible`；标记视口淡段 / 启用 search 命中 / 严重(E/F/crash)，重叠时严重优先；`visible` 非空即画极淡轨；样式仅走 `theme::minimap_*`；每帧采样上限约 4000。

## alnav UI 设计指导（opencode 风格）

配色与布局遵循以下规则；改动渲染代码前先看这里。所有颜色常量与映射函数集中在 `alnav/src/theme.rs`——**禁止在 `ui.rs` 或其他渲染代码里直接写 `Color::*`/硬编码 `Style`**，新增语义就去 `theme.rs` 加常量或函数，保证同一语义在任何地方渲染出来的颜色都一致。

- **Strip 弱边框 / 弹出浮层圆角描边**：Filter/Exclude/Highlight strip 用上下 `divider_block`（弱化边框）；Log 用 `rounded_block`。弹出浮层（Input/Search/Time/Detail/Help/Confirm/Picker 壳/Preview/字段候选/Command Palette）走 `render_modal_shell` / bordered `render_candidate_list`：`BorderType::Rounded` + `theme::border_style(true)`（dim accent）；相邻浮层留 1 格空隙。无 Preview 时 Picker 宽约一半。边框标题：strip 用 `numbered_title`，弹出用 `plain_title`。
- **单一强调色 + 大量 dim**：`theme::accent()` 是唯一的"焦点/强调"色（`default`/`nord` 为 cyan；其余内置主题用签名色：蓝/品红/绿/黄），非关键信息统一 `Modifier::DIM`，避免多色混战。Dashboard 六行 Unicode 字标走 `theme::logo` 渐变，Compact `"alnav"` 走 accent。
- **焦点**：popup/候选 List 用 `theme` 候选行 tokens（选中/非选中背景与文字、匹配字符色、选中前缀）；Filter/Highlight strip 组选中为 Magenta（`SELECTION_FRAME`）、未选中为 dim DarkGray；`di` 禁用用 `disabled_chip_style()`。
- **日志行选中态只在 LogList 聚焦时显示，且是柔和灰底**：经 `ListItem::style(log_selection_style())` 施加（**不用** `List::highlight_style`，以免 `Style::patch` 盖掉关键词高亮底色）；失焦时无选中底。关键词 Span 的高亮色在选中行上保持可读叠加。
- **状态栏三区**：左侧常驻 follow（on=success 填充 pill / off=同 glyph DIM）与 device（live=source accent pill，断开=disconnect warning pill，`-f`=file glyph accent）；lock/time/view-focus/progress/visual 仅在激活时用同一填充 pill 族；cursor `n/N` 与 match `k/total` 仍在左侧（无 `[]`）。中间 flash 为填充 pill（`FAILED`→warning，否则 success；3s）；右侧空闲仅 1–2 键（LogList `? help` + `; filter`，Strip `? help` + `d del…`），pending/modal 展开完整 L2；过窄先藏 hints。无左侧 `c…` 前缀。顶栏 toast overlay 为后续（YAGNI）。Picker 底部用柔和 `theme::picker_mode_prefix`（搜索镜 / `＋` / `✎`，accent+DIM、无填充）；非编辑不画居中模态。
- **Filter/Highlight chip 用填充 pill**：`theme::chip_pill`（按 `field_color` / level badge）与 `theme::highlight_pill_style`（`highlight_style`）；Input 已提交 chip 与 Filter strip 共用 pill 样式。
- **字段名颜色全局唯一映射**：`ChipField` 的颜色只由 `theme::field_color` 决定；popup 字段名与 pill 背景同源。
- **不硬编码 White/Black 当默认前景色**：如 `Level::I`（默认级别）用 `Style::default()` 继承终端主题色，不写 `Color::White`，以兼容浅色/深色终端。
- **日志相关颜色（时间戳/level 徽标/关键词高亮）一律从已安装的 `UiTokens` 读取**（Palette 映射；`theme.toml` overlay 可改），不在 `ui.rs` 里硬编码。CLI 彩色输出仍走 `alnav::logcolor`，与 TUI 主题无关。

语义色表（对应 `theme.rs` 常量，供扩展新 UI 元素时复用）：

| 用途 | 常量/函数 | 颜色来源 |
|------|-----------|------|
| 强调/焦点/聚焦边框 | `theme::accent()` | 主题签名色（default/nord=`cyan`） |
| Dashboard 字标 | `dashboard_logo_line_style` | 6 行 palette 渐变；default 纯 cyan |
| 成功/Following | `theme::SUCCESS` / `status_pill` | palette `green`；follow on=填充 pill，off=`status_icon_dim` |
| 会话 lock 徽标 | `theme::LOCK` / `status_pill_value` | palette `magenta` |
| 警告（chip 字段名 / disconnect / FAILED flash） | `theme::WARNING` | palette `yellow` |
| Status flash pill | `theme::status_flash_pill` | success 填充；含 `FAILED` 则 warning |
| Filter chip pill | `theme::chip_pill` | `field_color` / `level_badge_style` |
| Highlight pattern pill | `theme::highlight_pill_style` | `highlight_style(idx)` |
| Picker 候选选中/匹配 | `theme::candidate_*` / `picker_mode_prefix` | theme.toml 可覆盖 |
| Chip 组圆角边框 | `theme::chip_group_border_style` | 选中 Magenta / 未选中 dim DarkGray |
| 日志时间戳/pid/tid | `theme::muted()` | palette `bright_black` |
| 日志 level 徽标 | `theme::level_badge_style()` | Palette 映射（V/D/I/W/E/F） |
| 关键词/搜索高亮 | `theme::highlight_style(idx)` | 8-slot ramp（yellow → bright_green），按 search pattern 全局序号递进 |
| 禁用 chip 组 | `theme::disabled_chip_style()` | DarkGray+DIM |
| 日志行选中态（仅聚焦时） | `theme::log_selection_style()` | `Color::DarkGray`，经 ListItem.style 施加 |

## Filter Logic（`alnav-core` CLI）

- 同类型多值 = OR：`--tag "A" --tag "B"` → tag=A OR tag=B
- 同类型 AND：`--tag "A" --tag "B" --and` → tag=A AND tag=B
- 跨类型 = AND：`--tag "A" --msg "err"` → tag=A AND msg~err
- 值内 `|` 也是 OR：`--tag "A|B"`
- `--level W` 匹配 W/E/F（最低级别）
- `-e` 布尔表达式：支持 `and`/`or`/`not`/括号的任意组合
  - 语法：`FIELD ~ VALUE`、`level >= LEVEL`，用 `and`/`or`/`not`/`()` 组合
  - FIELD = `tag` | `msg` | `pkg` | `pid` | `tid`；VALUE = 裸词或 `"引号字符串"`
  - 多个 `-e` 之间 OR（与 grep `-e` 一致），与其他 flag AND

## alnav 已知范围外事项（YAGNI）

- 任意 `stdin` 管道流式输入（TUI 只支持 `--hdc` / `--adb` 自 spawn 子进程，因子进程生命周期完全自控，避免 stdin 管道场景下 Ctrl+C/`ISIG` 语义复杂度）
- 多文件 glob、`--sort-time` 归并；`--fields` 列可配置；搜索/过滤历史；光标行派生设时间；live 交互时间窗；相对时间（last 5m）；全文件日期索引；Windows 专门支持
- 日志区 `Ctrl-d`/`Ctrl-u` 翻页；草稿 vim 模态编辑；status 顶栏 toast overlay——留作后续扩展

## Exit Codes（`alnav-core` CLI）

- `0` — 有匹配
- `1` — 无匹配
- `2` — 参数错误
