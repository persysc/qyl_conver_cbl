use crate::qy::{empty_game, position, Game, Node};
use std::collections::HashSet;
use encoding_rs::GBK;
use flate2::read::ZlibDecoder;
use std::io::Read;
use std::path::Path;

const CBL2_VERSION: u32 = 0x15;
const INDEX_START: usize = 66624;
const INDEX_ENTRY_SIZE: usize = 276;
const BLOCK_SIZE: usize = 4096;
const MAX_DATA: usize = 512 * 1024 * 1024;
const PIECES: &[u8] = b"RNBAKABNRCCPPPPPrnbakabnrccppppp";
const N2F: [char; 40] = ['*', '*', '*', '*', '*', '*', '*', '*', '*', '*', '*', '*', '*', '*', '*', '*', '*', 'R', 'N', 'B', 'A', 'K', 'C', 'P', '*', '*', '*', '*', '*', '*', '*', '*', '*', 'r', 'n', 'b', 'a', 'k', 'c', 'p'];

pub struct CblLibrary { pub name: String, pub author: Option<String>, pub description: Option<String>, pub games: Vec<Game>, pub ordinals: Vec<usize>, pub warnings: Vec<String> }

pub fn read(path: &Path) -> Result<CblLibrary, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > MAX_DATA as u64 { return Err("CBL 文件超过 512MB".into()); }
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let data = decode(&bytes)?;
    if data.starts_with(b"CCBridgeLibrary") { read_library(&data) } else { read_legacy(&data) }
}

fn decode(buffer: &[u8]) -> Result<Vec<u8>, String> {
    if buffer.starts_with(b"CCBridgeLibrary") { return Ok(buffer.to_vec()); }
    if buffer.get(..8) != Some(b"CCBridge") { return Err("不是 CBL 棋库".into()); }
    let version = u32::from_le_bytes(buffer.get(8..12).ok_or("CBL 文件头不完整")?.try_into().unwrap());
    if version != CBL2_VERSION { return Err(format!("不支持的 CBL 版本 {version:#x}")); }
    if buffer.len() < 20 { return Err("CBL 文件头不完整".into()); }
    let compression = u32::from_le_bytes(buffer[12..16].try_into().unwrap());
    let size = u32::from_le_bytes(buffer[16..20].try_into().unwrap()) as usize;
    if compression > 3 || size > MAX_DATA { return Err("CBL 压缩数据不正确".into()); }
    if compression == 0 { return (buffer.len() == size + 20).then(|| buffer.to_vec()).ok_or("CBL 数据长度不正确".into()); }
    let mut decoded = buffer[..20].to_vec();
    ZlibDecoder::new(&buffer[20..]).take(size as u64 + 1).read_to_end(&mut decoded).map_err(|error| error.to_string())?;
    if decoded.len() != size + 20 { return Err("CBL 解压长度不正确".into()); }
    decoded[12..16].fill(0);
    Ok(decoded)
}

fn need<T>(value: Option<T>) -> Result<T, String> { value.ok_or_else(|| "CBL 记录头不完整".to_string()) }

fn read_legacy(buffer: &[u8]) -> Result<CblLibrary, String> {
    let mut reader = Cursor::new(buffer.get(20..).ok_or("CBL 数据不完整")?);
    let name = text(&need(field(&mut reader))?);
    let source = text(&need(field(&mut reader))?);
    let other = need(field(&mut reader))?; // Other 是变长字段。
    let has_other = !other.is_empty();
    let author = some(text(&need(field(&mut reader))?));
    let email = text(&need(field(&mut reader))?);
    let mut warnings = Vec::new();
    if has_other { warnings.push("旧版库级 Other 预留字段没有 QYL 对应字段，未转换".into()); }
    for (label, value) in [("来源", source), ("作者邮箱", email)] { if !value.is_empty() { warnings.push(format!("库级{label}没有 QYL 对应字段：{value}")); } }
    for label in ["创建时间", "修改时间"] { let raw = need(bytes(&mut reader, 19))?; if raw.iter().any(|v| *v != 0) { warnings.push(format!("库级{label}为 CBL 文本时间，未转换为 QYL 时间戳：{}", text(raw))); } }
    let count = u32::from_le_bytes(need(bytes(&mut reader, 4))?.try_into().unwrap()) as usize;
    let description = some(text(&need(field(&mut reader))?).replace("||", "\n"));
    let mut offset = 20 + reader.position;
    if count > buffer.len().saturating_sub(offset) / 4 { return Err("CBL 记录数量不正确".into()); }
    let mut games = Vec::new();
    let mut ordinals = Vec::new();
    for ordinal in 1..=count {
        let len = u32::from_le_bytes(buffer.get(offset..offset + 4).ok_or("CBL 记录长度缺失")?.try_into().unwrap()) as usize;
        if len < 4 { return Err(format!("第 {ordinal} 条记录长度不正确")); }
        let end = offset.checked_add(len).ok_or("CBL 记录越界")?;
        let chunk = buffer.get(offset..end).ok_or("CBL 记录越界")?;
        match legacy_record(chunk, ordinal, &mut warnings).filter(|game| game.validate().is_ok()) {
            Some(game) => { games.push(game); ordinals.push(ordinal); }
            None => warnings.push(format!("第 {ordinal} 局旧版记录头、分支、着法或记谱损坏，已跳过该局")),
        }
        offset = end;
    }
    if offset != buffer.len() { return Err("CBL 记录区长度不正确".into()); }
    if count > 0 && games.is_empty() { return Err("CBL 棋谱全部解析失败，未导入此棋库".into()); }
    Ok(CblLibrary { name, author, description, games, ordinals, warnings })
}

