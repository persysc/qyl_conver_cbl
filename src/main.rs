#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod cbl;
mod fen;
mod qy;
mod selection;

#[cfg(test)]
mod ui_tests;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::*;
use qy::{Game, Library};
use selection::Selection;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

actions!(convert, [SelectAll, ClearSelection]);

struct Shelf {
    path: PathBuf,
    name: String,
    author: Option<String>,
    description: Option<String>,
    created_at: u64,
    warnings: Vec<String>,
    games: Vec<Game>,
    ordinals: Vec<usize>,
    kind: &'static str,
}

struct Tool {
    shelves: Vec<Shelf>,
    active: Option<usize>,
    chosen: Selection,
    list: Entity<ListState<Rows>>,
    focus_handle: FocusHandle,
    message: SharedString,
}

struct Rows { games: Rc<Vec<Game>> }

impl ListDelegate for Rows {
    type Item = ListItem;
    fn items_count(&self, _: usize, _: &App) -> usize { self.games.len() }
    fn render_item(&mut self, ix: IndexPath, _: &mut Window, cx: &mut Context<ListState<Self>>) -> Option<Self::Item> {
        let game = self.games.get(ix.row)?;
        let title = if game.title.trim().is_empty() { "未命名棋谱".to_string() } else { game.title.clone() };
        let players = match (clean(&game.red_player), clean(&game.black_player)) {
            (Some(red_name), Some(black_name)) => format!("{red_name} 对 {black_name}"),
            (Some(name), None) | (None, Some(name)) => name,
            (None, None) => String::new(),
        };
        Some(ListItem::new(("game", ix.row)).h_10().px_3()
            .bg(if ix.row.is_multiple_of(2) { cx.theme().background } else { cx.theme().table_even })
            .border_b_1().border_color(cx.theme().table_row_border)
            .child(columns([format!("{:04}", ix.row + 1).into(), title.into(), players.into(), clean(&game.event).unwrap_or_default().into(), date_text(game.date).into(), result_name(game.result).into()])))
    }
    fn set_selected_index(&mut self, _: Option<IndexPath>, _: &mut Window, _: &mut Context<ListState<Self>>) {}
}

