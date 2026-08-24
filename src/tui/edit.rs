use crate::chap;
use crate::common::ring_vec::RingVec;
use crate::textwarp::CacheStr;
use crate::textwarp::LineParts;
use crate::textwarp::LineState;
use crate::tui::append_padding_lines;
use crate::tui::build_cursor_line;
use crate::tui::build_nav_text;
use crate::tui::char_range_to_visible;
use crate::tui::cut_highlight_part;
use crate::tui::n_chars_skip_control_mem_opt;
use crate::tui::BuildContent;
use crate::tui::Content;
use crate::tui::EditContext;
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

pub(crate) struct EditBuildContent;

impl BuildContent for EditBuildContent {
    fn build_content<'a>(
        txts: &'a RingVec<CacheStr>,
        with: usize,
        line_meta: &'a RingVec<LineState>,
        cur_line: usize,
        select_line: &Option<(usize, usize)>,
        ed_ctx: &EditContext,
    ) -> super::Content<'a> {
        let height = ed_ctx.height;
        let column_offset = ed_ctx.column_offset;
        let cursor_y = ed_ctx.cursor_y;
        let cursor_x = ed_ctx.cursor_x;
        let is_txt_model = ed_ctx.is_txt_model;
        let mut find_highlight_offset = ed_ctx.find_highlight_offset;
        let find_line_index = ed_ctx.find_line_index;
        let mut highlight_len = ed_ctx.highlight_len;
        assert!(txts.len() == line_meta.len());
        let mut lines = Vec::with_capacity(line_meta.len());
        let mut byte_cursor: usize = 0; //bytes的索引 表示光标在多少个u8
        let mut prev_char_bytes_size: usize = 0; //获取上一个字符bytes大小用来做删除操作
        for (i, txt) in txts.iter().enumerate() {
            //一行数据可能会分成很多个块
            // 单次字符遍历求可见切片（融合原 char_range_to_byte_range + slice_parts_range）
            let full = txt.text(0..);
            let visible =
                char_range_to_visible(full.as_parts(), column_offset, column_offset + with);
            let parts: &[&[u8]] = visible.as_parts();
            if cursor_y == i && is_txt_model {
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

                if let Some(line_num) = find_line_index {
                    let line_meta = line_meta.get(i).unwrap();
                    // log::debug!("line_meta.get_line_index:{}", line_meta.get_line_index());
                    // log::debug!("line_num():{}", line_num);
                    // log::debug!("line_meta.line_offset:{}", line_meta.line_offset);
                    // log::debug!("find_highlight_offset:{}", find_highlight_offset);
                    // log::debug!("line_meta.get_line_end():{}", line_meta.get_line_end());
                    if line_meta.get_line_index() == line_num {
                        if line_meta.line_offset <= find_highlight_offset
                            && find_highlight_offset < line_meta.get_line_end()
                        {
                            let highlight_start = find_highlight_offset - line_meta.line_offset;
                            let highlight_end =
                                (highlight_start + highlight_len).min(line_meta.get_line_end()); // 假设高亮一个字符
                            let (a, b, c) =
                                cut_highlight_part(parts, highlight_start, highlight_end);
                            let mut spans = Vec::with_capacity(a.len() + b.len() + c.len());
                            for v in a {
                                log::debug!("a:{}", str::from_utf8(v).unwrap_or("☻"));
                                spans.push(Span::raw(str::from_utf8(v).unwrap_or("☻")));
                            }
                            for v in b {
                                log::debug!("b:{}", str::from_utf8(v).unwrap_or("☻"));
                                spans.push(Span::styled(
                                    str::from_utf8(v).unwrap_or("☻"),
                                    Style::default().bg(Color::Green),
                                ));
                            }
                            for v in c {
                                log::debug!("c:{}", str::from_utf8(v).unwrap_or("☻"));
                                spans.push(Span::raw(str::from_utf8(v).unwrap_or("☻")));
                            }
                            highlight_len = highlight_len.saturating_sub(
                                line_meta
                                    .get_line_end()
                                    .saturating_sub(find_highlight_offset),
                            );
                            find_highlight_offset = line_meta.get_line_end();
                            lines.push(Line::from(spans));

                            continue;
                        }
                    }
                }

                let mut spans = Vec::with_capacity(parts.len());
                for v in parts {
                    spans.push(Span::raw(str::from_utf8(v).unwrap_or("☻")));
                }
                //spans.push(Span::raw(str::from_utf8(visible[1]).unwrap_or("")));
                lines.push(Line::from(spans));
            }
        }
        append_padding_lines(&mut lines, cursor_y, cursor_x, line_meta.len());
        let nav_text = build_nav_text(line_meta, height);
        let text = Text::from(lines);
        return Content {
            navi: nav_text,
            visible_content: text,
            byte_cursor,
            last_char_bytes_size: prev_char_bytes_size,
        };
    }

    fn command_focus() -> bool {
        false
    }
}

