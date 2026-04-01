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
    for (idx, (byte_index, ch)) in s.char_indices().enumerate() {
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

pub(crate) fn get_edit_content<'a>(
    txts: &'a RingVec<CacheStr>,
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
        let s = txt.as_str();
        let t = txt.text(column_offset..);
        let parts = t.as_parts();
        if cursor_y == i {
            //取上一行的最后一个字符char 大小
            let mut prev_line_last_char_size = 0;
            if cursor_x == 0 {
                if i > 0 {
                    if let Some(prev_txt) = txts.get(i - 1) {
                        let prev_t = prev_txt.text(0..);
                        let prev_parts = prev_t.as_parts();
                        prev_parts.iter().rev().for_each(|s| {
                            if s.len() > 0 {
                                (*s).char_indices().rev().for_each(|(_, ch)| {
                                    if !ch.is_control() {
                                        prev_line_last_char_size = ch.len_utf8();
                                        return;
                                    }
                                });
                                return;
                            }
                        });
                    }
                }
            }
            //计算part数组叠加的字符数量
            let mut char_count = LineParts::<usize>::empty();
            let mut char_curosr_index = 0; // 判断光标在那个 part中

            for (_, s) in parts.iter().enumerate() {
                let count = (*s).chars().count();
                char_count.append(count);
            }
            let mut char_sum_count = 0;
            for (idx, s) in char_count.as_parts().iter().enumerate() {
                char_sum_count += *s;
                if char_sum_count > 0 && cursor_x <= char_sum_count.saturating_sub(1) {
                    char_curosr_index = idx;
                    break;
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
            let spans = parts
                .iter()
                .map(|s| Span::raw(String::from_utf8_lossy(*s)))
                .collect::<Vec<Span>>();
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
    let mut spans = Vec::new();
    let mut last_char_bytes_size: usize = 0;
    let byte_cursor;
    let (a, b, c) = if char_curosr_index == 0 {
        let (a, b, c, last_csz) = n_chars_skip_control_mem_opt(str_parts[0], cursor_x);
        last_char_bytes_size = last_csz;
        (a, b, c)
    } else {
        let (a, b, c, last_csz) = n_chars_skip_control_mem_opt(
            str_parts[char_curosr_index],
            cursor_x.saturating_sub(char_count[..char_curosr_index].iter().sum()),
        );
        //如果上一个字符大小是0则光标可能在第一个字符那里
        if last_csz == 0 {
            let (_, _, _, sz) = n_chars_skip_control_mem_opt(
                str_parts[char_curosr_index - 1],
                char_count[..char_curosr_index].iter().sum(),
            );
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
            for j in 0..char_curosr_index {
                spans.push(Span::raw(String::from_utf8_lossy(str_parts[j])));
            }
            str_parts[..char_curosr_index]
                .iter()
                .filter(|s| !s.is_empty())
                .fold(0, |acc, s| acc + s.len())
                + a.get_non_control_len()
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
            str_parts[..char_curosr_index]
                .iter()
                .filter(|s| !s.is_empty())
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
                    Line::from(Span::styled(
                        format!("{:>4} ", meta.get_line_num()),
                        Style::default().fg(Color::White),
                    ))
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
    use crate::textwarp::CacheStr;
    use crate::textwarp::LineState;
    use crate::textwarp::LineStateBuilder;

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

    fn make_ring_txt(lines: Vec<&str>) -> RingVec<CacheStr> {
        let mut rv = RingVec::with_capacity(lines.len().max(1));
        for s in lines {
            rv.push(CacheStr::from_vec_for_test(s.as_bytes().to_vec()));
        }
        rv
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
        assert_eq!(last_sz, 3);     // 前一字符 "你" 占 3 字节
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
            get_edit_content(&txts, &meta, 0, &None, 10, 0, 0, 0);
        assert_eq!(byte_cursor, 0);
        // 文本内容应可见
        let text = content.to_string();
        assert!(text.contains("hello") || text.contains("h"), "内容应含 hello");
    }

    #[test]
    fn test_get_edit_content_byte_cursor_middle() {
        // "hello"，光标在 (x=2, y=0)，byte_cursor 应为 2
        let txts = make_ring_txt(vec!["hello"]);
        let meta = make_ring_meta(&[1]);
        let (_, _, byte_cursor, last_sz) =
            get_edit_content(&txts, &meta, 0, &None, 10, 0, 0, 2);
        assert_eq!(byte_cursor, 2);
        assert_eq!(last_sz, 1); // 前一字符 'e' 占 1 字节
    }

    #[test]
    fn test_get_edit_content_multibyte_byte_cursor() {
        // "你好世界"，光标在 (x=2, y=0)，byte_cursor 应为 6（2个汉字 × 3字节）
        let txts = make_ring_txt(vec!["你好世界"]);
        let meta = make_ring_meta(&[1]);
        let (_, _, byte_cursor, last_sz) =
            get_edit_content(&txts, &meta, 0, &None, 10, 0, 0, 2);
        assert_eq!(byte_cursor, 6);
        assert_eq!(last_sz, 3); // 前一字符 "好" 占 3 字节
    }

    #[test]
    fn test_get_edit_content_multiline_nav() {
        // 3 行文本，行号导航应正确
        let txts = make_ring_txt(vec!["line1", "line2", "line3"]);
        let meta = make_ring_meta(&[1, 2, 3]);
        let (nav, _, _, _) = get_edit_content(&txts, &meta, 0, &None, 5, 0, 0, 0);
        assert!(nav.to_string().contains("1"));
        assert!(nav.to_string().contains("2"));
        assert!(nav.to_string().contains("3"));
    }

    #[test]
    fn test_get_edit_content_cursor_beyond_lines() {
        // 光标行 cursor_y=3 超出 meta 范围（只有 2 行），应追加 padding
        let txts = make_ring_txt(vec!["a", "b"]);
        let meta = make_ring_meta(&[1, 2]);
        let (_, content, _, _) = get_edit_content(&txts, &meta, 0, &None, 10, 0, 3, 0);
        // 应有超过 2 行的渲染输出（含 padding 行）
        assert!(content.lines.len() >= 3);
    }
}
