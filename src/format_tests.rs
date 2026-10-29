use super::*;
use crate::qy::{Library, STANDARD_FEN};
use std::path::PathBuf;

fn directory(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("cbl-repair-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap(); path
}
fn root() -> Node { Node { from: 0, to: 0, parent: u32::MAX, children: vec![], comment: None } }
fn field(out: &mut Vec<u8>, value: &str) {
    let value = GBK.encode(value).0; out.extend_from_slice(&(value.len() as u32).to_le_bytes()); out.extend_from_slice(&value);
}
fn spec_record() -> Vec<u8> {
    include_str!("../tests/fixtures/cbl2-spec-record.hex").lines().filter(|s| !s.starts_with('#')).flat_map(str::split_whitespace).map(|v| u8::from_str_radix(v, 16).unwrap()).collect()
}
fn legacy_library(record: &[u8], compression: u32) -> Vec<u8> {
    use std::io::Write;
    let mut body = Vec::new();
    for s in ["说明书棋库", "来源", "非空Other", "创建人", "邮箱"] { field(&mut body, s); }
    body.extend_from_slice(&[0; 38]); body.extend_from_slice(&1u32.to_le_bytes()); field(&mut body, "说明||第二行"); body.extend_from_slice(record);
    let mut file = b"CCBridge".to_vec(); file.extend_from_slice(&0x15u32.to_le_bytes()); file.extend_from_slice(&compression.to_le_bytes()); file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    if compression == 0 { file.extend(body); } else {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(compression)); encoder.write_all(&body).unwrap(); file.extend(encoder.finish().unwrap());
    }
    file
}

