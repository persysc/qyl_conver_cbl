//! FEN 文本（.txt）→ QYL 棋库。
//!
//! 每行一个局面：`<棋盘> <走子方> [ - - 0 1 ] [标题]`，棋盘为象棋 10 行 × 9 列，
//! 走子方支持 `w`/`r`/`red`/`红` 与 `b`/`black`/`黑`。解析结果写成一局局只有
//! 根节点的 QY 棋谱：`start_fen` 为起始局面，标题取行内文字，无文字时用「第N题」。

use crate::qy::{empty_game, Game, Node};
use std::collections::HashSet;
use std::path::Path;

const STANDARD_BOARD: &str = "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR";
const PIECES: &[u8] = b"rnbakcpRNBAKCP";

pub struct FenLibrary {
    pub name: String,
    pub games: Vec<Game>,
    pub warnings: Vec<String>,
}

/// 目录扫描时用：判断 .txt 是否像 FEN 题集（至少有一行能解析出棋盘串），
/// 避免把普通的说明文本当成棋库文件加进来。
pub fn looks_like_text(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else { return false; };
    if bytes.len() > 4 * 1024 * 1024 { return false; }
    let text = decode(&bytes);
    text.lines().take(500).any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with(';') && parse_line(trimmed).is_some()
    })
}

pub fn read(path: &Path) -> Result<FenLibrary, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("无法读取文件：{error}"))?;
    let text = decode(&bytes);
    let name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_else(|| "FEN 棋库".into());
    let mut games = Vec::new();
    let mut warnings = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut duplicates = 0usize;
    let mut bad = 0usize;
    let mut first_error: Option<String> = None;

    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((board, turn, title)) = parse_line(line) else {
            bad += 1;
            let note = format!("第 {} 行：找不到 10 行 × 9 列的棋盘串", index + 1);
            if first_error.is_none() { first_error = Some(note.clone()); }
            if warnings.len() < 20 { warnings.push(note); }
            continue;
        };
        let (fen, standard) = match normalize(&board, turn) {
            Ok(value) => value,
            Err(error) => {
                bad += 1;
                let note = format!("第 {} 行：{error}", index + 1);
                if first_error.is_none() { first_error = Some(note.clone()); }
                if warnings.len() < 20 { warnings.push(note); }
                continue;
            }
        };
        if !seen.insert(fen.clone()) {
            duplicates += 1;
            continue;
        }
        let mut game = empty_game();
        game.title = if title.is_empty() { format!("第{}题", games.len() + 1) } else { title };
        game.start_fen = if standard { None } else { Some(fen) };
        game.nodes = vec![Node { from: 0, to: 0, parent: u32::MAX, children: Vec::new(), comment: None }];
        games.push(game);
    }

    if games.is_empty() {
        let detail = first_error.map(|error| format!("（{error}）")).unwrap_or_default();
        return Err(format!("没有找到有效的 FEN 局面{detail}"));
    }
    if duplicates > 0 { warnings.insert(0, format!("去重 {duplicates} 个重复局面")); }
    if bad > 0 { warnings.insert(0, format!("跳过 {bad} 行无法解析的局面")); }
    Ok(FenLibrary { name, games, warnings })
}

fn decode(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.trim_start_matches('\u{feff}').to_string();
    }
    let (text, _, _) = encoding_rs::GB18030.decode(bytes);
    text.trim_start_matches('\u{feff}').to_string()
}

