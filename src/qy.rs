use bincode::{config::standard, serde::{decode_from_slice, encode_into_std_write, encode_to_vec}};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"QYLY";
const VERSION: u16 = 1;
pub const MAX_DATA: usize = 512 * 1024 * 1024;
pub const STANDARD_FEN: &str = "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub version: u16, pub title: String, pub start_fen: Option<String>, pub nodes: Vec<Node>,
    pub red_player: Option<String>, pub black_player: Option<String>, pub red_team: Option<String>, pub black_team: Option<String>, pub result: u8,
    pub red_time: Option<String>, pub black_time: Option<String>, pub event: Option<String>, pub site: Option<String>, pub date: u64,
    pub round: Option<String>, pub table: Option<String>, pub referee: Option<String>, pub time_control: Option<String>, pub recorder: Option<String>,
    pub author: Option<String>, pub ecco_code: Option<String>, pub type_tag: Option<u8>, pub ecco: Option<u16>, pub position: u8,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node { pub from: u8, pub to: u8, pub parent: u32, pub children: Vec<u32>, pub comment: Option<Box<str>> }
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Info { name: String, author: Option<String>, description: Option<String>, #[serde(default)] game_count: u32, #[serde(default)] version: u16, #[serde(default)] created_at: u64, #[serde(default)] updated_at: u64 }
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry { title: String, red: Option<String>, black: Option<String>, result: u8, date: u64, offset: u64, length: u32 }
pub struct Library {
    pub path: PathBuf, pub name: String, pub author: Option<String>, pub description: Option<String>,
    pub created_at: u64, pub games: Vec<Game>, pub warnings: Vec<String>, pub dirty: bool,
}

pub fn empty_game() -> Game {
    Game { version: VERSION, title: String::new(), start_fen: None, nodes: Vec::new(), red_player: None, black_player: None, red_team: None, black_team: None, result: 0, red_time: None, black_time: None, event: None, site: None, date: 0, round: None, table: None, referee: None, time_control: None, recorder: None, author: None, ecco_code: None, type_tag: None, ecco: None, position: 0 }
}

// FEN 按顶行起算；QY 节点与清弈 Move 坐标一致，不使用 Board 的内部索引。
pub fn position(game: &Game) -> Result<([char; 90], bool), String> {
    let mut fields = game.start_fen.as_deref().unwrap_or(STANDARD_FEN).split_whitespace();
    let layout = fields.next().ok_or("缺少 FEN 局面")?;
    let black = match fields.next() { Some("b") => true, Some("w" | "r") => false, _ => return Err("FEN 轮走方不正确".into()) };
    let rows: Vec<_> = layout.split('/').collect();
    if rows.len() != 10 { return Err("FEN 必须有十行".into()); }
    let mut board = ['.'; 90];
    for (row, text) in rows.iter().enumerate() {
        let mut column = 0usize;
        for piece in text.chars() {
            if ('1'..='9').contains(&piece) { column += piece as usize - '0' as usize; }
            else {
                if !"RNBAKCPHErnbakcphe".contains(piece) || column >= 9 { return Err("FEN 棋子或列数不正确".into()); }
                board[row * 9 + column] = match piece { 'H' => 'N', 'h' => 'n', 'E' => 'B', 'e' => 'b', p => p };
                column += 1;
            }
            if column > 9 { return Err("FEN 列数超过九列".into()); }
        }
        if column != 9 { return Err("FEN 每行必须有九列".into()); }
    }
    Ok((board, black))
}

impl Game {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != VERSION { return Err(format!("不支持的 QY 版本 {}", self.version)); }
        if self.result > 3 || self.position > 1 || self.type_tag.is_some_and(|v| v > 4) { return Err("棋谱元信息枚举值不正确".into()); }
        position(self)?;
        if self.nodes.is_empty() { return Ok(()); }
        if self.nodes[0].parent != u32::MAX { return Err("根节点父索引不正确".into()); }
        let mut seen = vec![false; self.nodes.len()];
        let mut pending = vec![(0usize, u32::MAX)];
        while let Some((index, parent)) = pending.pop() {
            let node = self.nodes.get(index).ok_or("着法子索引越界")?;
            if seen[index] { return Err("着法树存在环或重复子节点".into()); }
            seen[index] = true;
            if node.parent != parent { return Err("着法父子索引不一致".into()); }
            if index != 0 && (node.from >= 90 || node.to >= 90 || node.from == node.to) { return Err("着法坐标不正确".into()); }
            for &child in node.children.iter().rev() { pending.push((child as usize, index as u32)); }
        }
        if seen.iter().any(|v| !v) { return Err("着法树存在不可达节点".into()); }
        Ok(())
    }
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    zstd::stream::read::Decoder::new(data).map_err(io)?.take(MAX_DATA as u64 + 1).read_to_end(&mut out).map_err(io)?;
    if out.len() > MAX_DATA { return Err("解压数据超过 512MB".into()); }
    Ok(out)
}
fn decode<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T, String> {
    let (value, used) = decode_from_slice(data, config()).map_err(io)?;
    if used != data.len() { return Err("QYL 数据含有未识别的尾部内容".into()); }
    Ok(value)
}
impl Library {
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(io)?;
        let size = file.metadata().map_err(io)?.len();
        if size < 14 || size > MAX_DATA as u64 { return Err("QYL 文件长度不正确或超过 512MB".into()); }
        let mut header = [0u8; 6]; file.read_exact(&mut header).map_err(io)?;
        if &header[..4] != MAGIC { return Err("文件魔数不匹配".into()); }
        let version = u16::from_le_bytes([header[4], header[5]]);
        if version != VERSION { return Err(format!("不支持的 QYL 版本 {version}")); }
        file.seek(SeekFrom::End(-8)).map_err(io)?;
        let mut footer = [0u8; 8]; file.read_exact(&mut footer).map_err(io)?;
        let index_offset = u64::from_le_bytes(footer);
        if index_offset < 6 || index_offset > size - 8 { return Err("QYL 索引损坏".into()); }
        file.seek(SeekFrom::Start(index_offset)).map_err(io)?;
        let mut compressed = vec![0u8; (size - index_offset - 8) as usize]; file.read_exact(&mut compressed).map_err(io)?;
        let index = decompress(&compressed)?;
        let (info, entries): (Info, Vec<Entry>) = decode(&index)?;
        let mut warnings = Vec::new();
        if entries.len() != info.game_count as usize { warnings.push(format!("目录实际为 {} 局，库信息记录为 {} 局，以目录为准", entries.len(), info.game_count)); }
        let mut games = Vec::with_capacity(entries.len());
        let mut total = index.len();
        for (ordinal, entry) in entries.into_iter().enumerate() {
            let result = (|| {
                if entry.offset < 6 || entry.offset > index_offset || u64::from(entry.length) > index_offset - entry.offset { return Err("棋谱数据范围越界".into()); }
                file.seek(SeekFrom::Start(entry.offset)).map_err(io)?;
                let mut raw = vec![0u8; entry.length as usize]; file.read_exact(&mut raw).map_err(io)?;
                let compressed: Vec<u8> = decode(&raw)?;
                let game_bin = decompress(&compressed)?;
                total = total.checked_add(game_bin.len()).ok_or("棋库数据过大")?;
                if total > MAX_DATA { return Err("棋库解压总量超过 512MB".into()); }
                let mut game: Game = decode(&game_bin)?;
                game.validate()?;
                if entry.title != game.title {
                    warnings.push(format!("第 {} 局目录标题“{}”与正文标题“{}”不同，采用清弈列表中的目录标题", ordinal + 1, entry.title, game.title));
                    game.title = entry.title;
                }
                if entry.red != game.red_player || entry.black != game.black_player || entry.result != game.result || entry.date != game.date {
                    warnings.push(format!("第 {} 局“{}”目录元信息与正文不同，棋手、结果、日期采用正文", ordinal + 1, game.title));
                }
                Ok(game)
            })();
            games.push(result.map_err(|e: String| format!("第 {} 局：{e}", ordinal + 1))?);
        }
        Ok(Self { path: path.to_path_buf(), name: info.name, author: info.author, description: info.description, created_at: info.created_at, games, warnings, dirty: false })
    }

    pub fn save(&mut self) -> Result<(), String> {
        for (i, game) in self.games.iter().enumerate() { game.validate().map_err(|e| format!("第 {} 局“{}”：{e}", i + 1, game.title))?; }
        // 与清弈按输入顺序分配唯一标题；原标题的缩进空白保留。
        let mut used = HashSet::new();
        for (i, game) in self.games.iter_mut().enumerate() {
            if game.title.trim().is_empty() {
                game.title = format!("{} 第{}局", self.name, i + 1);
                self.warnings.push(format!("第 {} 局空标题调整为“{}”", i + 1, game.title));
                self.dirty = true;
            }
            if used.insert(game.title.clone()) { continue; }
            let base = game.title.clone();
            let mut number = 2usize;
            loop {
                let title = format!("{base} ({number})");
                number += 1;
                if used.insert(title.clone()) {
                    self.warnings.push(format!("第 {} 局重名标题“{base}”调整为“{title}”", i + 1));
                    game.title = title;
                    self.dirty = true;
                    break;
                }
            }
        }
        let temp = self.path.with_extension("qyl.tmp");
        let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp).map_err(io)?;
        let result: Result<(), String> = (|| {
            {
                let mut writer = BufWriter::new(file);
                writer.write_all(MAGIC).and_then(|_| writer.write_all(&VERSION.to_le_bytes())).map_err(io)?;
                let mut entries = Vec::with_capacity(self.games.len());
                for game in &self.games {
                    let start = writer.stream_position().map_err(io)?;
                    let compressed = zstd::bulk::compress(&encode_to_vec(game, config()).map_err(io)?, 3).map_err(io)?;
                    encode_into_std_write(&compressed, &mut writer, config()).map_err(io)?;
                    let end = writer.stream_position().map_err(io)?;
                    entries.push(Entry { title: game.title.clone(), red: game.red_player.clone(), black: game.black_player.clone(), result: game.result, date: game.date, offset: start, length: u32::try_from(end - start).map_err(|_| "单局棋谱超过 4GB".to_string())? });
                }
                let index_offset = writer.stream_position().map_err(io)?;
                let info = Info { name: self.name.clone(), author: self.author.clone(), description: self.description.clone(), game_count: u32::try_from(self.games.len()).map_err(io)?, version: VERSION, created_at: self.created_at, updated_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.as_secs()).unwrap_or(0) };
                let index = zstd::bulk::compress(&encode_to_vec(&(info, entries), config()).map_err(io)?, 3).map_err(io)?;
                writer.write_all(&index).and_then(|_| writer.write_all(&index_offset.to_le_bytes())).and_then(|_| writer.flush()).map_err(io)?;
                writer.get_ref().sync_all().map_err(io)?;
            }
            let saved = Self::load(&temp)?;
            if saved.games != self.games || saved.name != self.name || saved.author != self.author || saved.description != self.description || saved.created_at != self.created_at || !saved.warnings.is_empty() { return Err("QYL 保存后内容校验失败".into()); }
            std::fs::hard_link(&temp, &self.path).map_err(|e| format!("目标文件已存在或无法创建，未覆盖：{e}"))?;
            let _ = std::fs::remove_file(&temp);
            Ok(())
        })();
        if result.is_err() { let _ = std::fs::remove_file(&temp); }
        result?;
        self.dirty = false;
        Ok(())
    }
}
fn config() -> impl bincode::config::Config { standard().with_fixed_int_encoding().with_little_endian().with_limit::<MAX_DATA>() }
fn io(error: impl ToString) -> String { error.to_string() }

