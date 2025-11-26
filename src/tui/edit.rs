use crate::common::ring_vec::RingVec;
use crate::textwarp::CacheStr;
use crate::textwarp::EditLineMeta;
use crate::textwarp::LineParts;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use utf8_iter::Utf8CharsEx;
pub(crate) struct EditUI<'a> {
    txts: RingVec<CacheStr>,
    line_meta: RingVec<EditLineMeta>,
    line_spans: Vec<Span<'a>>,
}

// 获取字符串中前 n 个非控制字符的位置，返回前三部分字符串及最后一个非控制字符的字节大小
fn n_chars_skip_control_mem_opt(s: &[u8], n: usize) -> (&[u8], &[u8], &[u8], usize) {
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
    column_offset: usize,
    cursor_y: usize,
    cursor_x: usize,
) -> (Text<'a>, Text<'a>, usize, usize) {
    assert!(txts.len() == line_meta.len());
    let mut lines = Vec::with_capacity(line_meta.len());
    let mut byte_cursor: usize = 0; //bytes的索引 表示光标在多少个u8
    let mut last_char_bytes_size: usize = 0; //获取上一个字符bytes大小用来做删除操作
    for (i, txt) in txts.iter().enumerate() {
        //一行数据可能会分成很多个块
        let t = txt.text(column_offset..);
        let parts = t.as_parts();
        if cursor_y == i {
            let parts_str = parts
                .iter()
                .map(|s| String::from_utf8_lossy(*s).to_string())
                .collect::<Vec<String>>();
            log::debug!("get_edit_content: line {}, parts: {:?}", i, parts_str);
            //计算part数组叠加的字符数量
            let mut char_count = LineParts::<usize>::empty();
            let mut char_curosr_index = 0; // 判断光标在那个 part中
            let mut char_sum_count = 0;
            for (idx, s) in parts.iter().enumerate() {
                let count = (*s).chars().count();
                char_sum_count += count;
                char_count.append(char_sum_count);
            }
            for (idx, s) in char_count.as_parts().iter().enumerate() {
                if cursor_x < *s {
                    char_curosr_index = idx;
                    break;
                }
            }
            let (spans, byte_pos, last_csz) =
                build_cursor_line(parts, cursor_x, char_count.as_parts(), char_curosr_index);
            //let char_count1 = str_left.chars().count();
            // let (spans, byte_pos, last_csz) = if cursor_x < char_count1 {
            //     build_cursor_line(parts, cursor_x, char_count1, true)
            // } else {
            //     build_cursor_line(parts, cursor_x, char_count1, false)
            // };
            byte_cursor = byte_pos;
            last_char_bytes_size = last_csz;
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
    (nav_text, text, byte_cursor, last_char_bytes_size)
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
    log::debug!(
        "build_cursor_line: cursor_x: {}, char_curosr_index: {}, char_count: {:?}",
        cursor_x,
        char_curosr_index,
        char_count
    );
    let (a, b, c) = if char_curosr_index == 0 {
        let (a, b, c, last_csz) = n_chars_skip_control_mem_opt(str_parts[0], cursor_x);
        last_char_bytes_size = last_csz;
        (a, b, c)
    } else {
        //let mut sum_count = 0;
        // for i in 0..char_curosr_index {
        //     sum_count += char_count[i];
        // }
        let (a, b, c, last_csz) = n_chars_skip_control_mem_opt(
            str_parts[char_curosr_index],
            cursor_x.saturating_sub(char_count[char_curosr_index - 1]),
        );
        //如果上一个字符大小是0则光标可能在第一个字符那里
        if last_csz == 0 {
            let (_, _, _, sz) = n_chars_skip_control_mem_opt(
                str_parts[char_curosr_index - 1],
                char_count[char_curosr_index - 1],
            );
            last_char_bytes_size = sz;
        } else {
            last_char_bytes_size = last_csz;
        }
        (a, b, c)
    };
    log::debug!(
        "build_cursor_line: last_char_bytes_size:{}, a: {:?}, b: {:?}, c: {:?}",
        last_char_bytes_size,
        String::from_utf8_lossy(a),
        String::from_utf8_lossy(b),
        String::from_utf8_lossy(c),
    );
    if b.len() > 0 {
        byte_cursor = if char_curosr_index == 0 {
            a.len()
        } else {
            // let mut byte_pos = 0;
            // for i in 0..char_curosr_index {
            //     byte_pos += str_parts[i].len();
            // }
            for j in 0..char_curosr_index {
                spans.push(Span::raw(String::from_utf8_lossy(str_parts[j])));
            }
            str_parts[..char_curosr_index]
                .iter()
                .filter(|s| !s.is_empty())
                .fold(0, |acc, s| acc + s.len())
                + a.len()
        };

        spans.push(Span::raw(String::from_utf8_lossy(a)));
        spans.push(Span::styled(
            String::from_utf8_lossy(b),
            Style::default().bg(Color::LightRed),
        ));
        spans.push(Span::raw(String::from_utf8_lossy(c)));
        for j in (char_curosr_index + 1)..str_parts.len() {
            spans.push(Span::raw(String::from_utf8_lossy(str_parts[j])));
        }
        // byte_cursor = if !is_str1 {
        //     spans.push(Span::raw(str_left));
        //     a.len() + str_left.len()
        // } else {
        //     a.len()
        // };
        // spans.push(Span::raw(a.to_string()));
        // spans.push(Span::styled(
        //     b.to_string(),
        //     Style::default().bg(Color::LightRed),
        // ));
        // spans.push(Span::raw(c.to_string()));
    } else {
        byte_cursor = if char_curosr_index == 0 {
            str_parts[0].len()
        } else {
            // let mut byte_pos = 0;
            // for i in 0..str_parts.len() {
            //     byte_pos += str_parts[i].len();
            // }
            str_parts[..char_curosr_index]
                .iter()
                .filter(|s| !s.is_empty())
                .fold(0, |acc, s| acc + s.len())
        };
        for j in 0..str_parts.len() {
            spans.push(Span::raw(String::from_utf8_lossy(str_parts[j])));
        }
        let diff = cursor_x.saturating_sub(char_count.last().copied().unwrap_or(0));
        let padding = " ".repeat(diff);
        spans.push(Span::raw(padding));
        // 在填充后显示高亮的光标
        spans.push(Span::styled(" ", Style::default().bg(Color::LightRed)));
        // byte_cursor = if !is_str1 {
        //     str_left.len() + str_right.len()
        // } else {
        //     str_left.len()
        // };
        // spans.push(Span::raw(str_left));
        // spans.push(Span::raw(str_right));
        // let diff = cursor_x.saturating_sub(str_left.len() + str_right.len());
        // let padding = " ".repeat(diff);
        // spans.push(Span::raw(padding));
        // // 在填充后显示高亮的光标
        // spans.push(Span::styled(" ", Style::default().bg(Color::LightRed)));
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