/// 返回（棋盘串, 走子方, 标题）。
fn parse_line(line: &str) -> Option<(String, char, String)> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut start = None;
    for (index, token) in tokens.iter().enumerate() {
        let board_like = token.split('/').count() == 10
            && token.chars().all(|ch| ch == '/' || ch.is_ascii_digit() || (ch.is_ascii_alphabetic() && PIECES.contains(&(ch as u8))));
        if board_like {
            start = Some(index);
            break;
        }
    }
    let start = start?;
    let board = tokens[start].to_string();
    let mut rest = start + 1;
    let turn = if let Some(value) = tokens.get(rest).and_then(|token| turn_of(token)) {
        rest += 1;
        value
    } else {
        'w'
    };
    let mut tail = rest;
    while tail < tokens.len() && tail - rest < 4 && is_fen_tail(tokens[tail], tail - rest) {
        tail += 1;
    }
    let mut parts = Vec::new();
    if start > 0 { parts.push(tokens[..start].join(" ")); }
    if tail < tokens.len() { parts.push(tokens[tail..].join(" ")); }
    let title = clean_title(&parts.join(" ")).unwrap_or_default();
    Some((board, turn, title))
}

fn turn_of(token: &str) -> Option<char> {
    match token.to_ascii_lowercase().as_str() {
        "w" | "r" | "red" => Some('w'),
        "b" | "black" => Some('b'),
        _ if token.starts_with('红') => Some('w'),
        _ if token.starts_with('黑') => Some('b'),
        _ => None,
    }
}

fn is_fen_tail(token: &str, index: usize) -> bool {
    let all_digits = !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit());
    if index < 2 { token == "-" || all_digits } else { all_digits }
}

fn clean_title(text: &str) -> Option<String> {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() { return None; }
    Some(joined.chars().take(200).collect())
}

/// 校验局面并返回（规范化 FEN, 是否标准开局）。
fn normalize(board: &str, turn: char) -> Result<(String, bool), String> {
    let rows: Vec<&str> = board.split('/').collect();
    if rows.len() != 10 { return Err("棋盘不是 10 行".into()); }
    let mut grid: Vec<Option<char>> = Vec::with_capacity(90);
    for row in &rows {
        let mut width = 0usize;
        for ch in row.chars() {
            if ch.is_ascii_digit() {
                let count = ch.to_digit(10).unwrap_or(0) as usize;
                if count == 0 { return Err("数字 0 不能出现在棋盘串里".into()); }
                grid.extend(std::iter::repeat(None).take(count));
                width += count;
            } else if ch.is_ascii_alphabetic() && PIECES.contains(&(ch as u8)) {
                grid.push(Some(ch));
                width += 1;
            } else {
                return Err(format!("非法字符 “{ch}”"));
            }
        }
        if width != 9 { return Err(format!("每行必须是 9 列，实际 {width} 列")); }
    }
    if grid.len() != 90 { return Err("棋盘不是 10 行 × 9 列".into()); }
    validate(&grid)?;
    let canonical = canonical_board(&grid);
    let fen = format!("{canonical} {}", if turn == 'b' { 'b' } else { 'w' });
    let standard = canonical == STANDARD_BOARD && turn != 'b';
    Ok((fen, standard))
}

fn canonical_board(grid: &[Option<char>]) -> String {
    let mut rows = Vec::with_capacity(10);
    for row in 0..10 {
        let mut text = String::new();
        let mut empty = 0usize;
        for column in 0..9 {
            match grid[row * 9 + column] {
                Some(piece) => {
                    if empty > 0 { text.push_str(&empty.to_string()); empty = 0; }
                    text.push(piece);
                }
                None => empty += 1,
            }
        }
        if empty > 0 { text.push_str(&empty.to_string()); }
        rows.push(text);
    }
    rows.join("/")
}