#[test]
fn qingyi_legacy_spec_coordinates_metadata_and_compression() {
    let dir = directory("spec"); let path = dir.join("spec.cbl");
    let record = spec_record(); assert_eq!(record.len(), 1151);
    for compression in 0..=3 {
        std::fs::write(&path, legacy_library(&record, compression)).unwrap(); let lib = read(&path).unwrap();
        assert_eq!(lib.author.as_deref(), Some("创建人")); assert_eq!(lib.description.as_deref(), Some("说明\n第二行"));
        let game = &lib.games[0];
        assert_eq!(game.title, "吕钦先负胡荣华(2.12上海)"); assert_eq!(game.red_player.as_deref(), Some("吕钦")); assert_eq!(game.black_player.as_deref(), Some("胡荣华")); assert_eq!(game.recorder.as_deref(), Some("盛昱"));
        assert_eq!(game.result, 2); assert_eq!(game.type_tag, Some(0)); assert_eq!(game.position, 0);
        assert_eq!(game.nodes.len(), 121); assert_eq!((game.nodes[1].from, game.nodes[1].to), (56, 47)); assert_eq!(game.start_fen.as_deref(), Some(STANDARD_FEN));
    }
    let mut bad = record.clone();
    let mut cursor = Cursor::new(&bad); bytes(&mut cursor, 40).unwrap(); super::field(&mut cursor).unwrap(); bytes(&mut cursor, 4).unwrap(); for _ in 0..7 { super::field(&mut cursor).unwrap(); } bytes(&mut cursor, 4).unwrap(); for _ in 0..5 { super::field(&mut cursor).unwrap(); } bytes(&mut cursor, 43).unwrap(); let moves = cursor.position;
    bad[moves + 4] = 0xf0; std::fs::write(&path, legacy_library(&bad, 0)).unwrap(); assert!(read(&path).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_branch_depths_and_black_turn_match_qingyi() {
    let mut record = vec![0; 4]; record.extend_from_slice(&spec_record()[4..36]); record.extend_from_slice(&3u32.to_le_bytes()); field(&mut record, "残局比赛"); record.extend_from_slice(&17u32.to_le_bytes());
    for s in ["分支样本", "资源", "Other", "地点", "2001-02-18", "红方", "黑方"] { field(&mut record, s); } record.extend_from_slice(&4u32.to_le_bytes());
    for s in ["讲评人", "邮箱", "录入人", "邮箱", "说明||第二行"] { field(&mut record, s); } record.push(0); record.extend_from_slice(&[0; 38]);
    let steps = [[17,0,0x63,0x64,0x66,0], [18,0,0x26,0x25,0x66,0], [19,0,0x64,0x65,0x66,0], [18,0,0x66,0x65,0x62,0], [19,0,0x64,0x65,0x66,0], [17,0,0x23,0x24,0x62,0]];
    record.extend_from_slice(&(steps.len() as u32).to_le_bytes()); let moves = record.len(); for step in steps { record.extend_from_slice(&step); } for _ in steps { record.extend_from_slice(&GBK.encode("注释||第二行").0); record.extend_from_slice(b"\r\n"); }
    let size = record.len() as u32; record[..4].copy_from_slice(&size.to_le_bytes()); let game = legacy_record(&record, 1, &mut vec![]).unwrap();
    assert_eq!(game.type_tag, Some(3)); assert_eq!(game.position, 0); assert_eq!(game.nodes[0].children, [1,6]); assert_eq!(game.nodes[1].children, [2,4]); assert_eq!((game.nodes[1].from, game.nodes[1].to), (33,42)); assert!(game.start_fen.unwrap().ends_with(" b")); assert_eq!(game.nodes[4].comment.as_deref(), Some("注释\n第二行"));
    for (at,value) in [(moves,16), (moves+2,0xf0), (moves+4,0xf0), (moves+6,20)] { let mut bad = record.clone(); bad[at]=value; assert!(legacy_record(&bad,1,&mut vec![]).is_none()); }
}

#[test]
fn standard_position_black_turn_table_and_unicode_are_preserved() {
    let dir = directory("fields"); let path = dir.join("fields.cbl"); let mut game = empty_game();
    game.title = "扩展𠮷😀".into(); game.table = Some("第3台".into()); game.nodes = vec![root()]; game.nodes[0].comment = Some("根🐴".into());
    write(&path, "库𠮷", Some("作者"), Some("说明\n第二行"), &[game.clone()]).unwrap(); let lib = read(&path).unwrap(); let saved = &lib.games[0];
    assert_eq!(saved.start_fen.as_deref(), Some(STANDARD_FEN)); assert_eq!(saved.title, game.title); assert_eq!(saved.table, game.table); assert_eq!(saved.nodes, game.nodes); assert_eq!(lib.author.as_deref(), Some("作者"));
    std::fs::remove_file(&path).unwrap();
    game.start_fen = Some(STANDARD_FEN.replace(" w", " b")); game.position = 0; write(&path, "库", None, None, &[game.clone()]).unwrap(); assert!(read(&path).unwrap().games[0].start_fen.as_ref().unwrap().ends_with(" b"));
    std::fs::remove_file(&path).unwrap();
    game.start_fen = None; game.position = 1; let warnings = write(&path, "库", None, None, &[game]).unwrap(); assert!(warnings.iter().any(|s| s.contains("立场"))); assert!(read(&path).unwrap().games[0].start_fen.as_ref().unwrap().ends_with(" w"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn nested_variations_and_whitespace_comments_are_preserved() {
    let dir = directory("tree"); let path = dir.join("tree.cbl"); let mut game = empty_game(); game.nodes = vec![root()];
    for i in 1..=8 { game.nodes.push(Node { from: if i % 2 == 1 { 82 } else { 1 }, to: if i % 2 == 1 { 65 } else { 20 }, parent: 0, children: vec![], comment: Some(if i == 3 { " ".into() } else { format!("注释{i}🐴").into_boxed_str() }) }); }
    for (parent, children) in [(0, vec![1, 8]), (1, vec![2, 5]), (2, vec![3, 4]), (5, vec![6, 7])] { game.nodes[parent].children = children.clone(); for child in children { game.nodes[child as usize].parent = parent as u32; } }
    game.nodes[3].from = 65; game.nodes[3].to = 46;
    game.nodes[4].from = 65; game.nodes[4].to = 48;
    game.nodes[5].from = 65; game.nodes[5].to = 46;
    game.nodes[7].from = 7; game.nodes[7].to = 24;
    write(&path, "库", None, None, &[game.clone()]).unwrap(); assert_eq!(comparable(&game).unwrap(), comparable(&read(&path).unwrap().games[0]).unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn directory_guid_order_overrides_physical_order() {
    let dir = directory("order"); let path = dir.join("order.cbl"); let mut a = empty_game(); a.title = "物理甲".into(); let mut b = a.clone(); b.title = "物理乙".into();
    write(&path, "库", None, None, &[a, b]).unwrap(); let mut raw = std::fs::read(&path).unwrap();
    raw[66628..66632].copy_from_slice(&2u32.to_le_bytes()); raw[66904..66908].copy_from_slice(&1u32.to_le_bytes()); std::fs::write(&path, raw).unwrap();
    let lib = read(&path).unwrap(); assert_eq!(lib.games.iter().map(|g| g.title.as_str()).collect::<Vec<_>>(), ["物理乙", "物理甲"]);
    assert_eq!(guid_le("{A6E7AA1E-CE8B-4553-9144-232ACF26C9FC}").unwrap(), [0x1e,0xaa,0xe7,0xa6,0x8b,0xce,0x53,0x45,0x91,0x44,0x23,0x2a,0xcf,0x26,0xc9,0xfc]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn directory_slots_bound_records_and_exclude_deleted_and_unlisted_games() {
    let dir = directory("active-slots"); let path = dir.join("slots.cbl");
    let mut game = empty_game(); game.title = "相同标题".into(); game.nodes = vec![root()];
    write(&path, "库", None, None, &vec![game; 38]).unwrap();
    let original = std::fs::read(&path).unwrap();
    for capacity in [128usize, 256, 512, 1024] {
        let start = INDEX_START + capacity * INDEX_ENTRY_SIZE;
        let mut raw = vec![0; start];
        raw[..INDEX_START + 38 * INDEX_ENTRY_SIZE].copy_from_slice(&original[..INDEX_START + 38 * INDEX_ENTRY_SIZE]);
        raw[60..64].copy_from_slice(&(capacity as u32).to_le_bytes());
        raw.extend_from_slice(&original[INDEX_START + 38 * INDEX_ENTRY_SIZE..]);
        for i in 0..38 {
            let slot = INDEX_START + i * INDEX_ENTRY_SIZE;
            raw[slot + 4..slot + 8].copy_from_slice(&((38 - i) as u32).to_le_bytes());
            if i >= 27 { raw[slot..slot + 4].copy_from_slice(&5u32.to_le_bytes()); }
            let record = start + i * BLOCK_SIZE;
            // 填充区的伪记录头不能切断真实记录，也不能变成新的一局。
            raw[record + 2300..record + 2316].copy_from_slice(b"CCBridge Record\0");
            raw[record + 2319] = 2;
        }
        // 删除槽复用有效局 GUID，不能覆盖或重复导入有效局。
        let deleted = INDEX_START + 27 * INDEX_ENTRY_SIZE;
        raw.copy_within(INDEX_START + 20..INDEX_START + 96, deleted + 20);
        raw.copy_within(start + 20..start + 36, start + 27 * BLOCK_SIZE + 20);
        let stale = raw[start..start + BLOCK_SIZE].to_vec(); raw.extend_from_slice(&stale);
        std::fs::write(&path, &raw).unwrap();
        let loaded = read(&path).unwrap();
        assert_eq!(loaded.games.len(), 27, "38 条物理记录中只有 27 局有效，容量 {capacity}");
        assert!(loaded.games.iter().all(|g| g.title == "相同标题"));
        assert!(loaded.warnings.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), raw);
        let qyl_path = dir.join(format!("converted-{capacity}.qyl"));
        let mut qyl = Library { path: qyl_path.clone(), name: loaded.name, author: loaded.author, description: loaded.description, created_at: 0, games: loaded.games, warnings: vec![], dirty: true };
        qyl.save().unwrap();
        let saved = Library::load(&qyl_path).unwrap();
        assert_eq!(saved.games, qyl.games);
        assert!(saved.warnings.is_empty());
        assert_eq!(qyl.warnings.len(), 26);
        let titles: Vec<_> = saved.games.iter().map(|game| game.title.clone()).collect();
        let expected: Vec<_> = (1..=27).map(|i| if i == 1 { "相同标题".to_string() } else { format!("相同标题 ({i})") }).collect();
        assert_eq!(titles, expected);
        for i in 0..27 { let slot = INDEX_START + i * INDEX_ENTRY_SIZE; raw[slot..slot + 4].copy_from_slice(&5u32.to_le_bytes()); }
        assert!(read_library(&raw).unwrap().games.is_empty());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn modern_directory_rejects_bad_lengths_allocations_and_ambiguous_identities() {
    let dir = directory("bad-directory"); let path = dir.join("bad.cbl");
    write(&path, "库", None, None, &[empty_game(), empty_game()]).unwrap();
    let raw = std::fs::read(&path).unwrap();
    let start = INDEX_START + 2 * INDEX_ENTRY_SIZE;
    assert_eq!(raw[16..20], 3u32.to_le_bytes());
    assert_eq!(raw.len(), start + 2 * BLOCK_SIZE);
    assert_eq!(u32::from_le_bytes(raw[INDEX_START..INDEX_START + 4].try_into().unwrap()), 7);
    assert_eq!(u32::from_le_bytes(raw[INDEX_START + 8..INDEX_START + 12].try_into().unwrap()), 1);
    let length = u32::from_le_bytes(raw[INDEX_START + 12..INDEX_START + 16].try_into().unwrap()) as usize;
    assert!(length >= 2210 && length < BLOCK_SIZE);
    assert!(raw[start + length..start + BLOCK_SIZE].iter().all(|byte| *byte == 0));
    for (at, value) in [(60, u32::MAX), (INDEX_START + 8, u32::MAX), (INDEX_START + 12, 4097), (INDEX_START + 12, 20)] {
        let mut bad = raw.clone(); bad[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(read_library(&bad).is_err());
    }
    let mut bad = raw.clone(); bad[start + 20] ^= 1; assert!(read_library(&bad).is_err());
    let mut duplicate = raw.clone();
    duplicate.copy_within(INDEX_START + 20..INDEX_START + 96, INDEX_START + INDEX_ENTRY_SIZE + 20);
    duplicate.copy_within(start + 20..start + 36, start + BLOCK_SIZE + 20);
    assert!(read_library(&duplicate).is_err());
    assert!(read_library(&raw[..raw.len() - 1]).is_err());
    assert!(read_library(&raw[..INDEX_START + 1]).is_err());
    let mut game = empty_game(); game.nodes = vec![root()]; game.nodes[0].comment = Some("跨块注释".repeat(2000).into());
    let big = dir.join("multi-block.cbl"); write(&big, "库", None, None, &[game.clone()]).unwrap();
    let bytes = std::fs::read(&big).unwrap();
    assert!(u32::from_le_bytes(bytes[INDEX_START + 8..INDEX_START + 12].try_into().unwrap()) > 1);
    assert_eq!(read(&big).unwrap().games[0].nodes, game.nodes);
    let empty = dir.join("empty.cbl"); write(&empty, "空库", None, None, &[]).unwrap(); assert!(read(&empty).unwrap().games.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rejects_truncation_bad_tree_and_text_without_replacing_destination() {
    let dir = directory("reject"); let path = dir.join("reject.cbl"); std::fs::write(&path, b"existing output").unwrap();
    let mut game = empty_game(); game.title = "长".repeat(65); assert!(write(&path, "库", None, None, &[game.clone()]).unwrap_err().contains("拒绝截断"));
    game.title = "正常".into(); game.nodes = vec![root()]; game.nodes[0].children = vec![99]; assert!(write(&path, "库", None, None, &[game.clone()]).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"existing output"); game.nodes.clear();
    assert!(write(&path, "库", None, None, &[game.clone()]).unwrap_err().contains("未覆盖"));
    assert_eq!(std::fs::read(&path).unwrap(), b"existing output");
    let tmp = path.with_extension("cbl.tmp"); std::fs::write(&tmp, b"other process").unwrap(); assert!(write(&path, "库", None, None, &[game]).is_err()); assert_eq!(std::fs::read(&tmp).unwrap(), b"other process");
    let bad = dir.join("bad.cbl"); for size in 0..20 { let raw = legacy_library(&spec_record(), 0); std::fs::write(&bad, &raw[..size]).unwrap(); assert!(read(&bad).is_err()); }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn valid_prefix_and_current_move_with_partial_comment_are_retained_with_warnings() {
    let dir = directory("tail"); let path = dir.join("tail.cbl"); let mut game = empty_game(); game.nodes = vec![root(), Node { from: 56, to: 47, parent: 0, children: vec![], comment: None }]; game.nodes[0].children = vec![1];
    write(&path, "库", None, None, &[game]).unwrap(); let raw = std::fs::read(&path).unwrap();
    let start = INDEX_START + INDEX_ENTRY_SIZE;
    let length = u32::from_le_bytes(raw[INDEX_START + 12..INDEX_START + 16].try_into().unwrap()) as usize;
    let with_tail = |suffix: &[u8]| {
        let mut data = raw.clone();
        data[start + length..start + length + suffix.len()].copy_from_slice(suffix);
        data[INDEX_START + 12..INDEX_START + 16].copy_from_slice(&((length + suffix.len()) as u32).to_le_bytes());
        data
    };
    let tail = with_tail(&[0,0,99,98]); std::fs::write(&path, tail).unwrap(); let lib = read(&path).unwrap(); assert_eq!(lib.games[0].nodes.len(), 2); assert!(!lib.warnings.is_empty());
    for suffix in [vec![4,0,99,98], vec![4,0,54,45]] {
        std::fs::write(&path, with_tail(&suffix)).unwrap();
        let lib = read(&path).unwrap(); assert_eq!(lib.games[0].nodes.len(), 2); assert!(!lib.warnings.is_empty());
    }
    for suffix in [vec![4,0,54,45,1,0,0,0,0], vec![4,0,54,45,100,0,0,0], vec![5,0,54,45,100,0,0,0,0xc4,0x8b,0xe8,0x6c]] {
        std::fs::write(&path, with_tail(&suffix)).unwrap();
        let lib = read(&path).unwrap(); assert_eq!(lib.games[0].nodes.len(), 3); assert!(!lib.warnings.is_empty());
        if suffix.len() == 12 { assert_eq!(lib.games[0].nodes[2].comment.as_deref(), Some("评注")); }
    }
    std::fs::write(&path, with_tail(&[1,0,40,41])).unwrap();
    let lib = read(&path).unwrap(); assert_eq!(lib.games[0].nodes.len(), 2); assert!(lib.warnings.iter().any(|w| w.contains("起点无棋子")));
    let mut broken = raw.clone(); broken[66624 + 276 + 19] = 99; std::fs::write(&path, broken).unwrap(); assert!(read(&path).is_err());
    let mut board_only = raw; board_only[INDEX_START + 12..INDEX_START + 16].copy_from_slice(&2210u32.to_le_bytes()); std::fs::write(&path, board_only).unwrap(); assert_eq!(read(&path).unwrap().games[0].nodes.len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn empty_root_terminator_partial_root_note_and_unknown_side_follow_qingyi() {
    let dir = directory("root-boundaries"); let path = dir.join("root.cbl");
    let mut game = empty_game(); game.nodes = vec![root()];
    write(&path, "库", None, None, &[game]).unwrap();
    let raw = std::fs::read(&path).unwrap();
    let start = INDEX_START + INDEX_ENTRY_SIZE;
    let moves = start + 2112 + 102;
    let set_moves = |steps: &[u8]| {
        let mut data = raw.clone();
        data[moves..moves + steps.len()].copy_from_slice(steps);
        let length = 2112 + 102 + steps.len();
        data[INDEX_START + 12..INDEX_START + 16].copy_from_slice(&(length as u32).to_le_bytes());
        data
    };
    // 空根记录也算读过根注释，第二个同坐标记录必须结束，不能覆盖根或继续读垃圾。
    let data = set_moves(&[0,0,0,0, 0,0,56,47, 4,0,0,0, 4,0,0,0, 0xc4,0x8b,0xe8,0x6c, 1,0,54,45]);
    std::fs::write(&path, data).unwrap();
    let loaded = read(&path).unwrap();
    assert_eq!(loaded.games[0].nodes.len(), 2);
    assert_eq!(loaded.games[0].nodes[0].comment, None);
    assert!(loaded.warnings.iter().any(|text| text.contains("结束标记")));
    // 截断根注释只取本条记录内完整的 UTF-16 字符。
    let data = set_moves(&[5,0,0,0, 100,0,0,0, 0xc4,0x8b,0xe8,0x6c]);
    std::fs::write(&path, data).unwrap();
    assert_eq!(read(&path).unwrap().games[0].nodes[0].comment.as_deref(), Some("评注"));
    for (from, to, side) in [(56,47,"w"), (27,36,"b")] {
        let mut data = set_moves(&[1,0,from,to]);
        data[start + 2112] = 0;
        std::fs::write(&path, data).unwrap();
        let loaded = read(&path).unwrap();
        assert_eq!(loaded.games[0].nodes.len(), 2);
        assert!(loaded.games[0].start_fen.as_deref().unwrap().ends_with(side));
        assert!(loaded.warnings.iter().any(|text| text.contains("轮走方标记异常")));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bounded_bad_legacy_record_is_skipped_and_good_record_keeps_original_ordinal() {
    let good = spec_record();
    let mut bad = good.clone(); *bad.last_mut().unwrap() = 0; // 末条注释的 CRLF 损坏，记录边界仍完整。
    let mut data = legacy_library(&bad, 0);
    let mut cursor = Cursor::new(&data[20..]);
    for _ in 0..5 { super::field(&mut cursor).unwrap(); }
    bytes(&mut cursor, 38).unwrap();
    let count_at = 20 + cursor.position;
    data[count_at..count_at + 4].copy_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&good);
    let length = (data.len() - 20) as u32; data[16..20].copy_from_slice(&length.to_le_bytes());
    let loaded = read_legacy(&data).unwrap();
    assert_eq!(loaded.games.len(), 1);
    assert_eq!(loaded.ordinals, [2]);
    assert_eq!(loaded.games[0].title, "吕钦先负胡荣华(2.12上海)");
    assert!(loaded.warnings.iter().any(|text| text.contains("第 1 局") && text.contains("已跳过")));
    assert!(read_legacy(&legacy_library(&bad, 0)).is_err());
    // 记录长度越界时不能定位下一条，必须拒绝整库。
    let mut cursor = Cursor::new(&data[20..]);
    for _ in 0..5 { super::field(&mut cursor).unwrap(); }
    bytes(&mut cursor, 42).unwrap(); super::field(&mut cursor).unwrap();
    let record_at = 20 + cursor.position;
    data[record_at..record_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(read_legacy(&data).is_err());
}

#[test]
fn numeric_and_custom_ecco_are_reported_without_rejecting_the_library() {
    let dir = directory("ecco");
    for (i, code) in ["12", "自定义", "Z00", "A12", "E99"].iter().enumerate() {
        let path = dir.join(format!("{i}.cbl")); let mut game = empty_game(); game.ecco_code = Some((*code).into()); game.ecco = Some(12);
        let warnings = write(&path, "库", None, None, &[game.clone()]).unwrap(); let saved = read(&path).unwrap();
        if ecco_supported(code) { assert_eq!(saved.games[0].ecco_code, game.ecco_code); }
        else { assert!(saved.games[0].ecco_code.is_none()); assert!(warnings.iter().any(|w| w.contains(&format!("ECCO 文本={code}")))); }
        assert_eq!(comparable(&game).unwrap(), comparable(&saved.games[0]).unwrap());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn real_samples_round_trip_every_representable_field_and_node() {
    let dir = directory("samples"); let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples"); let mut count = 0;
    for name in ["弈林新编.CBL", "象棋名局精解.CBL", "象棋谋略精要 残棋篇 李中健 著 2000年4月第1版.CBL"] {
        let source = read(&examples.join(name)).unwrap(); count += source.games.len(); let path = dir.join(name);
        write(&path, &source.name, source.author.as_deref(), source.description.as_deref(), &source.games).unwrap(); let saved = read(&path).unwrap(); assert_eq!(source.games.len(), saved.games.len());
        for (a,b) in source.games.iter().zip(&saved.games) { assert_eq!(comparable(a).unwrap(), comparable(b).unwrap(), "{name} / {}", a.title); }
        let mut qyl = Library { path: path.with_extension("qyl"), name: source.name.clone(), author: source.author.clone(), description: source.description.clone(), created_at: 7, games: source.games.clone(), warnings: vec![], dirty: true };
        qyl.save().unwrap();
        let saved = Library::load(&qyl.path).unwrap();
        assert_eq!(saved.games, qyl.games);
        assert!(saved.warnings.is_empty());
        let mut titles = std::collections::HashSet::new();
        for (original, converted) in source.games.iter().zip(&saved.games) {
            assert!(titles.insert(&converted.title), "QYL 标题必须唯一");
            let mut expected = original.clone(); expected.title = converted.title.clone();
            assert_eq!(&expected, converted, "除重名标题外，其余字段与着法必须完整保留");
        }
    }
    assert_eq!(count, 1109);
    let source = Library::load(&examples.join("单车例胜士象全.qyl")).unwrap(); let path = dir.join("qyl-to-cbl.cbl"); let warnings = write(&path, &source.name, source.author.as_deref(), source.description.as_deref(), &source.games).unwrap(); assert!(warnings.iter().any(|s| s.contains("棋局性质"))); let saved = read(&path).unwrap(); assert_eq!(saved.games.len(), 3); for (a,b) in source.games.iter().zip(&saved.games) { assert_eq!(comparable(a).unwrap(), comparable(b).unwrap()); }
    std::fs::remove_dir_all(dir).unwrap();
}
