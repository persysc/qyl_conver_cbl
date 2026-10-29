# 开发注意

从 QYL 棋库工具里已经核对过的问题。GPUI Kit 0.8.0（`gpui-pre` 0.3.9）升级后要重新核对。

- 启动必须调用 `gpui_kit::application().with_assets(gpui_kit::assets::Assets)`。不注册资源时，Windows 标题栏按钮能点但看不见。
- 自定义标题栏使用 `TitleBar::window_options()`，不要另放系统按钮。
- Windows 图标从 exe 资源 ID 1 读取。`build.rs` 必须检查 `embed_resource` 的返回值，否则找不到编译器时会静默跳过。
- 发布构建使用 `#![windows_subsystem = "windows"]`。不要只传 `/SUBSYSTEM:WINDOWS`，否则 MSVC 会找不存在的 `WinMain`。调试版保留控制台。
- 文件对话框使用 `AsyncFileDialog`，在 `cx.spawn_in` 里等待。同步调用会和 GPUI 借用的 `App` 冲突。
- `cx.focus_handle()` 每次都会新建句柄。窗口的句柄必须在 `Tool::new` 创建并由 `Tool` 保存，渲染的 `track_focus`、导入结束和点击棋库必须复用它。把临时句柄交给 `window.focus` 会让 Ctrl+A 找不到处理器。
- 快捷键修改运行 `src/ui_tests.rs` 的界面集成测试：加载真实样例，导入后直接 Ctrl+A、逐项点击后 Ctrl+A、取消选中及关闭棋库后 Ctrl+A。不要用格式读写测试代替按键分发验证。
- 长提示只显示文件名，用 `min_w_0`、单行和省略号限制宽度。
- 转换不覆盖已有目标。最终以硬链接原子提交新文件，不能把“先判断 exists 再 rename”当作并发保护；文件系统不支持时返回错误。
- 保存先独占新建临时文件（不能覆盖其他进程留下的 `.tmp`），写完同步磁盘，读回核对完整内容后再替换目标。QYL 比较全部 Game 字段及库信息；CBL 比较标准化 FEN、可表达元信息和按分支顺序重新编号的节点/注释，不能只比较局数。
- QYL 读写以本项目 `src/qy.rs` 为准，并对照 `F:/project/QingYi_ChineseChess`。目录只保存标题、红方、黑方、结果、日期和数据位置。
- CBL 只在清弈里有读取实现。新版以 `CCBridgeLibrary` 开头，2.1 版以 `CCBridge` 加版本 `0x15` 开头。本工具只把 QYL 写成新版 CBL。
- 转换必须保留棋手、用时、赛事、地点、轮次、结果、日期、注释和着法树，不得重排棋谱。CBL 允许同名棋谱，QYL 不允许：与清弈一致，按显示顺序分配标题，重名追加 ` (2)`、` (3)` 等，避开已分配标题；空标题使用源文件名加“第N局”（N 为原显示序号）。目录和正文同步使用最终标题，逐局提示改名。
- `start_fen=None` 表示标准开局，不是空棋盘。轮走方来自 FEN；`position` 是棋局立场（0 红、1 黑），不能拿它决定谁先走。
- QY 节点坐标从棋盘顶行起算。新版 CBL v2 原样存下标；新版记录 v1 用 `rank*9+file`；旧版 2.1 的 `Move.from="x,y"` 同样写成 `x+y*9`。不要把清弈 Board 内部的底边索引套到 QY 节点。
- 新版 CBL 通过记录 +20 的 GUID 与目录 +20 的 UTF-16 GUID 匹配，再按目录 +4 的序号稳定排序。头部 +60 可能记录容量，不能要求它与实际有效局数相等。
- CBL 的分组与台次是独立字段；QYL 没有分组。无法映射的字段必须逐局提示，不能静默丢失。CBL 固定文字字段按 UTF-16 单元校验容量，超长拒绝截断；注释中的空格和代理对字符必须保留。
- QYL 目录标题是清弈列表标题；目录与正文标题冲突时使用目录标题并提示，其他棋手、日期等取正文并提示差异。库级作者、说明、创建时间也要保留，不能在 Shelf 中丢掉。
- 读写修改必须运行完整 `cargo test --locked --offline`。`src/format_tests.rs` 含独立旧版规范样例、分支/排序/Unicode/坏输入/目标文件保护，以及 examples 下 1,109 局 CBL 和 3 局 QYL 的转换回归。详见 `FORMAT_COMPATIBILITY.md`。