fn legacy_record(chunk: &[u8], ordinal: usize, warnings: &mut Vec<String>) -> Option<Game> {
    let mut reader = Cursor::new(chunk);
    if u32::from_le_bytes(bytes(&mut reader, 4)?.try_into().ok()?) as usize != chunk.len() { return None; }
    let positions = bytes(&mut reader, 32)?;
    let mut board = ['*'; 90];
    for (&xy, &piece) in positions.iter().zip(PIECES) {
        if xy == 0 { continue; }
        let (x, y) = (xy >> 4, xy & 15);
        if !(1..=9).contains(&x) || !(1..=10).contains(&y) || board[(y as usize - 1) * 9 + x as usize - 1] != '*' { return None; }
        board[(y as usize - 1) * 9 + x as usize - 1] = piece as char;
    }
    let nature = match u32::from_le_bytes(bytes(&mut reader, 4)?.try_into().ok()?) { 1 | 2 => 0u8, 3 | 4 => 3, _ => return None };
    let event = text(&field(&mut reader)?);
    let start = u32::from_le_bytes(bytes(&mut reader, 4)?.try_into().ok()?);
    if start == 0 { return None; }
    let title = text(&field(&mut reader)?);
    for label in ["相关资源", "Other 预留字段"] {
        if !field(&mut reader)?.is_empty() { warnings.push(format!("第 {ordinal} 局“{title}”：{label}没有 QYL 对应字段，未转换")); }
    }
    let place = text(&field(&mut reader)?);
    let date = text(&field(&mut reader)?);
    let red = text(&field(&mut reader)?);
    let black = text(&field(&mut reader)?);
    let result = match u32::from_le_bytes(bytes(&mut reader, 4)?.try_into().ok()?) { 1 => 1, 2 => 2, 3 => 3, 4 => 0, _ => return None };
    let author = text(&field(&mut reader)?);
    let author_email = text(&field(&mut reader)?);
    let recorder = text(&field(&mut reader)?);
    let recorder_email = text(&field(&mut reader)?);
    for (label, value) in [("讲评人邮箱", author_email), ("录入人邮箱", recorder_email)] { if !value.is_empty() { warnings.push(format!("第 {ordinal} 局“{title}”：{label}没有 QYL 对应字段：{value}")); } }
    let remark = text(&field(&mut reader)?).replace("||", "\n");
    let side = match reader.next()? { 1 => "w", 0 => "b", _ => return None };
    for label in ["创建时间", "修改时间"] { let raw = bytes(&mut reader, 19)?; if raw.iter().any(|v| *v != 0) { warnings.push(format!("第 {ordinal} 局“{title}”：{label}没有 QYL 对应字段：{}", text(raw))); } }
    let move_count = u32::from_le_bytes(bytes(&mut reader, 4)?.try_into().ok()?) as usize;
    let steps = bytes(&mut reader, move_count.checked_mul(6)?)?;
    let mut notes = &chunk[reader.position..];
    let mut game = empty_game();
    game.title = title;
    game.event = some(event); game.site = some(place); game.date = digits(&date)?; game.red_player = some(red); game.black_player = some(black);
    game.result = result; game.author = some(author); game.recorder = some(recorder); game.type_tag = Some(nature);
    game.start_fen = Some(format!("{} {side}", fen(&board)));
    game.nodes.push(Node { from: 0, to: 0, parent: u32::MAX, children: Vec::new(), comment: some_box(&remark) });
    let mut path = vec![(0u32, board, side == "b")];
    for step in steps.as_chunks::<6>().0 {
        let depth = (u16::from_le_bytes([step[0], step[1]]) as u32).checked_sub(start)? as usize;
        path.truncate(depth.checked_add(1)?);
        if path.len() != depth + 1 { return None; }
        let (fx, fy, tx, ty) = (step[2] >> 4, step[2] & 15, step[3] >> 4, step[3] & 15);
        if fx > 8 || tx > 8 || fy > 9 || ty > 9 || (fx, fy) == (tx, ty) { return None; }
        let note_end = notes.windows(2).position(|mark| mark == b"\r\n")?;
        let note = text(&notes[..note_end]).replace("||", "\n");
        notes = &notes[note_end + 2..];
        let (parent, mut board, black) = *path.last()?;
        let (from, to) = (fy * 9 + fx, ty * 9 + tx);
        let piece = board[from as usize];
        let target = board[to as usize];
        if piece == '*' || piece.is_ascii_lowercase() != black || (target != '*' && target.is_ascii_lowercase() == black) || !legacy_notation(&step[4..6]) { return None; }
        board[to as usize] = piece; board[from as usize] = '*';
        let index = game.nodes.len() as u32;
        game.nodes[parent as usize].children.push(index);
        game.nodes.push(Node { from, to, parent, children: Vec::new(), comment: some_box(&note) });
        path.push((index, board, !black));
    }
    notes.is_empty().then_some(game)
}