fn validate(grid: &[Option<char>]) -> Result<(), String> {
    let mut counts = [0usize; 128];
    for piece in grid.iter().flatten() { counts[*piece as usize] += 1; }
    let limit = |piece: char| match piece.to_ascii_lowercase() {
        'r' | 'n' | 'b' | 'a' | 'c' => 2,
        'p' => 5,
        'k' => 1,
        _ => 0,
    };
    for (index, count) in counts.iter().enumerate() {
        if *count == 0 { continue; }
        let piece = char::from_u32(index as u32).unwrap_or('?');
        if *count > limit(piece) { return Err(format!("{piece} 有 {count} 个，超过上限 {}", limit(piece))); }
    }
    if counts['K' as usize] != 1 { return Err("必须且只能有一个红帅".into()); }
    if counts['k' as usize] != 1 { return Err("必须且只能有一个黑将".into()); }

    for row in 0..10usize {
        for column in 0..9usize {
            let Some(piece) = grid[row * 9 + column] else { continue };
            let red = piece.is_ascii_uppercase();
            match piece.to_ascii_lowercase() {
                'k' => {
                    let in_palace = (3..=5).contains(&column) && if red { (7..=9).contains(&row) } else { row <= 2 };
                    if !in_palace { return Err(format!("将帅在 ({column},{row})，不在九宫")); }
                }
                'a' => {
                    if !advisor_points(red).contains(&(column, row)) { return Err(format!("士在 ({column},{row}) 非法位置")); }
                }
                'b' => {
                    if !elephant_points(red).contains(&(column, row)) { return Err(format!("象在 ({column},{row}) 非法位置")); }
                }
                'p' => {
                    if red && row > 6 { return Err("红兵退回了己方底线".into()); }
                    if !red && row < 3 { return Err("黑卒退回了己方底线".into()); }
                }
                _ => {}
            }
        }
    }

    let red_king = king_position(grid, 'K');
    let black_king = king_position(grid, 'k');
    if let (Some((rx, ry)), Some((bx, by))) = (red_king, black_king) {
        if rx == bx {
            let (from, to) = if ry < by { (ry + 1, by) } else { (by + 1, ry) };
            if (from..to).all(|row| grid[row * 9 + rx].is_none()) { return Err("将帅照面（同一列且中间无子）".into()); }
        }
    }
    Ok(())
}

fn king_position(grid: &[Option<char>], king: char) -> Option<(usize, usize)> {
    for index in 0..90 {
        if grid[index] == Some(king) {
            return Some((index % 9, index / 9));
        }
    }
    None
}

fn advisor_points(red: bool) -> [(usize, usize); 5] {
    if red { [(3, 9), (5, 9), (4, 8), (3, 7), (5, 7)] } else { [(3, 0), (5, 0), (4, 1), (3, 2), (5, 2)] }
}

fn elephant_points(red: bool) -> [(usize, usize); 7] {
    if red { [(2, 9), (6, 9), (0, 7), (4, 7), (8, 7), (2, 5), (6, 5)] } else { [(2, 0), (6, 0), (0, 2), (4, 2), (8, 2), (2, 4), (6, 4)] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_six_field_and_two_field_lines() {
        let (board, turn, title) = parse_line("3a5/3k5/5a3/9/3N5/9/8r/5KR2/9/9 w - - 0 1").unwrap();
        assert_eq!(board, "3a5/3k5/5a3/9/3N5/9/8r/5KR2/9/9");
        assert_eq!(turn, 'w');
        assert_eq!(title, "");
        let (_, turn, title) = parse_line("3a5/3k5/5a3/9/3N5/9/8r/5KR2/9/9 b 黑先巧胜01").unwrap();
        assert_eq!(turn, 'b');
        assert_eq!(title, "黑先巧胜01");
    }

    #[test]
    fn normalizes_dedupes_and_detects_standard_opening() {
        let mut seen = HashSet::new();
        let (fen, standard) = normalize("rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR", 'w').unwrap();
        assert!(standard, "{fen}");
        let (fen, standard) = normalize("rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR", 'b').unwrap();
        assert!(!standard);
        assert!(seen.insert(fen));
    }

    #[test]
    fn rejects_illegal_positions() {
        assert!(normalize("3k5/9/9/9/9/9/9/9/9/3K5", 'w').is_err(), "将帅照面");
        assert!(normalize("3k5/9/4A4/9/9/9/9/9/9/4K4", 'w').is_err(), "仕出九宫");
        assert!(normalize("3k5/9/9/9/9/9/9/9/P8/4K4", 'w').is_err(), "兵退底线");
        assert!(normalize("3k5/9/9 w", 'w').is_err(), "行数不足");
    }
}
