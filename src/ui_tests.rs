use super::{configure_theme, ClearSelection, SelectAll, Tool};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Bounds, KeyBinding, TestAppContext, WindowBounds, px, size};
use std::path::Path;

#[test]
fn cbl_to_qyl_numbers_duplicate_titles_and_returns_warnings() {
    let dir = std::env::temp_dir().join(format!("cbl-qyl-unique-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("比赛.cbl");
    let games: Vec<_> = (0..3).map(|i| {
        let mut game = crate::qy::empty_game();
        game.title = "2022年全国象棋甲级联赛".into();
        game.red_player = Some(format!("棋手{i}"));
        game
    }).collect();
    crate::cbl::write(&source, "比赛", None, None, &games).unwrap();
    let before = std::fs::read(&source).unwrap();
    let shelf = super::load_shelf(&source).unwrap();
    let target = source.with_extension("qyl");
    let warnings = super::save_qyl(&target, &shelf).unwrap();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[0].contains("2022年全国象棋甲级联赛 (2)"));
    assert!(warnings[1].contains("2022年全国象棋甲级联赛 (3)"));
    let saved = crate::qy::Library::load(&target).unwrap();
    assert_eq!(saved.games.len(), 3);
    assert!(saved.warnings.is_empty());
    for (i, game) in saved.games.iter().enumerate() {
        assert_eq!(game.title, if i == 0 { "2022年全国象棋甲级联赛".to_string() } else { format!("2022年全国象棋甲级联赛 ({})", i + 1) });
        assert_eq!(game.red_player.as_deref(), Some(format!("棋手{i}").as_str()));
    }
    assert!(shelf.games.iter().all(|game| game.title == "2022年全国象棋甲级联赛"));
    assert_eq!(std::fs::read(&source).unwrap(), before);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cbl_to_qyl_empty_titles_use_source_filename_and_original_display_ordinal() {
    let dir = std::env::temp_dir().join(format!("cbl-qyl-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("文件名.cbl");
    let mut first = crate::qy::empty_game(); first.title = "文件名 第2局".into();
    let mut third = crate::qy::empty_game(); third.title = "  缩进标题  ".into();
    crate::cbl::write(&source, "不同的库名称", None, None, &[first, crate::qy::empty_game(), third]).unwrap();
    let mut shelf = super::load_shelf(&source).unwrap();
    // 模拟首局因单局异常被略过，后续原显示序号不能变成 1、2。
    shelf.games.remove(0); shelf.ordinals.remove(0);
    let target = source.with_extension("qyl");
    let warnings = super::save_qyl(&target, &shelf).unwrap();
    let saved = crate::qy::Library::load(&target).unwrap();
    assert_eq!(saved.games[0].title, "文件名 第2局");
    assert_eq!(saved.games[1].title, "  缩进标题  ");
    assert!(warnings.iter().any(|text| text.contains("第 2 局空标题")));
    let empty_source = dir.join("空库.cbl");
    crate::cbl::write(&empty_source, "空库", None, None, &[]).unwrap();
    let empty = super::load_shelf(&empty_source).unwrap();
    assert!(super::save_qyl(&empty_source.with_extension("qyl"), &empty).is_err());
    assert!(!empty_source.with_extension("qyl").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[gpui_kit::test]
fn ctrl_a_after_batch_import_and_left_click(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.bind_keys([
            KeyBinding::new("secondary-a", SelectAll, None),
            KeyBinding::new("escape", ClearSelection, None),
        ]);
        configure_theme(cx);
    });
    let (handle, tool) = cx.update(|cx| {
        let mut options = gpui_kit::component::TitleBar::window_options();
        options.window_bounds = Some(WindowBounds::Windowed(Bounds::new(
            Default::default(), size(px(1100.), px(720.)),
        )));
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| Tool::new(window, cx)))
            .expect("open converter window")
    });
    cx.update_window(handle, |_, window, cx| {
        tool.update(cx, |tool, cx| {
            let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
            tool.add(["弈林新编.CBL", "象棋名局精解.CBL", "单车例胜士象全.qyl"]
                .into_iter().map(|name| base.join(name)).collect(), window, cx);
            assert_eq!(tool.shelves.len(), 3, "{}", tool.message);
        });
        window.render_frame(cx);
        window.press("ctrl-a", cx);
        assert_eq!(tool.read(cx).chosen.items.len(), 3, "batch import then Ctrl+A");

        for index in [1usize, 0, 2] {
            window.click_at(("shelf", index), gpui_kit::point(px(20.), px(20.)), cx);
            assert_eq!(tool.read(cx).chosen.items.len(), 1, "left click selects one library");
            window.press("ctrl-a", cx);
            assert_eq!(tool.read(cx).chosen.items.len(), 3, "left click then Ctrl+A");
        }
        let active = tool.read(cx).active;
        window.press("escape", cx);
        assert!(tool.read(cx).chosen.items.is_empty(), "Esc clears all selections");
        assert_eq!(tool.read(cx).shelves.len(), 3);
        assert_eq!(tool.read(cx).active, active, "Esc keeps the preview");
        window.press("escape", cx);
        assert!(tool.read(cx).chosen.items.is_empty(), "repeated Esc is harmless");
        window.press("ctrl-a", cx);
        window.click_at("game-list", gpui_kit::point(px(30.), px(20.)), cx);
        window.press("escape", cx);
        assert!(tool.read(cx).chosen.items.is_empty(), "Esc works from the game list");
        window.press("ctrl-a", cx);

        window.click("unselect", cx);
        assert!(tool.read(cx).chosen.items.is_empty());
        window.press("ctrl-a", cx);
        assert_eq!(tool.read(cx).chosen.items.len(), 3, "cancel selection then Ctrl+A");

        window.click_at(("shelf", 0usize), gpui_kit::point(px(20.), px(20.)), cx);
        window.click(("close", 1usize), cx);
        assert_eq!(tool.read(cx).shelves.len(), 2);
        assert_eq!(tool.read(cx).active, Some(0), "closing another library keeps the preview");
        assert_eq!(tool.read(cx).chosen.items.len(), 1);
        window.press("ctrl-a", cx);
        assert_eq!(tool.read(cx).chosen.items.len(), 2, "close a library then Ctrl+A");
        window.click(("close", 1usize), cx);
        window.press("ctrl-a", cx);
        assert_eq!(tool.read(cx).chosen.items.len(), 1, "close last library then Ctrl+A");
        assert_eq!(tool.read(cx).active, Some(0));
        window.click(("close", 0usize), cx);
        assert_eq!(tool.read(cx).active, None, "closing the only library clears the preview");

        window.press("ctrl-a", cx);
        window.press("ctrl-a", cx);
        assert!(tool.read(cx).chosen.items.is_empty(), "empty list then Ctrl+A");
        tool.update(cx, |tool, cx| {
            let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
            let paths = gpui_kit::ExternalPaths([base.join("弈林新编.CBL"), base.join("单车例胜士象全.qyl")].into_iter().collect());
            tool.drop_files(&paths, window, cx);
        });
        window.render_frame(cx);
        window.press("ctrl-a", cx);
        assert_eq!(tool.read(cx).chosen.items.len(), 2, "import again through drop handler then Ctrl+A");
        assert_eq!(tool.read(cx).active, Some(0));
        window.click("clear", cx);
        window.press("ctrl-a", cx);
        assert!(tool.read(cx).chosen.items.is_empty(), "clear list then Ctrl+A");

        let dir = std::env::temp_dir().join(format!("cbl-batch-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, title) in [("成功.qyl", "正常".to_string()), ("失败.qyl", "长".repeat(65))] {
            let mut game = crate::qy::empty_game(); game.title = title; game.time_control = Some("60分钟".into());
            let mut library = crate::qy::Library { path: dir.join(name), name: "测试库".into(), author: Some("库作者".into()), description: Some("说明".into()), created_at: 7, games: vec![game], warnings: vec![], dirty: true };
            library.save().unwrap();
        }
        let before = std::fs::read(dir.join("成功.qyl")).unwrap();
        tool.update(cx, |tool, cx| tool.add(["成功.qyl", "失败.qyl"].map(|name| dir.join(name)).to_vec(), window, cx));
        window.render_frame(cx); window.press("ctrl-a", cx);
        tool.update(cx, |tool, cx| tool.convert("cbl", cx));
        let message = tool.read(cx).message.to_string();
        assert!(message.contains("成功.qyl：已转为 成功.cbl"), "{message}");
        assert!(message.contains("用时规则=60分钟"), "{message}");
        assert!(message.contains("失败.qyl：转换失败"), "{message}");
        assert!(message.contains("拒绝截断"), "{message}");
        assert!(dir.join("成功.cbl").exists()); assert!(!dir.join("失败.cbl").exists());
        assert_eq!(std::fs::read(dir.join("成功.qyl")).unwrap(), before, "source file stays intact");
        assert_eq!(crate::cbl::read(&dir.join("成功.cbl")).unwrap().author.as_deref(), Some("库作者"));
        let output = std::fs::read(dir.join("成功.cbl")).unwrap();
        tool.update(cx, |tool, cx| tool.convert("cbl", cx));
        assert!(tool.read(cx).message.contains("目标 成功.cbl 已存在，未覆盖"));
        assert_eq!(std::fs::read(dir.join("成功.cbl")).unwrap(), output);
        tool.update(cx, |tool, cx| { tool.clear_all(cx); tool.add(vec![dir.join("成功.cbl")], window, cx); tool.chosen.items.insert(0); });
        tool.update(cx, |tool, cx| tool.convert("qyl", cx));
        assert!(tool.read(cx).message.contains("目标 成功.qyl 已存在，未覆盖"));
        assert_eq!(std::fs::read(dir.join("成功.qyl")).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }).unwrap();
}
