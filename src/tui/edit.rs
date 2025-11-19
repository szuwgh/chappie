use crate::common::ring_vec::RingVec;
use crate::textwarp::CacheStr;
use crate::textwarp::EditLineMeta;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;

fn n_chars_skip_control_mem_opt(s: &str, n: usize) -> (&str, &str, &str, usize) {
    let mut count = 0;
    let mut start_idx = None;
    let mut end_idx = None;
    let mut last_start_idx = None;

    for (idx, ch) in s.char_indices() {
        if ch.is_control() {
            continue;
        }
        if n > 0 && count == n - 1 {
            last_start_idx = Some(idx);
        }
        if count == n {
            // 第 n 个非控制字符
            start_idx = Some(idx);
        }
        if count == n + 1 {
            // 第 n+1 个非控制字符
            end_idx = Some(idx);
            break;
        }
        count += 1;
    }

    // 如果 never set, 默认到末尾
    let last_start = last_start_idx.unwrap_or_else(|| 0);
    let start = start_idx.unwrap_or_else(|| s.len());
    let end = end_idx.unwrap_or_else(|| s.len());

    (&s[..start], &s[start..end], &s[end..], start - last_start)
}

pub(crate) fn get_edit_content<'a>(
    txts: &'a RingVec<CacheStr>,
    line_meta: &'a RingVec<EditLineMeta>,
    cur_line: usize,
    select_line: &Option<(usize, usize)>,
    height: usize,
    offset: usize,
    cursor_y: usize,
    cursor_x: usize,
) -> (Text<'a>, Text<'a>, usize, usize) {
    assert!(txts.len() == line_meta.len());
    let mut lines = Vec::with_capacity(line_meta.len());
    let mut byte_cursor: usize = 0; //bytes的索引 表示光标在多少个u8
    let mut last_char_bytes_size: usize = 0; //获取上一个字符bytes大小用来做删除操作
    for (i, txt) in txts.iter().enumerate() {
        let (str_left, str_right) = txt.text(offset..);
        if cursor_y == i {
            let char_count1 = str_left.chars().count();
            let (spans, byte_pos, last_csz) = if cursor_x < char_count1 {
                build_cursor_line(str_left, str_right, cursor_x, char_count1, true)
            } else {
                build_cursor_line(str_left, str_right, cursor_x, char_count1, false)
            };
            byte_cursor = byte_pos;
            last_char_bytes_size = last_csz;
            lines.push(Line::from(spans));
        } else {
            let spans = vec![Span::raw(str_left), Span::raw(str_right)];
            lines.push(Line::from(spans));
        }
    }
    append_padding_lines(&mut lines, cursor_y, cursor_x, line_meta.len());
    let nav_text = build_nav_text(line_meta, height);
    let text = Text::from(lines);
    (nav_text, text, byte_cursor, last_char_bytes_size)
}