fn read_library(buffer: &[u8]) -> Result<CblLibrary, String> {
    if buffer.len() < INDEX_START { return Err("CBL 文件头不完整".into()); }
    let capacity = u32::from_le_bytes(buffer[60..64].try_into().unwrap()) as usize;
    let index_end = capacity.checked_mul(INDEX_ENTRY_SIZE).and_then(|size| INDEX_START.checked_add(size)).ok_or("CBL 目录越界")?;
    let directory = buffer.get(INDEX_START..index_end).ok_or("CBL 目录不完整")?;
    let header = &buffer[..INDEX_START];
    let name = utf16(header, 64, 512);
    let author = some(utf16(header, 832, 64));
    let description = some(utf16(header, 1088, 65536));
    let mut warnings = Vec::new();
    for (label, at, size) in [("来源", 576, 256), ("作者邮箱", 896, 64), ("创建时间", 960, 64), ("修改时间", 1024, 64)] {
        let value = utf16(header, at, size);
        if !value.is_empty() { warnings.push(format!("库级{label}没有 QYL 对应字段：{value}")); }
    }
    let mut records = Vec::new();
    let mut offset = index_end;
    let mut identities = HashSet::new();
    // 删除槽仍占块；只读有效目录项，并按实际长度截断，不能扫描残留记录头。
    for (index, slot) in directory.chunks_exact(INDEX_ENTRY_SIZE).enumerate() {
        let value = |at| u32::from_le_bytes(slot[at..at + 4].try_into().unwrap());
        let (flags, seq, blocks, length) = (value(0), value(4), value(8) as usize, value(12) as usize);
        let allocated_end = blocks.checked_mul(BLOCK_SIZE).and_then(|size| offset.checked_add(size)).ok_or("CBL 分配块越界")?;
        buffer.get(offset..allocated_end).ok_or("CBL 分配块不完整")?;
        if flags & 2 != 0 {
            let end = offset.checked_add(length).filter(|end| *end <= allocated_end).ok_or("CBL 记录长度超出分配块")?;
            let record = &buffer[offset..end];
            let guid = guid_le(&utf16(slot, 20, 76)).ok_or_else(|| format!("目录第 {} 项 GUID 无效", index + 1))?;
            if !identities.insert(guid) || record.get(..16) != Some(b"CCBridge Record\0".as_slice()) || record.get(20..36) != Some(guid.as_slice()) {
                return Err(format!("目录第 {} 项记录头、GUID 不匹配或有效身份重复", index + 1));
            }
            let minimum = match record.get(19) { Some(1) => 1856 + 98, Some(2) => 2112 + 98, _ => return Err("CBL 记录版本无效".into()) };
            if record.len() < minimum { return Err("CBL 记录头或棋盘不完整".into()); }
            records.push((seq, record));
        }
        offset = allocated_end;
    }
    records.sort_by_key(|(seq, _)| *seq);
    let mut games = Vec::new();
    let mut ordinals = Vec::new();
    for (i, (_, record)) in records.iter().enumerate() {
        let ordinal = i + 1;
        match library_record(record, ordinal, &mut warnings) {
            Some(game) => { games.push(game); ordinals.push(ordinal); }
            None => warnings.push(format!("第 {ordinal} 局棋盘或着法损坏，已跳过该局")),
        }
    }
    if !records.is_empty() && games.is_empty() { return Err("CBL 棋谱全部解析失败，未导入此棋库".into()); }
    Ok(CblLibrary { name, author, description, games, ordinals, warnings })
}

fn guid_le(text: &str) -> Option<[u8; 16]> {
    let hex: String = text.trim().trim_matches(['{', '}']).chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 { return None; }
    let mut raw = [0u8; 16];
    for (i, byte) in raw.iter_mut().enumerate() { *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?; }
    raw[..4].reverse(); raw[4..6].reverse(); raw[6..8].reverse(); Some(raw)
}
fn legacy_notation(raw: &[u8]) -> bool { raw[0] >> 4 < 10 && raw[1] >> 4 < 3 && raw[1] & 15 < 9 }

