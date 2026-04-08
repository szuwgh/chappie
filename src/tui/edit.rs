use crate::common::ring_vec::RingVec;
use crate::textwarp::CacheStr;
use crate::textwarp::LineParts;
use crate::textwarp::LineState;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use utf8_iter::Utf8CharsEx;
pub(crate) struct EditUI<'a> {
    txts: RingVec<CacheStr>,
    line_meta: RingVec<LineState>,
    line_spans: Vec<Span<'a>>,
}

trait GetNonControlLen {
    fn get_non_control_len(&self) -> usize;
}

impl GetNonControlLen for &[u8] {
    fn get_non_control_len(&self) -> usize {
        self.len()
    }
}

// 获取字符串中前 n 个非控制字符的位置，返回前三部分字符串及最后一个非控制字符的字节大小
fn n_chars_skip_control_mem_opt(s: &[u8], n: usize) -> (&[u8], &[u8], &[u8], usize) {
    let mut count = 0;
    let mut start_idx = None;
    let mut end_idx = None;
    let mut last_start_idx = None;
    let slen = s.get_non_control_len();
    // 去掉多余的 .enumerate()：idx 从未被使用，直接取 char_indices() 的 (byte_index, ch)
    for (byte_index, ch) in s.char_indices() {
        // if ch.is_control() {
        //     continue;
        // }
        if n > 0 && count == n - 1 {
            //8
            last_start_idx = Some(byte_index);
        }
        if count == n {
            // 第 n 个非控制字符
            start_idx = Some(byte_index);
        }
        if count == n + 1 {
            // 第 n+1 个非控制字符
            end_idx = Some(byte_index);
            break;
        }
        count += 1;
    }
    // 如果 never set, 默认到末尾
    let last_start = last_start_idx.unwrap_or_else(|| 0);
    let start = start_idx.unwrap_or_else(|| slen);
    let end = end_idx.unwrap_or_else(|| slen);

    (&s[..start], &s[start..end], &s[end..], start - last_start)
}

/// 单次字符遍历，直接返回字符范围 [char_start, char_end) 对应的可见字节切片（至多 2 段）。
///
/// # 优化说明
/// 原先两步：char_range_to_byte_range（扫描一遍求字节边界）+
///           slice_parts_range（再扫描一遍切取子切片）。
/// 本函数融合两步：在遍历中同时记录 char_start 所在位置，
/// 到达 char_end 时即刻提取切片返回，字节只扫描一遍，节省约 50% 扫描量。
///
/// 可见区域跨越 3+ 段时取前两段（与原 slice_parts_range 行为一致）。
fn char_range_to_visible<'a>(
    parts: &[&'a [u8]],
    char_start: usize,
    char_end: usize,
) -> [&'a [u8]; 2] {
    let mut char_count = 0usize;
    // char_start 所在的 part 索引及其在该 part 内的字节偏移
    let mut start_pi = 0usize;
    let mut start_byte = 0usize;
    let mut found_start = false;

    for (pi, part) in parts.iter().enumerate() {
        for (byte_idx, _) in part.char_indices() {
            // 记录 char_start 位置（仅记录一次）
            if !found_start && char_count == char_start {
                start_pi = pi;
                start_byte = byte_idx;
                found_start = true;
            }
            // 到达 char_end：立即提取可见切片并返回
            if char_count == char_end {
                if !found_start {
                    return [b"", b""];
                }
                return if start_pi == pi {
                    // 起止在同一 part：单段切片
                    [&part[start_byte..byte_idx], b""]
                } else if start_pi + 1 == pi {
                    // 跨相邻两个 part
                    [&parts[start_pi][start_byte..], &part[..byte_idx]]
                } else {
                    // 跨 3+ 个 part：取前两段（与原 slice_parts_range 行为一致）
                    [&parts[start_pi][start_byte..], parts[start_pi + 1]]
                };
            }
            char_count += 1;
        }
    }
    // char_end 超出文本末尾
    if !found_start || parts.is_empty() {
        return [b"", b""];
    }
    let last_pi = parts.len() - 1;
    if start_pi == last_pi {
        [&parts[start_pi][start_byte..], b""]
    } else {
        [&parts[start_pi][start_byte..], parts[start_pi + 1]]
    }
}