impl Tool {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| ListState::new(Rows { games: Rc::new(Vec::new()) }, window, cx).selectable(false));
        let focus_handle = cx.focus_handle().tab_stop(true);
        window.focus(&focus_handle, cx);
        Self { shelves: Vec::new(), active: None, chosen: Selection::default(), list, focus_handle, message: "打开或拖入 CBL、QYL 棋库或 FEN 文本（.txt）".into() }
    }
    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = rfd::AsyncFileDialog::new().set_parent(window).add_filter("棋库与 FEN 文本", &["cbl", "qyl", "txt"]);
        cx.spawn_in(window, async move |this, cx| {
            let files = dialog.pick_files().await;
            this.update_in(cx, |this, window, cx| {
                if let Some(files) = files { this.add(files.into_iter().map(|file| file.path().to_path_buf()).collect(), window, cx); }
                window.focus(&this.focus_handle, cx);
            }).ok();
        }).detach();
    }
    fn drop_files(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        self.add(paths.paths().to_vec(), window, cx);
    }
    fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = rfd::AsyncFileDialog::new().set_parent(window).set_title("选择文件夹（自动包含子文件夹）");
        cx.spawn_in(window, async move |this, cx| {
            let folder = dialog.pick_folder().await;
            this.update_in(cx, |this, window, cx| {
                if let Some(folder) = folder {
                    let mut paths = Vec::new();
                    scan_libraries(folder.path(), &mut paths);
                    if paths.is_empty() {
                        this.message = format!("{}：没有找到 .cbl / .qyl 棋库", folder.path().display()).into();
                    } else {
                        this.add(paths, window, cx);
                    }
                }
                window.focus(&this.focus_handle, cx);
            }).ok();
        }).detach();
    }
    fn add(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let mut files = Vec::new();
        let mut folders = 0usize;
        for path in paths {
            if path.is_dir() {
                folders += 1;
                scan_libraries(&path, &mut files);
            } else {
                files.push(path);
            }
        }
        let scanned = files.len();
        let mut errors = Vec::new();
        let mut added = 0usize;
        for path in files {
            if self.shelves.iter().any(|shelf| shelf.path == path) { continue; }
            match load_shelf(&path) {
                Ok(shelf) => {
                    for warning in &shelf.warnings { errors.push(format!("{}：注意，{warning}", file_name(&path))); }
                    self.shelves.push(shelf); added += 1;
                }
                Err(error) => errors.push(format!("{}：{error}", file_name(&path))),
            }
        }
        if self.active.is_none() && !self.shelves.is_empty() { self.active = Some(0); }
        if folders > 0 && scanned == 0 {
            errors.insert(0, format!("已扫描 {folders} 个文件夹，没有找到 .cbl / .qyl 棋库"));
        } else if folders > 0 {
            errors.insert(0, format!("已加入 {added} 个棋库，共 {} 个（扫描 {folders} 个文件夹、{scanned} 个棋库文件）", self.shelves.len()));
        } else {
            errors.insert(0, format!("已加入 {added} 个棋库，共 {} 个", self.shelves.len()));
        }
        self.message = errors.join("\n").into();
        window.focus(&self.focus_handle, cx);
        self.sync(cx);
    }
    fn pick(&mut self, index: usize, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let order: Vec<usize> = (0..self.shelves.len()).collect();
        let modifiers = event.modifiers();
        self.chosen.select(index, &order, modifiers.secondary(), modifiers.shift);
        self.active = Some(index);
        window.focus(&self.focus_handle, cx);
        self.sync(cx);
    }
    fn close(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.shelves.len() { return; }
        self.shelves.remove(index);
        self.chosen.closed(index);
        self.active = self.active.and_then(|item| if item == index { self.shelves.get(index).map(|_| index).or_else(|| index.checked_sub(1)) } else if item > index { Some(item - 1) } else { Some(item) });
        self.message = format!("剩余 {} 个棋库", self.shelves.len()).into();
        self.sync(cx);
    }
    fn clear_all(&mut self, cx: &mut Context<Self>) {
        self.shelves.clear();
        self.chosen = Selection::default();
        self.active = None;
        self.message = "列表已清空".into();
        self.sync(cx);
    }
    fn clear_chosen(&mut self, cx: &mut Context<Self>) {
        self.chosen = Selection::default();
        self.message = "已取消选中".into();
        cx.notify();
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.chosen.items = (0..self.shelves.len()).collect();
        self.message = format!("已选中全部 {} 个棋库", self.shelves.len()).into();
        cx.notify();
    }
    fn sync(&mut self, cx: &mut Context<Self>) {
        let games = Rc::new(self.active.and_then(|index| self.shelves.get(index)).map(|shelf| shelf.games.clone()).unwrap_or_default());
        self.list.update(cx, |list, cx| { list.delegate_mut().games = games; cx.notify(); });
        cx.notify();
    }
    fn convert(&mut self, target: &'static str, cx: &mut Context<Self>) {
        let order: Vec<usize> = (0..self.shelves.len()).collect();
        let indexes = self.chosen.targets(&order);
        if indexes.is_empty() { self.message = "先在左侧选中要转换的棋库".into(); cx.notify(); return; }
        let mut lines = Vec::new();
        let (mut done, mut skipped, mut failed) = (0usize, 0usize, 0usize);
        for index in indexes {
            let Some(shelf) = self.shelves.get(index) else { continue };
            let name = file_name(&shelf.path);
            match convert_shelf(shelf, target) {
                ConvertResult::Done(warnings) => {
                    done += 1;
                    let path = shelf.path.with_extension(target);
                    lines.push(if warnings.is_empty() { format!("{name}：已转为 {}，内容校验通过", file_name(&path)) } else { format!("{name}：已转为 {}，有兼容性提示", file_name(&path)) });
                    let all: Vec<String> = shelf.warnings.iter().cloned().chain(warnings).collect();
                    lines.extend(all.into_iter().map(|warning| format!("{name}：注意，{warning}")));
                }
                ConvertResult::Skipped(reason) => { skipped += 1; lines.push(format!("{name}：{reason}")); }
                ConvertResult::Failed(error) => { failed += 1; lines.push(format!("{name}：转换失败，{error}")); }
            }
        }
        lines.insert(0, format!("转换完成：成功 {done}，失败 {failed}，跳过 {skipped}"));
        self.message = lines.join("\n").into();
        cx.notify();
    }
    fn auto_convert(&mut self, cx: &mut Context<Self>) {
        let order: Vec<usize> = (0..self.shelves.len()).collect();
        let mut indexes = self.chosen.targets(&order);
        let whole_list = indexes.is_empty();
        if whole_list { indexes = order; }
        if indexes.is_empty() { self.message = "列表为空，请先打开或拖入棋库（可直接拖文件夹）".into(); cx.notify(); return; }
        let (mut done, mut skipped, mut failed) = (0usize, 0usize, 0usize);
        let (mut warnings_total, mut new_files) = (0usize, 0usize);
        let mut lines = Vec::new();
        for index in indexes {
            let Some(shelf) = self.shelves.get(index) else { continue };
            let name = file_name(&shelf.path);
            let target: &'static str = match shelf.kind {
                "cbl" => "qyl",
                "qyl" => "cbl",
                _ => "qyl",
            };
            match convert_shelf(shelf, target) {
                ConvertResult::Done(warnings) => {
                    done += 1;
                    warnings_total += warnings.len();
                    new_files += 1;
                    let path = shelf.path.with_extension(target);
                    lines.push(format!("{name} → {}：成功{}", file_name(&path), if warnings.is_empty() { String::new() } else { format!("（{} 条兼容性提示）", warnings.len()) }));
                }
                ConvertResult::Skipped(reason) => { skipped += 1; lines.push(format!("{name}：{reason}")); }
                ConvertResult::Failed(error) => { failed += 1; lines.push(format!("{name}：转换失败，{error}")); }
            }
        }
        let scope = if whole_list { "全部棋库" } else { "已选棋库" };
        lines.insert(0, format!("自动双向转换（{scope}）：成功 {done}，失败 {failed}，跳过 {skipped}；新增 {new_files} 个文件，兼容性提示 {warnings_total} 条"));
        self.message = lines.join("\n").into();
        self.sync(cx);
    }
}