// pub(crate) fn EditBuildContent::build_content<'a>(
//     txts: &'a RingVec<CacheStr>,
//     with: usize,
//     line_meta: &'a RingVec<LineState>,
//     cur_line: usize,
//     select_line: &Option<(usize, usize)>,
//     ed_ctx: &EditContext,
// ) -> (Text<'a>, Text<'a>, usize, usize) {
// }

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
        LineStateBuilder::new()
            .line_index(line_num.saturating_sub(1))
            .build()
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
            .line_index(line_num.saturating_sub(1))
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
        td.get_one_page_from_state(&LineState::file_start())
            .unwrap();

        let undo_dir = TempDir::new().unwrap();
        let undo_path = undo_dir.path().join("undo.chpu");
        let mut tui = ChapTui::for_test(20, 80);
        tui.undo = Some(UndoFile::open(&undo_path).unwrap());
        (tui, td, tmp, undo_dir)
    }

    fn handle() -> HandleEdit {
        HandleEdit::new()
    }

    fn saved_text(td: &mut TextDisplay) -> String {
        let tmp = NamedTempFile::new().unwrap();
        td.save(tmp.path()).unwrap();
        std::fs::read_to_string(tmp.path()).unwrap()
    }

    fn sync_cursor_metrics(tui: &mut ChapTui, td: &TextDisplay) {
        let ed_ctx = EditContext {
            height: tui.elem.tv.get_height(),
            column_offset: tui.column_offset,
            cursor_y: tui.cursor_y,
            cursor_x: tui.cursor_x,
            is_txt_model: !tui.in_command_mode(),
            find_highlight_offset: 0,
            find_line_index: None,
            highlight_len: 0,
        };
        let (content, meta) = td.get_current_page().unwrap();
        let content = EditBuildContent::build_content(
            content,
            tui.elem.tv.get_width(),
            &meta,
            0,
            &None,
            &ed_ctx,
            // tui.cursor_x,
            // !tui.in_command_mode(),
            // None,
        );
        tui.bytes_cursor = content.byte_cursor;
        tui.bytes_cursor_size = content.last_char_bytes_size;
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
                let mut col = 0usize;
                line.spans.iter().filter_map(move |span| {
                    let span_col = col;
                    col += span.content.chars().count();
                    if span.style.bg.is_some() {
                        Some((
                            line_idx,
                            span_col,
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
        let ed_ctx = EditContext {
            height: tui.elem.tv.get_height(),
            column_offset: tui.column_offset,
            cursor_y: tui.cursor_y,
            cursor_x: tui.cursor_x,
            is_txt_model: true,
            find_highlight_offset: 0,
            find_line_index: None,
            highlight_len: 0,
        };
        // td.get_one_page(tui.start_line_num).unwrap();
        let (content, meta) = td.get_current_page().unwrap();
        let content = EditBuildContent::build_content(
            content,
            tui.elem.tv.get_width(),
            &meta,
            0,
            &None,
            &ed_ctx,
        );
        (
            render_text_lines(&content.navi),
            render_text_lines(&content.visible_content),
            render_cursor_marks(&content.visible_content),
            content.byte_cursor,
            content.last_char_bytes_size,
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

    fn current_file_start_state(td: &TextDisplay) -> LineState {
        if let TextDisplay::EditBlock(v) = td {
            if let Some((block_id, block_offset)) =
                v.resolve_block_for_file_offset_with_boundary(0, true)
            {
                let block_line_index = v
                    .find_block_line_for_offset(block_id, block_offset)
                    .unwrap_or(0);
                return LineState::builder()
                    .block_num(block_id)
                    .block_line_index(block_line_index)
                    .block_offset(block_offset)
                    .line_index(0)
                    .line_offset(0)
                    .line_file_start(0)
                    .build();
            }
        }
        LineState::file_start()
    }

    fn set_viewport_and_cursor(
        tui: &mut ChapTui,
        td: &TextDisplay,
        start_line_num: usize,
        cursor_y: usize,
        cursor_x: usize,
    ) {
        td.get_one_page_from_state(&current_file_start_state(td))
            .unwrap();
        for _ in 1..start_line_num {
            let Some(last) = td.get_current_line_meta().unwrap().last().cloned() else {
                break;
            };
            td.scroll_next_one_line(&last).unwrap();
        }
        if let Some(first) = td.get_current_line_meta().unwrap().get(0) {
            tui.start_line_state = first.clone();
        }
        tui.cursor_y = cursor_y;
        tui.cursor_x = cursor_x;
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
    fn test_build_cursor_line_empty_visible_parts_shows_cursor() {
        let parts: &[&[u8]] = &[];
        let (spans, byte_cursor, last_sz) = build_cursor_line(parts, 0, &[], 0);

        assert_eq!(byte_cursor, 0);
        assert_eq!(last_sz, 0);
        assert!(
            spans.iter().any(|s| s.style.bg == Some(Color::LightRed)),
            "空可见分片仍应渲染光标"
        );
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

    // ── EditBuildContent::build_content 集成测试 ─────────────────────────────────────────────

    #[test]
    fn test_build_content_byte_cursor_at_0() {
        // 单行 "hello"，光标在 (x=0, y=0)
        let txts = make_ring_txt(vec!["hello"]);
        let meta = make_ring_meta(&[1]);
        let ed_ctx = EditContext {
            height: 10,
            column_offset: 0,
            cursor_y: 0,
            cursor_x: 0,
            is_txt_model: true,
            find_highlight_offset: 0,
            find_line_index: None,
            highlight_len: 0,
        };
        let content = EditBuildContent::build_content(&txts, 80, &meta, 0, &None, &ed_ctx);
        assert_eq!(content.byte_cursor, 0);
        // 文本内容应可见
        let text = content.visible_content.to_string();
        assert!(
            text.contains("hello") || text.contains("h"),
            "内容应含 hello"
        );
    }

    #[test]
    fn test_build_content_byte_cursor_middle() {
        // "hello"，光标在 (x=2, y=0)，byte_cursor 应为 2
        let txts = make_ring_txt(vec!["hello"]);
        let meta = make_ring_meta(&[1]);
        let ed_ctx = EditContext {
            height: 10,
            column_offset: 0,
            cursor_y: 0,
            cursor_x: 2,
            is_txt_model: true,
            find_highlight_offset: 0,
            find_line_index: None,
            highlight_len: 0,
        };
        let content = EditBuildContent::build_content(&txts, 80, &meta, 0, &None, &ed_ctx);
        assert_eq!(content.byte_cursor, 2);
        assert_eq!(content.last_char_bytes_size, 1); // 前一字符 'e' 占 1 字节
    }

    #[test]
    fn test_build_content_multibyte_byte_cursor() {
        // "你好世界"，光标在 (x=2, y=0)，byte_cursor 应为 6（2个汉字 × 3字节）
        let txts = make_ring_txt(vec!["你好世界"]);
        let meta = make_ring_meta(&[1]);
        let ed_ctx = EditContext {
            height: 10,
            column_offset: 0,
            cursor_y: 0,
            cursor_x: 2,
            is_txt_model: true,
            find_highlight_offset: 0,
            find_line_index: None,
            highlight_len: 0,
        };
        let content = EditBuildContent::build_content(&txts, 80, &meta, 0, &None, &ed_ctx);
        assert_eq!(content.byte_cursor, 6);
        assert_eq!(content.last_char_bytes_size, 3); // 前一字符 "好" 占 3 字节
    }

    #[test]
    fn test_build_content_multiline_nav() {
        // 3 行文本，行号导航应正确
        let txts = make_ring_txt(vec!["line1", "line2", "line3"]);
        let meta = make_ring_meta(&[1, 2, 3]);
        let ed_ctx = EditContext {
            height: 5,
            column_offset: 0,
            cursor_y: 0,
            cursor_x: 2,
            is_txt_model: true,
            find_highlight_offset: 0,
            find_line_index: None,
            highlight_len: 0,
        };
        let content = EditBuildContent::build_content(&txts, 80, &meta, 0, &None, &ed_ctx);
        assert!(content.navi.to_string().contains("1"));
        assert!(content.navi.to_string().contains("2"));
        assert!(content.navi.to_string().contains("3"));
    }

    #[test]
    fn test_build_content_cursor_beyond_lines() {
        // 光标行 cursor_y=3 超出 meta 范围（只有 2 行），应追加 padding
        let txts = make_ring_txt(vec!["a", "b"]);
        let meta = make_ring_meta(&[1, 2]);
        let ed_ctx = EditContext {
            height: 10,
            column_offset: 0,
            cursor_y: 3,
            cursor_x: 0,
            is_txt_model: true,
            find_highlight_offset: 0,
            find_line_index: None,
            highlight_len: 0,
        };
        let content = EditBuildContent::build_content(&txts, 80, &meta, 0, &None, &ed_ctx);
        // 应有超过 2 行的渲染输出（含 padding 行）
        assert!(content.visible_content.lines.len() >= 3);
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
            // td.get_one_page(1).unwrap();
            let meta = td.get_current_line_meta().unwrap();
            match step % 4 {
                0 => {
                    // tui.start_line_num = 1;
                    tui.cursor_y = 0;
                    tui.cursor_x = 0;
                    tui.bytes_cursor = 0;
                    tui.bytes_cursor_size = 0;
                    let ch = if step % 8 == 0 { '你' } else { 'A' };
                    h.handle_char(&mut tui, meta, &td, ch).unwrap();
                }
                1 => {
                    // tui.start_line_num = 1;
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
                    //tui.start_line_num = 1;
                    tui.cursor_y = 0;
                    tui.cursor_x = 1;
                    //td.get_one_page(tui.start_line_num).unwrap();
                    sync_cursor_metrics(&mut tui, &td);
                    let meta = td.get_current_line_meta().unwrap();
                    h.handle_enter(&mut tui, meta, &td).unwrap();
                }
                3 => {
                    // tui.start_line_num = 1;
                    tui.cursor_y = 0;
                    tui.cursor_x = 1;
                    // td.get_one_page(tui.start_line_num).unwrap();
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