fn library_record(buffer: &[u8], ordinal: usize, warnings: &mut Vec<String>) -> Option<Game> {
    let version = *buffer.get(19)?;
    if version != 1 && version != 2 { return None; }
    let (title_at, event_at, round_at, group_at, table_at, date_at, place_at, red_at, red_team_at, red_time_at, black_at, black_team_at, black_time_at, judge_at, record_at, _remark_at, author_at, result_at, body_at) = if version == 1 {
        (52, 564, 628, 692, 724, 756, 820, 948, 1012, 1076, 1172, 1236, 1300, 0, 0, 1396, 1524, 1820, 1856)
    } else {
        (180, 692, 756, 820, 852, 884, 948, 1076, 1140, 1204, 1300, 1364, 1428, 1524, 1588, 1652, 1780, 2076, 2112)
    };
    if buffer.len() < body_at + 98 { return None; }
    let mut board = ['.'; 90];
    for (index, square) in board.iter_mut().enumerate() {
        let code = *buffer.get(body_at + 8 + index)? as usize;
        *square = N2F.get(code).copied().filter(|piece| *piece != '*').unwrap_or('.');
    }
    let side = if buffer[body_at] == 2 { "b" } else { "w" };
    if !matches!(buffer[body_at], 1 | 2) { warnings.push(format!("第 {ordinal} 局：轮走方标记异常，与清弈一致先按红方处理，再由首着棋子修正")); }
    let mut game = empty_game();
    game.title = utf16(buffer, title_at, 128);
    game.event = some(utf16(buffer, event_at, 64)); game.round = some(utf16(buffer, round_at, 64)); game.table = some(utf16(buffer, table_at, 32));
    game.date = digits(&utf16(buffer, date_at, 64))?; game.site = some(utf16(buffer, place_at, 64));
    game.red_player = some(utf16(buffer, red_at, 64)); game.red_team = some(utf16(buffer, red_team_at, 64)); game.red_time = some(utf16(buffer, red_time_at, 64));
    game.black_player = some(utf16(buffer, black_at, 64)); game.black_team = some(utf16(buffer, black_team_at, 64)); game.black_time = some(utf16(buffer, black_time_at, 64));
    if version == 2 { game.referee = some(utf16(buffer, judge_at, 64)); game.recorder = some(utf16(buffer, record_at, 64)); }
    game.author = some(utf16(buffer, author_at, 64));
    game.result = match buffer.get(result_at).copied().unwrap_or(0) { 1 => 1, 2 => 2, 3 => 3, _ => 0 };
    let ecco = buffer.get(result_at.saturating_sub(40)..result_at.saturating_sub(37)).map(|bytes| String::from_utf8_lossy(bytes).trim().to_string()).unwrap_or_default();
    if ecco.len() == 3 && ecco.chars().next().is_some_and(|char| ('A'..='E').contains(&char)) && ecco[1..].chars().all(|char| char.is_ascii_digit()) { game.ecco_code = Some(ecco); }
    game.start_fen = Some(format!("{} {side}", fen(&board)));
    for (label, at, size) in [("分组", group_at, 32), ("红方等级分", red_time_at + 64, 32), ("黑方等级分", black_time_at + 64, 32), ("备注", _remark_at, 64)] {
        let value = utf16(buffer, at, size);
        if !value.is_empty() { warnings.push(format!("第 {ordinal} 局“{}”：{label}没有 QYL 对应字段：{value}", game.title)); }
    }
    game.nodes = read_nodes(buffer, body_at, version, ordinal, warnings)?;
    if let Some(node) = game.nodes.get(1) {
        if let Some(piece) = fen_piece(game.start_fen.as_deref().unwrap_or(""), node.from as usize) {
            let side = if piece.is_ascii_lowercase() { "b" } else { "w" };
            if let Some(board) = game.start_fen.as_deref().and_then(|value| value.split(' ').next()) {
                game.start_fen = Some(format!("{board} {side}"));
            }
        }
    }
    game.validate().ok()?;
    let (board, _) = position(&game).ok()?;
    let original = std::mem::take(&mut game.nodes);
    game.nodes.push(Node { children: vec![], ..original[0].clone() });
    let mut pending = Vec::new();
    for &child in original[0].children.iter().rev() { pending.push((child as usize, 0usize, board)); }
    while let Some((index, parent, mut board)) = pending.pop() {
        let source = &original[index];
        let piece = board[source.from as usize];
        if piece == '.' {
            warnings.push(format!("第 {ordinal} 局“{}”：着法 {}→{} 起点无棋子，已略过该分支及其后续", game.title, source.from, source.to));
            continue;
        }
        board[source.to as usize] = piece; board[source.from as usize] = '.';
        let new_index = game.nodes.len();
        game.nodes[parent].children.push(new_index as u32);
        game.nodes.push(Node { parent: parent as u32, children: vec![], ..source.clone() });
        for &child in source.children.iter().rev() { pending.push((child as usize, new_index, board)); }
    }
    Some(game)
}

fn read_nodes(buffer: &[u8], body_at: usize, version: u8, ordinal: usize, warnings: &mut Vec<String>) -> Option<Vec<Node>> {
    let mut nodes = vec![Node { from: 0, to: 0, parent: u32::MAX, children: Vec::new(), comment: None }];
    let mut stack = Vec::new();
    let mut root_seen = false;
    let mut parent = 0usize;
    let mut pos = body_at + 102;
    while pos < buffer.len() && buffer.len() - pos > 3 {
        let sig = buffer[pos];
        let (src, dst) = (buffer[pos + 2], buffer[pos + 3]);
        if sig > 7 || (src == dst && root_seen) {
            warnings.push(format!("第 {ordinal} 局：着法区有结束标记或附加数据，仅保留已解析的 {} 个着法", nodes.len() - 1)); break;
        }
        let (has_next, has_change) = (sig & 1 == 0, sig & 2 != 0);
        if src != dst && (square(version, src).is_none() || square(version, dst).is_none()) {
            warnings.push(format!("第 {ordinal} 局：尾部坐标越界，保留此前 {} 个着法", nodes.len() - 1)); break;
        }
        let (comment, step) = if sig & 4 != 0 {
            let Some(raw) = buffer.get(pos + 4..pos + 8) else {
                warnings.push(format!("第 {ordinal} 局：尾部注释长度缺失，保留此前 {} 个着法", nodes.len() - 1)); break;
            };
            let declared = u32::from_le_bytes(raw.try_into().ok()?) as usize;
            let len = declared.min(buffer.len() - pos - 8);
            if declared != len || len & 1 != 0 {
                warnings.push(format!("第 {ordinal} 局：尾部注释不完整，按当前记录边界保留本着及可读注释"));
            }
            (utf16(buffer, pos + 8, len), 8 + len)
        } else { (String::new(), 4) };
        if src == dst { root_seen = true; nodes[0].comment = some_box(&comment); pos = if has_next { pos + step } else { usize::MAX }; continue; }
        let (Some(from), Some(to)) = (square(version, src), square(version, dst)) else {
            warnings.push(format!("第 {ordinal} 局：尾部坐标越界，与清弈一致保留此前 {} 个着法", nodes.len() - 1)); break;
        };
        if from == to { break; }
        let index = nodes.len();
        nodes[parent].children.push(index as u32);
        nodes.push(Node { from, to, parent: parent as u32, children: Vec::new(), comment: some_box(&comment) });
        if has_next { if has_change { stack.push(parent); } parent = index; }
        else if !has_change { parent = stack.pop().unwrap_or(0); }
        pos += step;
    }
    if pos < buffer.len() && buffer.len() - pos < 4 { warnings.push(format!("第 {ordinal} 局：着法区尾部不足四字节，保留此前 {} 个着法", nodes.len() - 1)); }
    Some(nodes)
}