fn build_cursor_line<'a>(
    str_left: &'a str,
    str_right: &'a str,
    cursor_x: usize,
    char_count1: usize,
    is_str1: bool,
) -> (Vec<Span<'a>>, usize, usize) {
    //let (str1, str2) = txt.text(offset..);
    let mut spans = Vec::new();
    let mut last_char_bytes_size: usize = 0;
    let byte_cursor;
    let (a, b, c) = if !is_str1 {
        let (a, b, c, last_csz) =
            n_chars_skip_control_mem_opt(str_right, cursor_x.saturating_sub(char_count1));
        //如果上一个字符大小是0则光标可能在第一个字符那里
        if last_csz == 0 {
            let (_, _, _, sz) = n_chars_skip_control_mem_opt(str_left, char_count1);
            last_char_bytes_size = sz;
        } else {
            last_char_bytes_size = last_csz;
        }
        (a, b, c)
    } else {
        let (a, b, c, last_csz) = n_chars_skip_control_mem_opt(str_left, cursor_x);
        last_char_bytes_size = last_csz;
        (a, b, c)
    };
    if b.len() > 0 {
        byte_cursor = if !is_str1 {
            spans.push(Span::raw(str_left));
            a.len() + str_left.len()
        } else {
            a.len()
        };
        spans.push(Span::raw(a.to_string()));
        spans.push(Span::styled(
            b.to_string(),
            Style::default().bg(Color::LightRed),
        ));
        spans.push(Span::raw(c.to_string()));
    } else {
        byte_cursor = if !is_str1 {
            str_left.len() + str_right.len()
        } else {
            str_left.len()
        };
        spans.push(Span::raw(str_left));
        spans.push(Span::raw(str_right));
        let diff = cursor_x.saturating_sub(str_left.len() + str_right.len());
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
fn build_nav_text(line_meta: &RingVec<EditLineMeta>, height: usize) -> Text {
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

// pub(crate) fn get_edit_content<'a>(
//     txts: &'a RingVec<CacheStr>,
//     line_meta: &'a RingVec<EditLineMeta>,
//     cur_line: usize,
//     select_line: &Option<(usize, usize)>,
//     height: usize,
//     offset: usize,
//     cursor_y: usize,
//     cursor_x: usize,
// ) -> (Text<'a>, Text<'a>, usize, usize) {
//     assert!(txts.len() == line_meta.len());
//     let mut lines = Vec::with_capacity(line_meta.len());
//     let mut byte_cursor: usize = 0; //bytes的索引 表示光标在多少个u8
//     let mut last_char_bytes_size: usize = 0; //获取上一个字符bytes大小用来做删除操作
//     for (i, txt) in txts.iter().enumerate() {
//         let (str1, str2) = txt.text(offset..);
//         if cursor_y == i {
//             let mut spans = Vec::new();
//             let count1 = str1.chars().count();
//             if cursor_x < count1 {
//                 let (a, b, c, size) = n_chars_skip_control_mem_opt(str1.as_ref(), cursor_x);
//                 last_char_bytes_size = size;
//                 if b.len() > 0 {
//                     byte_cursor = a.len();
//                     spans.push(Span::raw(a.to_string()));
//                     spans.push(Span::styled(
//                         b.to_string(),
//                         Style::default().bg(Color::LightRed),
//                     ));
//                     spans.push(Span::raw(c.to_string()));
//                     spans.push(Span::raw(str2));
//                 } else {
//                     byte_cursor = str1.len();
//                     spans.push(Span::raw(str1));
//                     spans.push(Span::raw(str2));
//                     let diff = cursor_x.saturating_sub(txt.len());
//                     let padding = " ".repeat(diff);
//                     spans.push(Span::raw(padding));
//                     // 在填充后显示高亮的光标
//                     spans.push(Span::styled(" ", Style::default().bg(Color::LightRed)));
//                 }
//             } else {
//                 let (a, b, c, size) =
//                     n_chars_skip_control_mem_opt(str2.as_ref(), cursor_x - str1.chars().count());
//                 //如果上一个字符大小是0则光标可能在第一个字符那里
//                 if size == 0 {
//                     let (_, _, _, sz) = n_chars_skip_control_mem_opt(str1.as_ref(), count1);
//                     last_char_bytes_size = sz;
//                 } else {
//                     last_char_bytes_size = size;
//                 }
//                 if b.len() > 0 {
//                     byte_cursor = a.len() + str1.len();
//                     spans.push(Span::raw(str1));
//                     spans.push(Span::raw(a.to_string()));
//                     spans.push(Span::styled(
//                         b.to_string(),
//                         Style::default().bg(Color::LightRed),
//                     ));
//                     spans.push(Span::raw(c.to_string()));
//                 } else {
//                     byte_cursor = str1.len() + str2.len();
//                     spans.push(Span::raw(str1));
//                     spans.push(Span::raw(str2));
//                     let diff = cursor_x.saturating_sub(txt.len());
//                     let padding = " ".repeat(diff);
//                     spans.push(Span::raw(padding));
//                     // 在填充后显示高亮的光标
//                     spans.push(Span::styled(" ", Style::default().bg(Color::LightRed)));
//                 }
//             }

//             lines.push(Line::from(spans));
//         } else {
//             let spans = vec![Span::raw(str1), Span::raw(str2)];
//             lines.push(Line::from(spans));
//         }
//     }
//     if cursor_y >= line_meta.len() {
//         let diff = cursor_y - line_meta.len();
//         for _ in 0..diff {
//             lines.push(Line::raw(""));
//         }
//         let mut spans = Vec::new();
//         let padding = " ".repeat(cursor_x);
//         spans.push(Span::raw(padding));
//         // 在填充后显示高亮的光标
//         spans.push(Span::styled(" ", Style::default().bg(Color::LightRed)));
//         lines.push(Line::from(spans));
//     }

//     let nav_text = Text::from(
//         (0..height)
//             .enumerate()
//             .map(|(i, _)| {
//                 if i > line_meta.len() {
//                     return Line::raw("");
//                 }
//                 Line::from(Span::styled(
//                     format!("{:>4} ", line_meta.get(i).unwrap().get_line_num()),
//                     Style::default().fg(Color::White),
//                 ))
//                 // 高亮当前行
//             })
//             .collect::<Vec<Line>>(),
//     );

//     let text = Text::from(lines);
//     (nav_text, text, byte_cursor, last_char_bytes_size)
// }