pub(crate) fn get_edit_content<'a>(
    txts: &'a RingVec<CacheStr>,
    with: usize,
    line_meta: &'a RingVec<LineState>,
    cur_line: usize,
    select_line: &Option<(usize, usize)>,
    height: usize,
    column_offset: usize,
    cursor_y: usize,
    cursor_x: usize,
) -> (Text<'a>, Text<'a>, usize, usize) {
    assert!(txts.len() == line_meta.len());
    let mut lines = Vec::with_capacity(line_meta.len());
    let mut byte_cursor: usize = 0; //bytes的索引 表示光标在多少个u8
    let mut prev_char_bytes_size: usize = 0; //获取上一个字符bytes大小用来做删除操作
    for (i, txt) in txts.iter().enumerate() {
        //一行数据可能会分成很多个块
        // 单次字符遍历求可见切片（融合原 char_range_to_byte_range + slice_parts_range）
        let full = txt.text(0..);
        let visible = char_range_to_visible(full.as_parts(), column_offset, column_offset + with);
        let parts: &[&[u8]] = &visible[..];
        if cursor_y == i {
            //取上一行的最后一个字符char 大小
            let mut prev_line_last_char_size = 0;
            if cursor_x == 0 {
                if i > 0 {
                    let is_new_logical_line = line_meta
                        .get(i)
                        .zip(line_meta.get(i - 1))
                        .map(|(cur, prev)| cur.get_line_index() != prev.get_line_index())
                        .unwrap_or(false);
                    if is_new_logical_line {
                        prev_line_last_char_size = 1;
                    } else if let Some(prev_txt) = txts.get(i - 1) {
                        let prev_t = prev_txt.text(0..);
                        let prev_parts = prev_t.as_parts();
                        // 当前视觉行是上一逻辑行的续段时，行首退格应删除上一段末尾字符，
                        // 不是逻辑换行符，因此继续取上一段最后一个非控制字符的字节大小。
                        prev_line_last_char_size = prev_parts
                            .iter()
                            .rev()
                            .find_map(|s| {
                                s.char_indices()
                                    .rev()
                                    .find(|(_, ch)| !ch.is_control())
                                    .map(|(_, ch)| ch.len_utf8())
                            })
                            .unwrap_or(0);
                    }
                }
            }
            // 计算各 part 字符数，同时确定光标所在段；合并为单次遍历
            let mut char_count = LineParts::<usize>::empty();
            let mut char_curosr_index = 0;
            let mut char_sum_count = 0;
            let mut cursor_part_found = false;
            for (idx, s) in parts.iter().enumerate() {
                let count = s.chars().count();
                char_count.append(count);
                if !cursor_part_found {
                    char_sum_count += count;
                    if char_sum_count > 0 && cursor_x <= char_sum_count.saturating_sub(1) {
                        char_curosr_index = idx;
                        cursor_part_found = true;
                    }
                }
            }
            let (spans, byte_pos, last_csz) =
                build_cursor_line(parts, cursor_x, char_count.as_parts(), char_curosr_index);
            byte_cursor = byte_pos;
            if cursor_x > 0 {
                prev_char_bytes_size = last_csz;
            } else {
                prev_char_bytes_size = prev_line_last_char_size;
            }
            lines.push(Line::from(spans));
        } else {
            // 优化B：str::from_utf8 直接借用原始字节，避免 from_utf8_lossy 堆分配。
            // 优化5：visible 固定 2 段，直接按索引构造 Span，预分配容量 2，
            // 避免 map().collect() 的迭代器包装和动态扩容开销。
            let mut spans = Vec::with_capacity(2);
            spans.push(Span::raw(str::from_utf8(visible[0]).unwrap_or("")));
            spans.push(Span::raw(str::from_utf8(visible[1]).unwrap_or("")));
            lines.push(Line::from(spans));
        }
    }
    append_padding_lines(&mut lines, cursor_y, cursor_x, line_meta.len());
    let nav_text = build_nav_text(line_meta, height);
    let text = Text::from(lines);
    (nav_text, text, byte_cursor, prev_char_bytes_size)
}

fn build_cursor_line<'a>(
    str_parts: &[&'a [u8]],
    cursor_x: usize,
    char_count: &[usize],
    char_curosr_index: usize, //判断光标在那个 part中
) -> (Vec<Span<'a>>, usize, usize) {
    //let (str1, str2) = txt.text(offset..);
    // 优化2：预分配容量：最多 str_parts.len() 个普通段 + 3 个光标相关 span（前段/光标/后段）
    let mut spans = Vec::with_capacity(str_parts.len() + 3);
    let mut last_char_bytes_size: usize = 0;
    let byte_cursor;
    let (a, b, c) = if char_curosr_index == 0 {
        let (a, b, c, last_csz) = n_chars_skip_control_mem_opt(str_parts[0], cursor_x);
        last_char_bytes_size = last_csz;
        (a, b, c)
    } else {
        // 优化2：前几段字符数之和会被用两次，提前计算缓存，避免重复 .iter().sum()
        let prev_char_sum: usize = char_count[..char_curosr_index].iter().sum();
        let (a, b, c, last_csz) = n_chars_skip_control_mem_opt(
            str_parts[char_curosr_index],
            cursor_x.saturating_sub(prev_char_sum),
        );
        //如果上一个字符大小是0则光标可能在第一个字符那里
        if last_csz == 0 {
            let (_, _, _, sz) =
                n_chars_skip_control_mem_opt(str_parts[char_curosr_index - 1], prev_char_sum);
            last_char_bytes_size = sz;
        } else {
            last_char_bytes_size = last_csz;
        }
        (a, b, c)
    };
    if b.len() > 0 {
        byte_cursor = if char_curosr_index == 0 {
            a.get_non_control_len()
        } else {
            // 优化3：原来先推入 spans（遍历一次），再 fold 累加字节（遍历第二次），
            // 合并为单次循环：推入 span 的同时累加字节数
            let mut prefix_bytes = 0usize;
            for j in 0..char_curosr_index {
                let part = str_parts[j];
                prefix_bytes += part.len();
                spans.push(Span::raw(str::from_utf8(part).unwrap_or("")));
            }
            prefix_bytes + a.get_non_control_len()
        };
        let (display, color) = if b == b"\n" {
            (" ", Color::LightBlue)
        } else {
            (str::from_utf8(b).unwrap_or(""), Color::LightRed)
        };

        spans.push(Span::raw(str::from_utf8(a).unwrap_or("")));
        spans.push(Span::styled(display, Style::default().bg(color)));
        spans.push(Span::raw(str::from_utf8(c).unwrap_or("")));
        for j in (char_curosr_index + 1)..str_parts.len() {
            spans.push(Span::raw(str::from_utf8(str_parts[j]).unwrap_or("")));
        }
    } else {
        byte_cursor = if char_curosr_index == 0 {
            str_parts[0].get_non_control_len()
        } else {
            // 空切片 len()==0 不影响 fold 结果，filter 多余，直接删除
            str_parts[..char_curosr_index]
                .iter()
                .fold(0, |acc, s| acc + s.len())
                + str_parts[char_curosr_index].get_non_control_len()
        };
        for j in 0..str_parts.len() {
            spans.push(Span::raw(str::from_utf8(str_parts[j]).unwrap_or("")));
        }
        let diff = cursor_x.saturating_sub(char_count.iter().sum());
        let padding = " ".repeat(diff);
        spans.push(Span::raw(padding));
        // 在填充后显示高亮的光标
        spans.push(Span::styled(" ", Style::default().bg(Color::LightRed)));
    }
    (spans, byte_cursor, last_char_bytes_size)
}