fn square(version: u8, value: u8) -> Option<u8> {
    if version == 1 {
        let (file, rank) = (value / 16, value % 16);
        (file < 9 && rank < 10).then_some(rank * 9 + file)
    } else {
        (value < 90).then_some(value)
    }
}

struct Cursor<'a> { data: &'a [u8], position: usize }
impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self { Self { data, position: 0 } }
    fn next(&mut self) -> Option<u8> { let byte = *self.data.get(self.position)?; self.position += 1; Some(byte) }
}
fn bytes<'a>(reader: &mut Cursor<'a>, len: usize) -> Option<&'a [u8]> {
    let end = reader.position.checked_add(len)?; let data = reader.data.get(reader.position..end)?; reader.position = end; Some(data)
}
fn field(reader: &mut Cursor<'_>) -> Option<Vec<u8>> { let len = u32::from_le_bytes(bytes(reader, 4)?.try_into().ok()?) as usize; bytes(reader, len).map(|data| data.to_vec()) }
fn text(bytes: &[u8]) -> String { GBK.decode(bytes).0.into_owned() }
fn utf16(buffer: &[u8], start: usize, length: usize) -> String {
    let end = start.saturating_add(length).min(buffer.len());
    let Some(raw) = buffer.get(start..end) else { return String::new() };
    let units: Vec<_> = raw.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes([p[0], p[1]])).take_while(|v| *v != 0).collect();
    String::from_utf16_lossy(&units)
}
fn some(value: String) -> Option<String> { (!value.is_empty()).then_some(value) }
fn some_box(value: &str) -> Option<Box<str>> { (!value.is_empty()).then(|| value.into()) }
fn digits(value: &str) -> Option<u64> {
    value.bytes().filter(u8::is_ascii_digit).try_fold(0u64, |acc, byte| acc.checked_mul(10)?.checked_add((byte - b'0') as u64))
}
fn fen(board: &[char]) -> String {
    let mut out = String::new();
    for row in 0..10 {
        let mut empty = 0;
        for column in 0..9 {
            let piece = board[row * 9 + column];
            if piece == '*' || piece == '.' { empty += 1; continue; }
            if empty > 0 { out.push_str(&empty.to_string()); empty = 0; }
            out.push(piece);
        }
        if empty > 0 { out.push_str(&empty.to_string()); }
        if row < 9 { out.push('/'); }
    }
    out
}
fn fen_piece(fen: &str, index: usize) -> Option<char> {
    let mut at = 0;
    for char in fen.split(' ').next()?.chars() {
        if char == '/' { continue; }
        if char.is_ascii_digit() { at += char.to_digit(10)? as usize; continue; }
        if at == index { return Some(char); }
        at += 1;
    }
    None
}