enum ConvertResult { Done(Vec<String>), Skipped(String), Failed(String) }

fn convert_shelf(shelf: &Shelf, target: &'static str) -> ConvertResult {
    if shelf.kind == target { return ConvertResult::Skipped(format!("已经是 {target}，未转换")); }
    if shelf.kind == "txt" && target != "qyl" { return ConvertResult::Skipped("FEN 文本只能转为 QYL".into()); }
    let path = shelf.path.with_extension(target);
    if path.exists() { return ConvertResult::Skipped(format!("已跳过，目标 {} 已存在，未覆盖", file_name(&path))); }
    let result = if target == "cbl" {
        cbl::write(&path, &shelf.name, shelf.author.as_deref(), shelf.description.as_deref(), &shelf.games).map(|mut warnings| {
            if shelf.created_at != 0 { warnings.push(format!("QYL 创建时间戳 {} 没有 CBL 对应字段", shelf.created_at)); }
            warnings
        })
    } else { save_qyl(&path, shelf) };
    match result {
        Ok(warnings) => ConvertResult::Done(warnings),
        Err(error) => ConvertResult::Failed(error),
    }
}

fn is_library(path: &Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.to_ascii_lowercase()).as_deref() {
        Some("cbl") | Some("qyl") => true,
        Some("txt") => fen::looks_like_text(path),
        _ => false,
    }
}

fn scan_libraries(dir: &Path, files: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    scan_into(dir, files, &mut seen, 0);
}

fn scan_into(dir: &Path, files: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, depth: usize) {
    if depth > 64 { return; }
    let Ok(canonical) = dir.canonicalize() else { return };
    if !seen.insert(canonical) { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() || (kind.is_symlink() && path.is_dir()) {
            subdirs.push(path);
        } else if kind.is_file() && is_library(&path) {
            files.push(path);
        }
    }
    subdirs.sort();
    for subdir in subdirs { scan_into(&subdir, files, seen, depth + 1); }
}

fn load_shelf(path: &Path) -> Result<Shelf, String> {
    let (kind, name, author, description, created_at, games, ordinals, warnings) = match path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.to_ascii_lowercase()).as_deref() {
        Some("cbl") => { let lib = cbl::read(path)?; ("cbl", lib.name, lib.author, lib.description, 0, lib.games, lib.ordinals, lib.warnings) }
        Some("qyl") => { let lib = Library::load(path)?; ("qyl", lib.name, lib.author, lib.description, lib.created_at, lib.games, vec![], lib.warnings) }
        Some("txt") => { let lib = fen::read(path)?; ("txt", lib.name, None, None, 0, lib.games, vec![], lib.warnings) }
        _ => return Err("只支持 CBL、QYL 棋库和 FEN 文本（.txt）".into()),
    };
    Ok(Shelf { path: path.to_path_buf(), name, author, description, created_at, games, ordinals, warnings, kind })
}