/// 为超出文本范围的光标添加空行和光标显示
fn append_padding_lines(
    lines: &mut Vec<Line>,
    cursor_y: usize,
    cursor_x: usize,
    line_meta_len: usize,
) {
    if cursor_y >= line_meta_len {
        let diff = cursor_y.saturating_sub(line_meta_len);
        for _ in 0..diff {
            lines.push(Line::raw(""));
        }
        let mut spans = Vec::new();
        let padding = " ".repeat(cursor_x);
        spans.push(Span::raw(padding));
        // 在填充后显示高亮的光标
        spans.push(Span::styled(" ", Style::default().bg(Color::LightRed)));
        lines.push(Line::from(spans));
    }
}

/// 构建行号导航文本
fn build_nav_text(line_meta: &RingVec<LineState>, height: usize) -> Text<'_> {
    let nav_lines: Vec<Line> = (0..height)
        .map(|i| {
            line_meta.get(i).map_or_else(
                || Line::raw(""),
                |meta| {
                    if meta.get_line_offset() > 0 {
                        Line::raw("")
                    } else {
                        Line::from(Span::styled(
                            format!("{:>4} ", meta.get_line_num()),
                            Style::default().fg(Color::White),
                        ))
                    }
                },
            )
        })
        .collect();
    Text::from(nav_lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ring_vec::RingVec;
    use crate::handle::{Handle, HandleEdit};
    use crate::textwarp::edit_block::GapBlockText;
    use crate::textwarp::CacheStr;
    use crate::textwarp::EditTextWarp;
    use crate::textwarp::LineState;
    use crate::textwarp::LineStateBuilder;
    use crate::textwarp::TextDisplay;
    use crate::textwarp::TextOper;
    use crate::textwarp::TextWarpType;
    use crate::tui::ChapTui;
    use crate::undo::undo::UndoFile;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use tempfile::TempDir;

    // ── 构造辅助 ──────────────────────────────────────────────────────────────

    fn make_meta(line_num: usize) -> LineState {
        LineStateBuilder::new().line_num(line_num).build()
    }

    fn make_ring_meta(line_nums: &[usize]) -> RingVec<LineState> {
        let mut rv = RingVec::with_capacity(line_nums.len().max(1));
        for &n in line_nums {
            rv.push(make_meta(n));
        }
        rv
    }

    fn make_meta_with_offsets(line_num: usize, line_offset: usize) -> LineState {
        LineStateBuilder::new()
            .line_num(line_num)
            .line_offset(line_offset)
            .build()
    }

    fn make_ring_txt(lines: Vec<&str>) -> RingVec<CacheStr> {
        let mut rv = RingVec::with_capacity(lines.len().max(1));
        for s in lines {
            rv.push(CacheStr::from_vec_for_test(s.as_bytes().to_vec()));
        }
        rv
    }

    fn setup_with_undo(content: &str) -> (ChapTui, TextDisplay, NamedTempFile, TempDir) {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(content.as_bytes()).unwrap();
        tmp.flush().unwrap();

        let gap = GapBlockText::from_file_path(tmp.path()).unwrap();
        let mut td = TextDisplay::EditBlock(EditTextWarp::new(gap, 20, 80, TextWarpType::SoftWrap));
        td.get_one_page(1).unwrap();

        let undo_dir = TempDir::new().unwrap();
        let undo_path = undo_dir.path().join("undo.chpu");
        let mut tui = ChapTui::for_test(20, 80);
        tui.undo = Some(UndoFile::open(&undo_path).unwrap());
        (tui, td, tmp, undo_dir)
    }

    fn handle() -> HandleEdit {
        HandleEdit {}
    }

    fn saved_text(td: &mut TextDisplay) -> String {
        let tmp = NamedTempFile::new().unwrap();
        td.save(tmp.path()).unwrap();
        std::fs::read_to_string(tmp.path()).unwrap()
    }

    fn sync_cursor_metrics(tui: &mut ChapTui, td: &TextDisplay) {
        let (content, meta) = td.get_current_page().unwrap();
        let (_, _, byte_cursor, last_char_bytes_size) = get_edit_content(
            content,
            tui.elem.tv.get_width(),
            &meta,
            0,
            &None,
            tui.elem.tv.get_height(),
            tui.column_offset,
            tui.cursor_y,
            tui.cursor_x,
        );
        tui.bytes_cursor = byte_cursor;
        tui.bytes_cursor_size = last_char_bytes_size;
    }

    fn render_text_lines(text: &Text<'_>) -> Vec<String> {
        text.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    fn render_cursor_marks(text: &Text<'_>) -> Vec<(usize, usize, String, Option<Color>)> {
        text.lines
            .iter()
            .enumerate()
            .flat_map(|(line_idx, line)| {
                line.spans
                    .iter()
                    .enumerate()
                    .filter_map(move |(span_idx, span)| {
                        if span.style.bg.is_some() {
                            Some((
                                line_idx,
                                span_idx,
                                span.content.as_ref().to_string(),
                                span.style.bg,
                            ))
                        } else {
                            None
                        }
                    })
            })
            .collect()
    }

    fn render_snapshot(
        tui: &mut ChapTui,
        td: &TextDisplay,
    ) -> (
        Vec<String>,
        Vec<String>,
        Vec<(usize, usize, String, Option<Color>)>,
        usize,
        usize,
    ) {
        td.get_one_page(tui.start_line_num).unwrap();
        let (content, meta) = td.get_current_page().unwrap();
        let (nav, body, byte_cursor, last_char_bytes_size) = get_edit_content(
            content,
            tui.elem.tv.get_width(),
            &meta,
            0,
            &None,
            tui.elem.tv.get_height(),
            tui.column_offset,
            tui.cursor_y,
            tui.cursor_x,
        );
        (
            render_text_lines(&nav),
            render_text_lines(&body),
            render_cursor_marks(&body),
            byte_cursor,
            last_char_bytes_size,
        )
    }

    fn canonical_snapshot(
        tui: &mut ChapTui,
        td: &TextDisplay,
    ) -> (
        Vec<String>,
        Vec<String>,
        Vec<(usize, usize, String, Option<Color>)>,
        usize,
        usize,
    ) {
        set_viewport_and_cursor(tui, td, 1, 0, 0);
        render_snapshot(tui, td)
    }

    fn set_viewport_and_cursor(
        tui: &mut ChapTui,
        td: &TextDisplay,
        start_line_num: usize,
        cursor_y: usize,
        cursor_x: usize,
    ) {
        tui.start_line_num = start_line_num;
        tui.cursor_y = cursor_y;
        tui.cursor_x = cursor_x;
        td.get_one_page(tui.start_line_num).unwrap();
        sync_cursor_metrics(tui, td);
    }

    #[test]
    fn test_build_nav_text_hides_softwrap_continuation_numbers() {
        let mut meta = RingVec::with_capacity(3);
        meta.push(make_meta_with_offsets(10, 0));
        meta.push(make_meta_with_offsets(10, 5));
        meta.push(make_meta_with_offsets(11, 0));

        let nav = build_nav_text(&meta, 3);
        let rendered: Vec<String> = nav
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();

        assert_eq!(rendered[0], "  10 ");
        assert_eq!(rendered[1], "");
        assert_eq!(rendered[2], "  11 ");
    }

    // ── n_chars_skip_control_mem_opt ─────────────────────────────────────────

    #[test]
    fn test_n_chars_ascii_cursor_at_0() {
        // n=0：光标在第 0 个字符，a 为空，b 为 'h'，c 为 "ello"
        let (a, b, c, last_sz) = n_chars_skip_control_mem_opt(b"hello", 0);
        assert_eq!(a, b"");
        assert_eq!(b, b"h");
        assert_eq!(c, b"ello");
        // n=0 时 last_start_idx 从未设置，start-last_start = 0-0 = 0
        assert_eq!(last_sz, 0);
    }

    #[test]
    fn test_n_chars_ascii_cursor_middle() {
        // n=2：光标在 'l'(index=2)，a="he"，b="l"，c="lo"，前一字符 'e' 占 1 字节
        let (a, b, c, last_sz) = n_chars_skip_control_mem_opt(b"hello", 2);
        assert_eq!(a, b"he");
        assert_eq!(b, b"l");
        assert_eq!(c, b"lo");
        assert_eq!(last_sz, 1); // 'e' 占 1 字节
    }

    #[test]
    fn test_n_chars_ascii_cursor_at_last() {
        // n=4：光标在 'o'(index=4)，c 为空
        let (a, b, c, last_sz) = n_chars_skip_control_mem_opt(b"hello", 4);
        assert_eq!(a, b"hell");
        assert_eq!(b, b"o");
        assert_eq!(c, b"");
        assert_eq!(last_sz, 1); // 'l' 占 1 字节
    }

    #[test]
    fn test_n_chars_ascii_cursor_beyond_end() {
        // n=5：光标超出文本末尾，b 为空，走末尾光标分支
        let (a, b, c, _) = n_chars_skip_control_mem_opt(b"hello", 5);
        assert_eq!(a, b"hello");
        assert_eq!(b, b"");
        assert_eq!(c, b"");
    }

    #[test]
    fn test_n_chars_multibyte_cursor_at_0() {
        // "你好"：每个汉字 3 字节，光标在 "你"
        let s = "你好".as_bytes();
        let (a, b, c, last_sz) = n_chars_skip_control_mem_opt(s, 0);
        assert_eq!(a, b"");
        assert_eq!(b, "你".as_bytes());
        assert_eq!(c, "好".as_bytes());
        assert_eq!(last_sz, 0); // n=0 时 last_sz 为 0
    }

    #[test]
    fn test_n_chars_multibyte_cursor_at_1() {
        // "你好"：光标在 "好"，前一字符 "你" 占 3 字节
        let s = "你好".as_bytes();
        let (a, b, c, last_sz) = n_chars_skip_control_mem_opt(s, 1);
        assert_eq!(a, "你".as_bytes());
        assert_eq!(b, "好".as_bytes());
        assert_eq!(c, b"");
        assert_eq!(last_sz, 3); // "你" 占 3 字节
    }

    #[test]
    fn test_n_chars_newline_cursor() {
        // "he\nllo"：光标在 '\n'(index=2)，b = b"\n"
        let (a, b, c, last_sz) = n_chars_skip_control_mem_opt(b"he\nllo", 2);
        assert_eq!(a, b"he");
        assert_eq!(b, b"\n");
        assert_eq!(c, b"llo");
        assert_eq!(last_sz, 1); // 'e' 占 1 字节
    }

    #[test]
    fn test_n_chars_single_char() {
        // 只有一个字符，光标在它上面
        let (a, b, c, _) = n_chars_skip_control_mem_opt(b"x", 0);
        assert_eq!(a, b"");
        assert_eq!(b, b"x");
        assert_eq!(c, b"");
    }

    #[test]
    fn test_n_chars_empty_string() {
        // 空字符串，任意 n 都应返回空
        let (a, b, c, _) = n_chars_skip_control_mem_opt(b"", 0);
        assert_eq!(a, b"");
        assert_eq!(b, b"");
        assert_eq!(c, b"");
    }

    // ── build_cursor_line ────────────────────────────────────────────────────

    fn collect_text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn test_build_cursor_line_ascii_cursor_at_0() {
        // 单 part "hello"，光标在位置 0
        let parts: &[&[u8]] = &[b"hello"];
        let char_count = &[5usize];
        let (spans, byte_cursor, _) = build_cursor_line(parts, 0, char_count, 0);
        // 可见文本仍是 "hello"
        assert_eq!(collect_text(&spans), "hello");
        // 光标前 0 字节
        assert_eq!(byte_cursor, 0);
        // 'h' 应有高亮背景
        assert_eq!(spans[1].style.bg, Some(Color::LightRed));
        assert_eq!(spans[1].content.as_ref(), "h");
    }

    #[test]
    fn test_build_cursor_line_ascii_cursor_middle() {
        // 光标在位置 2（'l'）
        let parts: &[&[u8]] = &[b"hello"];
        let char_count = &[5usize];
        let (spans, byte_cursor, last_sz) = build_cursor_line(parts, 2, char_count, 0);
        assert_eq!(collect_text(&spans), "hello");
        assert_eq!(byte_cursor, 2);
        assert_eq!(last_sz, 1);
        assert_eq!(spans[1].style.bg, Some(Color::LightRed));
        assert_eq!(spans[1].content.as_ref(), "l");
    }

    #[test]
    fn test_build_cursor_line_newline_shows_blue() {
        // 光标落在 '\n' 上，应显示蓝色背景空格
        let parts: &[&[u8]] = &[b"hi\n"];
        let char_count = &[3usize];
        let (spans, _, _) = build_cursor_line(parts, 2, char_count, 0);
        let cursor_span = spans.iter().find(|s| s.style.bg == Some(Color::LightBlue));
        assert!(cursor_span.is_some(), "\\n 位置应显示蓝色背景");
        assert_eq!(cursor_span.unwrap().content.as_ref(), " ");
    }

    #[test]
    fn test_build_cursor_line_cursor_beyond_text() {
        // 光标超出文本末尾，应在末尾追加红色空格
        let parts: &[&[u8]] = &[b"hi"];
        let char_count = &[2usize];
        let (spans, _, _) = build_cursor_line(parts, 5, char_count, 0);
        let cursor_span = spans.iter().find(|s| s.style.bg == Some(Color::LightRed));
        assert!(cursor_span.is_some(), "超出范围光标应显示红色背景");
    }

    #[test]
    fn test_build_cursor_line_multibyte() {
        // "你好"，光标在 "好"（位置 1），byte_cursor 应为 3
        let parts: &[&[u8]] = &["你好".as_bytes()];
        let char_count = &[2usize];
        let (spans, byte_cursor, last_sz) = build_cursor_line(parts, 1, char_count, 0);
        assert_eq!(byte_cursor, 3); // "你" 占 3 字节
        assert_eq!(last_sz, 3); // 前一字符 "你" 占 3 字节
        let cursor_span = spans.iter().find(|s| s.style.bg == Some(Color::LightRed));
        assert!(cursor_span.is_some());
        assert_eq!(cursor_span.unwrap().content.as_ref(), "好");
    }

    // ── build_nav_text ────────────────────────────────────────────────────────

    #[test]
    fn test_build_nav_text_correct_line_numbers() {
        // 3 行内容 height=5，前 3 行有行号，后 2 行为空
        let meta = make_ring_meta(&[1, 2, 3]);
        let nav = build_nav_text(&meta, 5);
        assert_eq!(nav.lines.len(), 5);
        assert!(nav.lines[0].to_string().contains("1"));
        assert!(nav.lines[1].to_string().contains("2"));
        assert!(nav.lines[2].to_string().contains("3"));
        assert_eq!(nav.lines[3].to_string(), "");
        assert_eq!(nav.lines[4].to_string(), "");
    }

    #[test]
    fn test_build_nav_text_height_less_than_meta() {
        // height=2 < meta 行数，只渲染前 2 行
        let meta = make_ring_meta(&[10, 20, 30]);
        let nav = build_nav_text(&meta, 2);
        assert_eq!(nav.lines.len(), 2);
        assert!(nav.lines[0].to_string().contains("10"));
        assert!(nav.lines[1].to_string().contains("20"));
    }

    #[test]
    fn test_build_nav_text_empty_meta() {
        let meta = make_ring_meta(&[]);
        let nav = build_nav_text(&meta, 3);
        assert_eq!(nav.lines.len(), 3);
        for line in &nav.lines {
            assert_eq!(line.to_string(), "");
        }
    }

    // ── append_padding_lines ─────────────────────────────────────────────────

    #[test]
    fn test_append_padding_cursor_within_range() {
        // cursor_y < line_meta_len，不追加任何行
        let mut lines: Vec<Line> = vec![Line::raw("a"), Line::raw("b")];
        append_padding_lines(&mut lines, 1, 0, 3);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_append_padding_cursor_at_boundary() {
        // cursor_y == line_meta_len，追加 1 行光标
        let mut lines: Vec<Line> = vec![Line::raw("a"), Line::raw("b")];
        append_padding_lines(&mut lines, 2, 0, 2);
        assert_eq!(lines.len(), 3);
        // 最后一行含有红色光标
        let last = lines.last().unwrap();
        let has_cursor = last
            .spans
            .iter()
            .any(|s| s.style.bg == Some(Color::LightRed));
        assert!(has_cursor, "边界行应含红色光标");
    }

    #[test]
    fn test_append_padding_cursor_beyond_range() {
        // cursor_y=5, line_meta_len=3 → 追加 2 个空行 + 1 个光标行
        let mut lines: Vec<Line> = vec![Line::raw("a"), Line::raw("b"), Line::raw("c")];
        append_padding_lines(&mut lines, 5, 0, 3);
        assert_eq!(lines.len(), 6); // 3 + 2 空行 + 1 光标行
    }

    #[test]
    fn test_append_padding_cursor_x_offset() {
        // cursor_x=3，光标前应有 3 个空格
        let mut lines: Vec<Line> = vec![];
        append_padding_lines(&mut lines, 0, 3, 0);
        assert_eq!(lines.len(), 1);
        let padding_span = &lines[0].spans[0];
        assert_eq!(padding_span.content.as_ref(), "   ");
    }

    // ── get_edit_content 集成测试 ─────────────────────────────────────────────

    #[test]
    fn test_get_edit_content_byte_cursor_at_0() {
        // 单行 "hello"，光标在 (x=0, y=0)
        let txts = make_ring_txt(vec!["hello"]);
        let meta = make_ring_meta(&[1]);
        let (_, content, byte_cursor, _) =
            get_edit_content(&txts, 80, &meta, 0, &None, 10, 0, 0, 0);
        assert_eq!(byte_cursor, 0);
        // 文本内容应可见
        let text = content.to_string();
        assert!(
            text.contains("hello") || text.contains("h"),
            "内容应含 hello"
        );
    }

    #[test]
    fn test_get_edit_content_byte_cursor_middle() {
        // "hello"，光标在 (x=2, y=0)，byte_cursor 应为 2
        let txts = make_ring_txt(vec!["hello"]);
        let meta = make_ring_meta(&[1]);
        let (_, _, byte_cursor, last_sz) =
            get_edit_content(&txts, 80, &meta, 0, &None, 10, 0, 0, 2);
        assert_eq!(byte_cursor, 2);
        assert_eq!(last_sz, 1); // 前一字符 'e' 占 1 字节
    }

    #[test]
    fn test_get_edit_content_multibyte_byte_cursor() {
        // "你好世界"，光标在 (x=2, y=0)，byte_cursor 应为 6（2个汉字 × 3字节）
        let txts = make_ring_txt(vec!["你好世界"]);
        let meta = make_ring_meta(&[1]);
        let (_, _, byte_cursor, last_sz) =
            get_edit_content(&txts, 80, &meta, 0, &None, 10, 0, 0, 2);
        assert_eq!(byte_cursor, 6);
        assert_eq!(last_sz, 3); // 前一字符 "好" 占 3 字节
    }

    #[test]
    fn test_get_edit_content_multiline_nav() {
        // 3 行文本，行号导航应正确
        let txts = make_ring_txt(vec!["line1", "line2", "line3"]);
        let meta = make_ring_meta(&[1, 2, 3]);
        let (nav, _, _, _) = get_edit_content(&txts, 80, &meta, 0, &None, 5, 0, 0, 0);
        assert!(nav.to_string().contains("1"));
        assert!(nav.to_string().contains("2"));
        assert!(nav.to_string().contains("3"));
    }

    #[test]
    fn test_get_edit_content_cursor_beyond_lines() {
        // 光标行 cursor_y=3 超出 meta 范围（只有 2 行），应追加 padding
        let txts = make_ring_txt(vec!["a", "b"]);
        let meta = make_ring_meta(&[1, 2]);
        let (_, content, _, _) = get_edit_content(&txts, 80, &meta, 0, &None, 10, 0, 3, 0);
        // 应有超过 2 行的渲染输出（含 padding 行）
        assert!(content.lines.len() >= 3);
    }

    #[test]
    fn test_ui_render_restores_after_500_mixed_ops_and_reverse_undo() {
        let original = format!(
            "{}tail\n",
            "line-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n".repeat(520)
        );
        let (mut tui, mut td, _f, _uf) = setup_with_undo(&original);
        let h = handle();

        let initial_snapshot = render_snapshot(&mut tui, &td);
        let initial_text = saved_text(&mut td);
        assert_eq!(initial_text, original);

        for step in 0..500usize {
            td.get_one_page(1).unwrap();
            let meta = td.get_current_line_meta().unwrap();
            match step % 4 {
                0 => {
                    tui.start_line_num = 1;
                    tui.cursor_y = 0;
                    tui.cursor_x = 0;
                    tui.bytes_cursor = 0;
                    tui.bytes_cursor_size = 0;
                    let ch = if step % 8 == 0 { '你' } else { 'A' };
                    h.handle_char(&mut tui, meta, &td, ch).unwrap();
                }
                1 => {
                    tui.start_line_num = 1;
                    tui.cursor_y = 0;
                    tui.cursor_x = 0;
                    tui.bytes_cursor = 0;
                    tui.bytes_cursor_size = 0;
                    let paste = if step % 12 == 1 {
                        format!("mix-{step}\n中文-{step}")
                    } else if step % 10 == 1 {
                        format!("段落-{step}\nnext-{step}\nend")
                    } else {
                        format!("p{step:03}-你")
                    };
                    h.handle_paste(&mut tui, meta, &td, &paste).unwrap();
                }
                2 => {
                    tui.start_line_num = 1;
                    tui.cursor_y = 0;
                    tui.cursor_x = 1;
                    td.get_one_page(tui.start_line_num).unwrap();
                    sync_cursor_metrics(&mut tui, &td);
                    let meta = td.get_current_line_meta().unwrap();
                    h.handle_enter(&mut tui, meta, &td).unwrap();
                }
                3 => {
                    tui.start_line_num = 1;
                    tui.cursor_y = 0;
                    tui.cursor_x = 1;
                    td.get_one_page(tui.start_line_num).unwrap();
                    sync_cursor_metrics(&mut tui, &td);
                    let meta = td.get_current_line_meta().unwrap();
                    h.handle_backspace(&mut tui, meta, &td).unwrap();
                }
                _ => unreachable!(),
            }

            if let TextDisplay::EditBlock(v) = &td {
                v.assert_block_storage_consistent_for_test();
            }
            if step % 50 == 49 {
                let snapshot = render_snapshot(&mut tui, &td);
                assert!(!snapshot.1.is_empty(), "UI 正文渲染不应为空，step={step}");
                assert!(!snapshot.2.is_empty(), "UI 光标高亮不应丢失，step={step}");
            }
        }

        let changed_snapshot = render_snapshot(&mut tui, &td);
        let changed_text = saved_text(&mut td);
        assert_ne!(changed_text, original, "500 次混合操作后文本应发生变化");
        assert_ne!(
            changed_snapshot, initial_snapshot,
            "500 次混合操作后 UI 渲染应发生变化"
        );

        for undo_idx in 0..500usize {
            h.handle_ctrl_z(&mut tui, &td).unwrap();
            if undo_idx % 50 == 49 {
                let snapshot = render_snapshot(&mut tui, &td);
                assert!(
                    !snapshot.1.is_empty(),
                    "undo 过程中 UI 正文渲染不应为空，undo_idx={undo_idx}"
                );
                assert!(
                    !snapshot.2.is_empty(),
                    "undo 过程中 UI 光标高亮不应丢失，undo_idx={undo_idx}"
                );
            }
        }

        if let TextDisplay::EditBlock(v) = &td {
            v.assert_block_storage_consistent_for_test();
        }
        let final_snapshot = render_snapshot(&mut tui, &td);
        let final_text = saved_text(&mut td);
        assert_eq!(final_text, original, "500 次 ctrl+z 后全文应恢复");
        assert_eq!(
            final_snapshot, initial_snapshot,
            "500 次 ctrl+z 后 UI 渲染应恢复"
        );
    }

    #[test]
    fn test_ui_render_scrolled_window_restores_after_200_mixed_ops_and_undo() {
        let original = format!(
            "{}tail\n",
            (0..720)
                .map(|i| format!("row-{i:04}-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n"))
                .collect::<String>()
        );
        let (mut tui, mut td, _f, _uf) = setup_with_undo(&original);
        let h = handle();

        set_viewport_and_cursor(&mut tui, &td, 240, 4, 3);
        let initial_snapshot = render_snapshot(&mut tui, &td);
        let initial_text = saved_text(&mut td);
        assert_eq!(initial_text, original);

        for step in 0..200usize {
            match step % 4 {
                0 => {
                    set_viewport_and_cursor(&mut tui, &td, 240, 4, 3);
                    let meta = td.get_current_line_meta().unwrap();
                    h.handle_char(&mut tui, meta, &td, if step % 8 == 0 { '你' } else { 'K' })
                        .unwrap();
                }
                1 => {
                    set_viewport_and_cursor(&mut tui, &td, 240, 5, 0);
                    let meta = td.get_current_line_meta().unwrap();
                    let paste = format!("段{step}\nwrap-{step}-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
                    h.handle_paste(&mut tui, meta, &td, &paste).unwrap();
                }
                2 => {
                    set_viewport_and_cursor(&mut tui, &td, 241, 3, 2);
                    let meta = td.get_current_line_meta().unwrap();
                    h.handle_enter(&mut tui, meta, &td).unwrap();
                }
                3 => {
                    set_viewport_and_cursor(&mut tui, &td, 241, 3, 2);
                    let meta = td.get_current_line_meta().unwrap();
                    h.handle_backspace(&mut tui, meta, &td).unwrap();
                }
                _ => unreachable!(),
            }
            if let TextDisplay::EditBlock(v) = &td {
                v.assert_block_storage_consistent_for_test();
            }
        }

        let changed_snapshot = render_snapshot(&mut tui, &td);
        assert_ne!(changed_snapshot, initial_snapshot);

        for _ in 0..200usize {
            h.handle_ctrl_z(&mut tui, &td).unwrap();
        }

        if let TextDisplay::EditBlock(v) = &td {
            v.assert_block_storage_consistent_for_test();
        }
        set_viewport_and_cursor(&mut tui, &td, 240, 4, 3);
        let final_snapshot = render_snapshot(&mut tui, &td);
        let final_text = saved_text(&mut td);
        assert_eq!(final_text, original, "滚动窗口混合操作回退后全文应恢复");
        assert_eq!(
            final_snapshot, initial_snapshot,
            "滚动窗口混合操作回退后 UI 快照应恢复"
        );
    }

    #[test]
    fn test_ui_render_large_paste_hides_softwrap_continuation_numbers_and_keeps_cursor() {
        let original = "header\nbody\n".repeat(40);
        let (mut tui, mut td, _f, _uf) = setup_with_undo(&original);
        let h = handle();

        set_viewport_and_cursor(&mut tui, &td, 1, 0, 0);
        let meta = td.get_current_line_meta().unwrap();
        let large_paste = format!(
            "超长前缀-{}\n{}\n尾巴",
            "你".repeat(40),
            "segment-".repeat(30)
        );
        h.handle_paste(&mut tui, meta, &td, &large_paste).unwrap();

        let (nav, body, cursor_marks, _, _) = render_snapshot(&mut tui, &td);
        let visible_nonempty_body_rows: Vec<usize> = body
            .iter()
            .enumerate()
            .filter_map(|(i, line)| (!line.is_empty()).then_some(i))
            .collect();
        assert!(
            visible_nonempty_body_rows.len() >= 3,
            "超大 paste 后当前页应出现多行正文渲染"
        );
        let continuation_rows: Vec<usize> = nav
            .iter()
            .enumerate()
            .filter_map(|(i, line)| {
                (line.is_empty() && body.get(i).is_some_and(|b| !b.is_empty())).then_some(i)
            })
            .collect();
        assert!(
            !continuation_rows.is_empty(),
            "软换行续行应隐藏行号，当前 nav={nav:?} body={body:?}"
        );
        assert!(
            cursor_marks
                .iter()
                .any(|(_, _, _, bg)| *bg == Some(Color::LightRed)),
            "超大 paste 后应保留红色光标高亮"
        );
        let text = saved_text(&mut td);
        assert!(text.starts_with(&large_paste));
    }

    #[test]
    fn test_ui_render_snapshot_stack_rewinds_for_repeated_large_pastes() {
        let original = format!(
            "{}end\n",
            (0..260)
                .map(|i| format!("base-{i:03}-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n"))
                .collect::<String>()
        );
        let (mut tui, mut td, _f, _uf) = setup_with_undo(&original);
        let h = handle();

        let mut snapshots = vec![canonical_snapshot(&mut tui, &td)];
        let mut texts = vec![saved_text(&mut td)];
        for step in 0..30usize {
            set_viewport_and_cursor(&mut tui, &td, 1, 0, 0);
            let meta = td.get_current_line_meta().unwrap();
            let paste = match step % 3 {
                0 => format!("paste-{step}-{}\n", "x".repeat(120)),
                1 => format!("中文块-{step}-{}\nnext-{step}\n", "你".repeat(24)),
                _ => format!("mix-{step}-{}\n{}\n", "a".repeat(40), "段".repeat(18)),
            };
            h.handle_paste(&mut tui, meta, &td, &paste).unwrap();
            if let TextDisplay::EditBlock(v) = &td {
                v.assert_block_storage_consistent_for_test();
            }
            snapshots.push(canonical_snapshot(&mut tui, &td));
            texts.push(saved_text(&mut td));
        }

        for undo_idx in (0..30usize).rev() {
            h.handle_ctrl_z(&mut tui, &td).unwrap();
            if let TextDisplay::EditBlock(v) = &td {
                v.assert_block_storage_consistent_for_test();
            }
            let expected_snapshot = &snapshots[undo_idx];
            let expected_text = &texts[undo_idx];
            let actual_snapshot = canonical_snapshot(&mut tui, &td);
            let actual_text = saved_text(&mut td);
            assert_eq!(
                actual_text, *expected_text,
                "连续 paste 回退时，第 {undo_idx} 层全文应回到之前快照"
            );
            assert_eq!(
                actual_snapshot, *expected_snapshot,
                "连续 paste 回退时，第 {undo_idx} 层 UI 渲染应回到之前快照"
            );
        }
    }
}