pub fn write(path: &Path, name: &str, author: Option<&str>, description: Option<&str>, games: &[Game]) -> Result<Vec<String>, String> {
    use std::io::Write as _;
    let mut warnings = Vec::new();
    check_text("棋库名", name, 512)?;
    check_text("棋库作者", author.unwrap_or(""), 64)?;
    check_text("棋库说明", description.unwrap_or(""), 65536)?;
    let index_end = games.len().checked_mul(INDEX_ENTRY_SIZE).and_then(|size| INDEX_START.checked_add(size)).filter(|end| *end <= MAX_DATA).ok_or("CBL 目录超过 512MB")?;
    let mut bytes = vec![0u8; index_end];
    bytes[..15].copy_from_slice(b"CCBridgeLibrary");
    bytes[16..20].copy_from_slice(&3u32.to_le_bytes());
    bytes[60..64].copy_from_slice(&u32::try_from(games.len()).map_err(|e| e.to_string())?.to_le_bytes());
    put_utf16_at(&mut bytes, 64, name, 512);
    put_utf16_at(&mut bytes, 832, author.unwrap_or(""), 64);
    put_utf16_at(&mut bytes, 1088, description.unwrap_or(""), 65536);
    for ordinal in 1..=games.len() {
        let start = INDEX_START + (ordinal - 1) * INDEX_ENTRY_SIZE;
        bytes[start..start + 4].copy_from_slice(&7u32.to_le_bytes());
        bytes[start + 4..start + 8].copy_from_slice(&(ordinal as u32).to_le_bytes());
        put_utf16_at(&mut bytes, start + 20, &record_guid(ordinal), 76);
    }
    for (ordinal, game) in games.iter().enumerate() {
        let context = format!("第 {} 局“{}”", ordinal + 1, game.title);
        game.validate().map_err(|e| format!("{context}：{e}"))?;
        for (label, value, capacity) in [
            ("标题", game.title.as_str(), 128), ("赛事", game.event.as_deref().unwrap_or(""), 64),
            ("轮次", game.round.as_deref().unwrap_or(""), 64), ("台次", game.table.as_deref().unwrap_or(""), 32),
            ("地点", game.site.as_deref().unwrap_or(""), 64), ("红方棋手", game.red_player.as_deref().unwrap_or(""), 64),
            ("黑方棋手", game.black_player.as_deref().unwrap_or(""), 64), ("红方队伍", game.red_team.as_deref().unwrap_or(""), 64),
            ("黑方队伍", game.black_team.as_deref().unwrap_or(""), 64), ("红方用时", game.red_time.as_deref().unwrap_or(""), 64),
            ("黑方用时", game.black_time.as_deref().unwrap_or(""), 64), ("裁判", game.referee.as_deref().unwrap_or(""), 64),
            ("记录员", game.recorder.as_deref().unwrap_or(""), 64), ("作者", game.author.as_deref().unwrap_or(""), 64),
        ] { check_text(label, value, capacity).map_err(|e| format!("{context}：{e}"))?; }
        let mut missing = Vec::new();
        if let Some(code) = game.ecco_code.as_deref().filter(|code| !ecco_supported(code)) { missing.push(format!("ECCO 文本={code}")); }
        if let Some(value) = &game.time_control { missing.push(format!("用时规则={value}")); }
        if let Some(value) = game.type_tag { missing.push(format!("棋局性质={value}")); }
        if let Some(value) = game.ecco { missing.push(format!("旧版 ECCO 数字={value}")); }
        if game.position != 0 { missing.push(format!("棋局立场={}", game.position)); }
        if !missing.is_empty() { warnings.push(format!("{context}：CBL 无对应字段，未写入：{}", missing.join("、"))); }
        let start = bytes.len();
        write_record(&mut bytes, game, ordinal + 1).map_err(|e| format!("{context}：{e}"))?;
        let length = bytes.len() - start;
        let blocks = length.div_ceil(BLOCK_SIZE);
        let end = blocks.checked_mul(BLOCK_SIZE).and_then(|size| start.checked_add(size)).filter(|end| *end <= MAX_DATA).ok_or("CBL 输出超过 512MB")?;
        let slot = INDEX_START + ordinal * INDEX_ENTRY_SIZE;
        bytes[slot + 8..slot + 12].copy_from_slice(&(blocks as u32).to_le_bytes());
        bytes[slot + 12..slot + 16].copy_from_slice(&(length as u32).to_le_bytes());
        bytes.resize(end, 0);
    }
    let temp = path.with_extension("cbl.tmp");
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp).map_err(|e| e.to_string())?;
    let result = (|| {
        file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
        drop(file);
        let check = read(&temp)?;
        if check.name != name || check.author.as_deref().unwrap_or("") != author.unwrap_or("") || check.description.as_deref().unwrap_or("") != description.unwrap_or("") || check.games.len() != games.len() || !check.warnings.is_empty() { return Err("CBL 保存后棋库信息校验失败".into()); }
        for (i, (original, saved)) in games.iter().zip(&check.games).enumerate() {
            if comparable(original)? != comparable(saved)? { return Err(format!("第 {} 局“{}”：保存后局面、元信息或着法注释校验失败", i + 1, original.title)); }
        }
        std::fs::hard_link(&temp, path).map_err(|e| format!("目标文件已存在或无法创建，未覆盖：{e}"))?;
        let _ = std::fs::remove_file(&temp);
        Ok(())
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temp); }
    result?; Ok(warnings)
}
fn ecco_supported(code: &str) -> bool { code.len() == 3 && matches!(code.as_bytes()[0], b'A'..=b'E') && code.as_bytes()[1..].iter().all(u8::is_ascii_digit) }
fn record_guid(ordinal: usize) -> String { format!("{{00000000-0000-0000-0000-{ordinal:012X}}}") }
fn check_text(label: &str, value: &str, capacity: usize) -> Result<(), String> {
    let units = value.encode_utf16().count();
    if value.contains('\0') { return Err(format!("{label}含有空字符，无法完整写入 CBL")); }
    if units > capacity / 2 { return Err(format!("{label}超过 CBL 容量（{units}/{} 个 UTF-16 单元），拒绝截断", capacity / 2)); }
    Ok(())
}
// CBL 不存 QY 的内部编号，比较时按分支顺序重新编号，并只消除已报告的格式差异。
fn comparable(game: &Game) -> Result<Game, String> {
    game.validate()?;
    let mut result = game.clone();
    let (board, black) = position(game)?;
    result.start_fen = Some(format!("{} {}", fen(&board), if black { "b" } else { "w" }));
    result.position = 0; result.time_control = None; result.type_tag = None; result.ecco = None;
    if result.ecco_code.as_deref().is_some_and(|code| !ecco_supported(code)) { result.ecco_code = None; }
    for field in [&mut result.red_player, &mut result.black_player, &mut result.red_team, &mut result.black_team, &mut result.red_time, &mut result.black_time, &mut result.event, &mut result.site, &mut result.round, &mut result.table, &mut result.referee, &mut result.recorder, &mut result.author] { if field.as_deref() == Some("") { *field = None; } }
    let mut nodes: Vec<Node> = Vec::new();
    let mut pending = vec![(0usize, u32::MAX)];
    if game.nodes.is_empty() { pending.clear(); }
    while let Some((old, parent)) = pending.pop() {
        let source = &game.nodes[old]; let index = nodes.len() as u32;
        if parent != u32::MAX { nodes[parent as usize].children.push(index); }
        nodes.push(Node { from: if old == 0 { 0 } else { source.from }, to: if old == 0 { 0 } else { source.to }, parent, children: vec![], comment: source.comment.clone().filter(|c| !c.is_empty()) });
        for &child in source.children.iter().rev() { pending.push((child as usize, index)); }
    }
    if nodes.is_empty() { nodes.push(Node { from: 0, to: 0, parent: u32::MAX, children: vec![], comment: None }); }
    result.nodes = nodes; Ok(result)
}