fn save_qyl(path: &Path, shelf: &Shelf) -> Result<Vec<String>, String> {
    let created_at = if shelf.created_at != 0 { shelf.created_at } else { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|time| time.as_secs()).unwrap_or(0) };
    let mut library = Library { path: path.to_path_buf(), name: shelf.name.clone(), author: shelf.author.clone(), description: shelf.description.clone(), created_at, games: shelf.games.clone(), warnings: vec![], dirty: true };
    if shelf.kind == "cbl" {
        if library.games.is_empty() { return Err("该棋库中没有任何可转换的棋谱".into()); }
        let stem = shelf.path.file_stem().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "棋库".into());
        for (game, ordinal) in library.games.iter_mut().zip(&shelf.ordinals) {
            if game.title.trim().is_empty() {
                game.title = format!("{stem} 第{ordinal}局");
                library.warnings.push(format!("第 {ordinal} 局空标题调整为“{}”", game.title));
            }
        }
    }
    library.save()?;
    Ok(library.warnings)
}

fn file_name(path: &Path) -> String { path.file_name().and_then(|name| name.to_str()).unwrap_or("棋库").to_string() }
fn clean(value: &Option<String>) -> Option<String> { value.as_deref().map(str::trim).filter(|text| !text.is_empty()).map(str::to_string) }
fn date_text(date: u64) -> String { if date == 0 { String::new() } else if date.to_string().len() == 8 { let text = date.to_string(); format!("{}-{}-{}", &text[..4], &text[4..6], &text[6..]) } else { date.to_string() } }
fn result_name(result: u8) -> &'static str { match result { 1 => "红胜", 2 => "黑胜", 3 => "和棋", _ => "未知" } }

fn columns([number, title, players, event, date, result]: [SharedString; 6]) -> Div {
    h_flex().w_full().items_center().gap_3().text_sm()
        .child(div().w_10().flex_shrink_0().child(number))
        .child(div().flex_1().flex_grow(2.).min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(title))
        .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(players))
        .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(event))
        .child(div().w_24().flex_shrink_0().child(date))
        .child(div().w_10().flex_shrink_0().child(result))
}

fn configure_theme(cx: &mut App) {
    Theme::update(cx, |theme| {
        let (background, sidebar, head, stripe, border, primary, hover, pressed) = if theme.is_dark() {
            (0x151c26, 0x1b2533, 0x263448, 0x1a2432, 0x344256, 0x699bcb, 0x7caddb, 0x588ab9)
        } else {
            (0xf8fafc, 0xe8eef5, 0xe2eaf3, 0xf0f5fa, 0xd5dee9, 0x315b86, 0x274b70, 0x1e3c5c)
        };
        theme.background = rgb(background).into(); theme.sidebar = rgb(sidebar).into(); theme.title_bar = rgb(sidebar).into();
        theme.table_head = rgb(head).into(); theme.table_head_foreground = theme.foreground; theme.table_even = rgb(stripe).into();
        theme.border = rgb(border).into(); theme.table_row_border = theme.border;
        theme.accent = rgb(if theme.is_dark() { 0x243044 } else { 0xd7e4f2 }).into();
        theme.sidebar_accent = theme.accent; theme.list_active = theme.accent; theme.list_hover = theme.table_head;
        theme.primary = rgb(primary).into(); theme.primary_hover = rgb(hover).into(); theme.primary_active = rgb(pressed).into();
        theme.button_primary = theme.primary; theme.button_primary_hover = theme.primary_hover; theme.button_primary_active = theme.primary_active;
    });
}