#[cfg(test)]
mod tests {
    use super::*;
    fn library(path: PathBuf) -> Library { Library { path, name: "测试库".into(), author: Some("作者".into()), description: Some("说明".into()), created_at: 7, games: vec![], warnings: vec![], dirty: true } }
    #[test]
    fn preserves_all_fields_and_library_metadata() {
        let dir = std::env::temp_dir().join(format!("qyl-preserve-{}", std::process::id())); std::fs::create_dir_all(&dir).unwrap();
        let mut lib = library(dir.join("库.qyl"));
        let mut game = empty_game(); game.title = "甲𠮷".into(); game.red_player = Some("红".into()); game.black_player = Some("黑".into());
        game.red_time = Some("5分".into()); game.black_time = Some("6分".into()); game.event = Some("赛事".into()); game.site = Some("地点".into()); game.table = Some("台次".into()); game.time_control = Some("规则".into()); game.position = 1; game.type_tag = Some(4); game.ecco = Some(1); game.ecco_code = Some("A01".into());
        game.nodes = vec![Node { from: 0, to: 0, parent: u32::MAX, children: vec![1], comment: Some("根🐴".into()) }, Node { from: 82, to: 65, parent: 0, children: vec![], comment: Some(" ".into()) }];
        lib.games = vec![game.clone(), empty_game()]; lib.games.swap(0, 1); lib.games[1].title.push('乙'); lib.save().unwrap();
        let saved = Library::load(&lib.path).unwrap(); assert_eq!(saved.games, lib.games); assert_eq!(saved.author, lib.author); assert_eq!(saved.description, lib.description); assert_eq!(saved.created_at, 7);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn saving_assigns_unique_titles_and_keeps_directory_body_and_game_data_consistent() {
        let dir = std::env::temp_dir().join(format!("qyl-unique-{}", std::process::id())); std::fs::create_dir_all(&dir).unwrap();
        let mut lib = library(dir.join("unique.qyl"));
        for (i, title) in ["联赛", "联赛", "联赛 (2)", "联赛", "联赛 (2)", "", "  ", " 独立标题 "].iter().enumerate() {
            let mut game = empty_game(); game.title = (*title).into(); game.red_player = Some(format!("棋手{i}"));
            game.nodes = vec![Node { from: 0, to: 0, parent: u32::MAX, children: vec![], comment: Some(format!("注释{i}").into()) }];
            lib.games.push(game);
        }
        let original = lib.games.clone();
        lib.save().unwrap();
        let titles: Vec<_> = lib.games.iter().map(|game| game.title.as_str()).collect();
        assert_eq!(titles, ["联赛", "联赛 (2)", "联赛 (2) (2)", "联赛 (3)", "联赛 (2) (3)", "测试库 第6局", "测试库 第7局", " 独立标题 "]);
        assert_eq!(lib.warnings.len(), 6);
        assert!(lib.warnings[0].contains("第 2 局重名标题“联赛”调整为“联赛 (2)”"));
        let saved = Library::load(&lib.path).unwrap();
        assert_eq!(saved.games, lib.games);
        assert!(saved.warnings.is_empty(), "目录标题与正文标题必须一致");
        for (before, after) in original.iter().zip(&saved.games) {
            let mut expected = before.clone(); expected.title = after.title.clone();
            assert_eq!(&expected, after, "只允许调整重名标题，不得改动棋手、注释或着法");
        }
        let bytes = std::fs::read(&lib.path).unwrap();
        let offset = u64::from_le_bytes(bytes[bytes.len() - 8..].try_into().unwrap()) as usize;
        let (_, entries): (Info, Vec<Entry>) = decode(&decompress(&bytes[offset..bytes.len() - 8]).unwrap()).unwrap();
        let mut seen = HashSet::new();
        for (entry, game) in entries.iter().zip(&saved.games) {
            assert!(seen.insert(&entry.title));
            assert_eq!(entry.title, game.title);
        }
        lib.path = dir.join("again.qyl"); lib.save().unwrap();
        assert_eq!(Library::load(&lib.path).unwrap().games, saved.games, "再次保存不能继续加编号");
        assert_eq!(lib.warnings.len(), 6);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn index_title_is_authoritative_and_other_metadata_uses_body() {
        let dir = std::env::temp_dir().join(format!("qyl-index-{}", std::process::id())); std::fs::create_dir_all(&dir).unwrap();
        let mut lib = library(dir.join("index.qyl")); let mut game = empty_game(); game.title = "正文旧标题".into(); game.red_player = Some("正文棋手".into()); lib.games.push(game); lib.save().unwrap();
        let mut bytes = std::fs::read(&lib.path).unwrap(); let offset = u64::from_le_bytes(bytes[bytes.len()-8..].try_into().unwrap()) as usize;
        let (mut info, mut entries): (Info, Vec<Entry>) = decode(&decompress(&bytes[offset..bytes.len()-8]).unwrap()).unwrap();
        entries[0].title = "目录新标题".into(); entries[0].red = Some("旧棋手".into()); info.game_count = 99;
        bytes.truncate(offset); bytes.extend(zstd::bulk::compress(&encode_to_vec(&(info, entries), config()).unwrap(), 3).unwrap()); bytes.extend((offset as u64).to_le_bytes()); std::fs::write(&lib.path, &bytes).unwrap();
        let mut saved = Library::load(&lib.path).unwrap(); assert_eq!(saved.games[0].title, "目录新标题"); assert_eq!(saved.games[0].red_player.as_deref(), Some("正文棋手")); assert_eq!(saved.warnings.len(), 3); std::fs::remove_file(&saved.path).unwrap(); saved.save().unwrap(); assert!(Library::load(&lib.path).unwrap().warnings.is_empty());
        let before = std::fs::read(&saved.path).unwrap();
        assert!(saved.save().unwrap_err().contains("未覆盖"));
        assert_eq!(std::fs::read(&saved.path).unwrap(), before);
        let tmp = lib.path.with_extension("qyl.tmp"); std::fs::write(&tmp, b"other process").unwrap(); assert!(saved.save().is_err()); assert_eq!(std::fs::read(&tmp).unwrap(), b"other process");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rejects_bad_tree_fen_and_footer_without_panicking() {
        let mut game = empty_game(); game.nodes = vec![Node { from: 0, to: 0, parent: u32::MAX, children: vec![99], comment: None }]; assert!(game.validate().is_err());
        game.nodes[0].children = vec![0]; assert!(game.validate().is_err());
        game.nodes.clear(); game.start_fen = Some("FEN".into()); assert!(game.validate().is_err());
        let path = std::env::temp_dir().join(format!("bad-footer-{}.qyl", std::process::id())); let mut bytes = b"QYLY\x01\x00".to_vec(); bytes.extend(u64::MAX.to_le_bytes()); std::fs::write(&path, bytes).unwrap(); assert!(Library::load(&path).is_err()); std::fs::remove_file(path).unwrap();
    }
}