fn write_record(out: &mut Vec<u8>, game: &Game, ordinal: usize) -> Result<(), String> {
    let start = out.len();
    out.extend_from_slice(b"CCBridge Record");
    out.extend_from_slice(&[0, 0, 0, 0, 2]);
    out.resize(start + 180, 0);
    out[start + 20..start + 36].copy_from_slice(&guid_le(&record_guid(ordinal)).unwrap());
    put_utf16_at(out, start + 180, &game.title, 128);
    put_utf16_at(out, start + 692, game.event.as_deref().unwrap_or(""), 64);
    put_utf16_at(out, start + 756, game.round.as_deref().unwrap_or(""), 64);
    put_utf16_at(out, start + 852, game.table.as_deref().unwrap_or(""), 32);
    put_utf16_at(out, start + 884, &if game.date == 0 { String::new() } else { game.date.to_string() }, 64);
    put_utf16_at(out, start + 948, game.site.as_deref().unwrap_or(""), 64);
    put_utf16_at(out, start + 1076, game.red_player.as_deref().unwrap_or(""), 64);
    put_utf16_at(out, start + 1140, game.red_team.as_deref().unwrap_or(""), 64);
    put_utf16_at(out, start + 1204, game.red_time.as_deref().unwrap_or(""), 64);
    put_utf16_at(out, start + 1300, game.black_player.as_deref().unwrap_or(""), 64);
    put_utf16_at(out, start + 1364, game.black_team.as_deref().unwrap_or(""), 64);
    put_utf16_at(out, start + 1428, game.black_time.as_deref().unwrap_or(""), 64);
    put_utf16_at(out, start + 1524, game.referee.as_deref().unwrap_or(""), 64);
    put_utf16_at(out, start + 1588, game.recorder.as_deref().unwrap_or(""), 64);
    put_utf16_at(out, start + 1780, game.author.as_deref().unwrap_or(""), 64);
    out.resize(start + 2076, 0);
    if let Some(code) = game.ecco_code.as_deref().filter(|code| ecco_supported(code)) { out[start + 2036..start + 2039].copy_from_slice(code.as_bytes()); }
    out.push(match game.result { 1 => 1, 2 => 2, 3 => 3, _ => 0 });
    out.resize(start + 2112, 0);
    let (board, black) = position(game)?;
    out.push(if black { 2 } else { 1 });
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]);
    for piece in board { out.push(piece_code(piece)); }
    out.extend_from_slice(&[0, 0, 0, 0]);
    write_tree(out, game)?;
    Ok(())
}

fn write_tree(out: &mut Vec<u8>, game: &Game) -> Result<(), String> {
    if game.nodes.is_empty() { return Ok(()); }
    let mut pending = Vec::new();
    if let Some(comment) = game.nodes[0].comment.as_deref().filter(|value| !value.is_empty()) {
        if comment.contains('\0') { return Err("根注释含有空字符，无法完整写入 CBL".into()); }
        let size = comment.encode_utf16().count() * 2;
        out.extend_from_slice(&[if game.nodes[0].children.is_empty() { 5 } else { 4 }, 0, 0, 0]);
        out.extend_from_slice(&(size as u32).to_le_bytes());
        put_utf16(out, comment, size);
    }
    let count = game.nodes[0].children.len();
    for (i, &child) in game.nodes[0].children.iter().enumerate().rev() { pending.push((child as usize, i + 1 == count)); }
    while let Some((index, last_sibling)) = pending.pop() {
        let nodes = &game.nodes;
        let node = &nodes[index];
        if index != 0 {
            let comment = node.comment.as_deref().unwrap_or("");
            if comment.contains('\0') { return Err("着法注释含有空字符，无法完整写入 CBL".into()); }
            let mut sig = if node.children.is_empty() { 1 } else { 0 };
            if !last_sibling { sig |= 2; }
            if !comment.is_empty() { sig |= 4; }
            out.push(sig); out.push(0); out.push(node.from); out.push(node.to);
            if !comment.is_empty() {
                let size = comment.encode_utf16().count() * 2;
                out.extend_from_slice(&(size as u32).to_le_bytes());
                put_utf16(out, comment, size);
            }
        }
        let count = node.children.len();
        for (child_index, &child) in node.children.iter().enumerate().rev() { pending.push((child as usize, child_index + 1 == count)); }
    }
    Ok(())
}