impl Render for Tool {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shelves = self.shelves.iter().enumerate().map(|(index, shelf)| (index, file_name(&shelf.path), shelf.kind, shelf.games.len())).collect::<Vec<_>>();
        let active = self.active;
        let chosen = self.chosen.items.clone();
        let message = self.message.clone();
        let count = active.and_then(|index| self.shelves.get(index)).map(|shelf| shelf.games.len()).unwrap_or(0);
        let list = self.list.clone();
        let empty = chosen.is_empty();
        v_flex().size_full().bg(cx.theme().background).text_color(cx.theme().foreground)
            .id("convert").track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(|this, _: &ClearSelection, _, cx| this.clear_chosen(cx)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| this.drop_files(paths, window, cx)))
            .child(TitleBar::new().bg(cx.theme().title_bar).text_color(cx.theme().foreground).child(div().pl_3().text_sm().font_semibold().child("CBL / QYL 棋库转换")))
            .child(h_flex().items_stretch().flex_1().min_h_0().w_full()
                .child(v_flex().w_64().flex_shrink_0().h_full().min_w_0().p_4().gap_3().bg(cx.theme().sidebar).border_r_1().border_color(cx.theme().border)
                    .child(h_flex().justify_between().items_center().child(div().text_xl().font_semibold().child("棋库")).child(h_flex().gap_2()
                        .child(Button::new("unselect").ghost().compact().disabled(self.chosen.items.is_empty()).label("取消选中").on_click(cx.listener(|this, _, _, cx| this.clear_chosen(cx))))
                        .child(Button::new("clear").ghost().compact().disabled(self.shelves.is_empty()).label("清空").on_click(cx.listener(|this, _, _, cx| this.clear_all(cx))))))
                    .child(h_flex().gap_2()
                        .child(Button::new("open").label("打开文件").on_click(cx.listener(|this, _, window, cx| this.open(window, cx))))
                        .child(Button::new("open-dir").label("打开文件夹").on_click(cx.listener(|this, _, window, cx| this.open_folder(window, cx)))))
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child("单击查看，Ctrl＋单击多选，Shift＋单击连选，Ctrl＋A 全选，Esc 取消选中。拖入文件或文件夹均可，文件夹会递归扫描子目录。支持 CBL、QYL 棋库和每行一个 FEN 的 .txt 文本。"))
                    .child(div().id("shelves").flex_1().min_h_0().overflow_y_scrollbar().children(shelves.into_iter().map(|(index, name, kind, games)| {
                        let marked = chosen.contains(&index);
                        let row = div().id(("shelf", index)).test_support().w_full().min_w_0().p_3().mb_2().rounded_md().border_1().border_color(if marked { cx.theme().primary } else { cx.theme().border })
                            .bg(if marked { cx.theme().accent } else if active == Some(index) { cx.theme().sidebar_accent } else { cx.theme().background });
                        let row = if marked { row } else { row.hover(|row| row.bg(cx.theme().table_head)) };
                        row
                            .on_click(cx.listener(move |this, event, window, cx| this.pick(index, event, window, cx)))
                            .child(h_flex().w_full().items_start().justify_between().gap_2()
                                .child(div().min_w_0().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().child(name))
                                .child(Button::new(("close", index)).ghost().compact().label("×").on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    window.focus(&this.focus_handle, cx);
                                    this.close(index, cx);
                                }))))
                            .child(div().mt_1().text_sm().text_color(cx.theme().muted_foreground).child(format!("{kind} · {games} {}", if kind == "txt" { "题" } else { "局" })))
                    }))))
                .child(v_flex().flex_1().h_full().min_w_0()
                    .child(h_flex().px_4().h_16().flex_shrink_0().items_center().justify_between().gap_3().border_b_1().border_color(cx.theme().border)
                        .child(h_flex().flex_1().min_w_0().items_center().gap_3()
                            .child(div().flex_shrink_0().text_xl().font_semibold().child("棋谱"))
                            .child(div().min_w_0().text_sm().text_color(cx.theme().muted_foreground).child("转换后的棋库与源文件保存在同一目录")))
                        .child(h_flex().flex_shrink_0().gap_2()
                            .child(Button::new("auto").primary().disabled(self.shelves.is_empty()).label("自动双向转换").on_click(cx.listener(|this, _, _, cx| this.auto_convert(cx))))
                            .child(Button::new("to-qyl").ghost().disabled(empty).label("转为 QYL").on_click(cx.listener(|this, _, _, cx| this.convert("qyl", cx))))
                            .child(Button::new("to-cbl").ghost().disabled(empty).label("转为 CBL").on_click(cx.listener(|this, _, _, cx| this.convert("cbl", cx))))))
                    .child(div().h_9().px_3().flex_shrink_0().flex().items_center().bg(cx.theme().table_head).font_semibold().border_b_1().border_color(cx.theme().border)
                        .child(columns(["序号".into(), "标题".into(), "棋手对阵".into(), "赛事".into(), "日期".into(), "结果".into()])))
                    .child(div().px_3().py_2().flex_shrink_0().max_h_32().overflow_y_scrollbar().text_sm().text_color(cx.theme().muted_foreground).child(format!("当前棋库 {count} 局 · 已选 {} 个棋库\n{message}", chosen.len())))
                    .child(div().id("game-list").test_support().flex_1().min_h_0().child(List::new(&list).size_full()))))
    }
}

fn main() {
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        gpui_kit::init(cx);
        cx.bind_keys([
            KeyBinding::new("secondary-a", SelectAll, None),
            KeyBinding::new("escape", ClearSelection, None),
        ]);
        configure_theme(cx);
        cx.activate(true);
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        let mut options = TitleBar::window_options();
        options.window_bounds = Some(WindowBounds::Windowed(bounds));
        options.window_min_size = Some(size(px(860.), px(560.)));
        gpui_kit::open_window(options, cx, |window, cx| {
            window.set_window_title("CBL / QYL 棋库转换");
            cx.new(|cx| Tool::new(window, cx))
        }).expect("无法打开转换窗口");
    });
}