fn put_utf16(out: &mut Vec<u8>, value: &str, max_bytes: usize) {
    let mut count = 0;
    for unit in value.encode_utf16() { if count + 2 > max_bytes { break; } out.extend_from_slice(&unit.to_le_bytes()); count += 2; }
}
fn put_utf16_at(out: &mut Vec<u8>, at: usize, value: &str, max_bytes: usize) {
    if out.len() < at + max_bytes { out.resize(at + max_bytes, 0); }
    let mut count = 0;
    for unit in value.encode_utf16() { if count + 2 > max_bytes { break; } out[at + count..at + count + 2].copy_from_slice(&unit.to_le_bytes()); count += 2; }
}
fn piece_code(piece: char) -> u8 { N2F.iter().position(|item| *item == piece).unwrap_or(0) as u8 }
#[cfg(test)]
#[path = "format_tests.rs"]
mod format_tests;

#[cfg(test)]
mod tests {

    use super::*;
    #[test]
    fn round_trips_players_time_site_and_variation() {
        let mut game = empty_game();
        game.title = "测试".into(); game.red_player = Some("红方".into()); game.black_player = Some("黑方".into());
        game.red_time = Some("5分".into()); game.site = Some("北京".into()); game.event = Some("杯赛".into()); game.result = 1; game.date = 20260101;
        game.start_fen = Some("rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w".into());
        game.nodes = vec![
            Node { from: 0, to: 0, parent: u32::MAX, children: vec![1], comment: Some("开局".into()) },
            Node { from: 82, to: 64, parent: 0, children: vec![2, 3], comment: None },
            Node { from: 1, to: 19, parent: 1, children: vec![], comment: Some("主变".into()) },
            Node { from: 7, to: 25, parent: 1, children: vec![], comment: None },
        ];
        let directory = std::env::temp_dir().join(format!("cbl-qyl-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("库.cbl");
        write(&path, "测试库", None, None, &[game.clone()]).unwrap();
        let loaded = read(&path).unwrap();
        assert_eq!(loaded.games[0].red_player.as_deref(), Some("红方"));
        assert_eq!(loaded.games[0].black_time, None);
        assert_eq!(loaded.games[0].site.as_deref(), Some("北京"));
        assert_eq!(loaded.games[0].nodes.len(), 4);
        assert_eq!(loaded.games[0].nodes[2].comment.as_deref(), Some("主变"));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn reads_legacy_players_site_and_move() {
        fn put(out: &mut Vec<u8>, text: &str) { let bytes = encoding_rs::GBK.encode(text).0; out.extend_from_slice(&(bytes.len() as u32).to_le_bytes()); out.extend_from_slice(&bytes); }
        let mut body = Vec::new();
        put(&mut body, "旧库"); put(&mut body, ""); put(&mut body, ""); put(&mut body, ""); put(&mut body, "");
        body.extend_from_slice(&[0; 38]);
        body.extend_from_slice(&1u32.to_le_bytes());
        put(&mut body, "");
        let mut record = Vec::new();
        record.extend_from_slice(&0u32.to_le_bytes());
        record.extend_from_slice(&[0x23]);
        record.extend_from_slice(&[0; 31]);
        record.extend_from_slice(&1u32.to_le_bytes());
        put(&mut record, "杯赛");
        record.extend_from_slice(&1u32.to_le_bytes());
        put(&mut record, "第一局"); put(&mut record, ""); put(&mut record, ""); put(&mut record, "上海"); put(&mut record, "2026-01-02");
        put(&mut record, "红方"); put(&mut record, "黑方");
        record.extend_from_slice(&1u32.to_le_bytes());
        put(&mut record, ""); put(&mut record, ""); put(&mut record, ""); put(&mut record, ""); put(&mut record, "");
        record.push(1);
        record.extend_from_slice(&[0; 38]);
        record.extend_from_slice(&1u32.to_le_bytes());
        record.extend_from_slice(&[1, 0, 0x12, 0x32, 0, 0]);
        record.extend_from_slice(&encoding_rs::GBK.encode("注释").0);
        record.extend_from_slice(b"\r\n");
        let len = record.len() as u32;
        record[..4].copy_from_slice(&len.to_le_bytes());
        body.extend_from_slice(&record);
        let mut file = b"CCBridge".to_vec();
        file.extend_from_slice(&0x15u32.to_le_bytes());
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        let directory = std::env::temp_dir().join(format!("cbl-legacy-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("旧.cbl");
        std::fs::write(&path, file).unwrap();
        let loaded = read(&path).unwrap();
        assert_eq!(loaded.games[0].red_player.as_deref(), Some("红方"));
        assert_eq!(loaded.games[0].site.as_deref(), Some("上海"));
        assert_eq!(loaded.games[0].date, 20260102);
        assert_eq!(loaded.games[0].nodes.len(), 2);
        assert_eq!(loaded.games[0].nodes[1].comment.as_deref(), Some("注释"));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
