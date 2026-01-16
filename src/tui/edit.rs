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
        let mut len = self.len();
        // for c in self.iter().rev() {
        //     if *c == b'\n' {
        //         len -= 1; // 减去控制字符的字节长度
        //     } else {
        //         break; // 遇到非控制字符时停止
        //     }
        // }
        len
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
        //log::debug!("line {} content:{:?}", i, s);
        let t = txt.text(column_offset..);
        let parts = t.as_parts();
        if cursor_y == i {
            //取上一行的最后一个字符char 大小
            let mut prev_line_last_char_size = 0;
            if cursor_x == 0 {
                if i > 0 {
                    parts.iter().rev().for_each(|s| {
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
            //计算part数组叠加的字符数量
            let mut char_count = LineParts::<usize>::empty();
            let mut char_curosr_index = 0; // 判断光标在那个 part中

            for (_, s) in parts.iter().enumerate() {
                let count = (*s).chars().count();
                char_count.append(count);
            }
            log::debug!("char_count parts:{:?}", char_count.as_parts());
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
    for (i, x) in str_parts.iter().enumerate() {
        log::debug!("i:{} | part content:{:?}", i, String::from_utf8_lossy(x));
    }
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
        log::debug!("highlight char found:{:?}", b);
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
    log::debug!(
        "cursor_x:{}, byte_cursor:{}, last_char_bytes_size:{},char_curosr_index:{}",
        cursor_x,
        byte_cursor,
        last_char_bytes_size,
        char_curosr_index
    );
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
fn build_nav_text(line_meta: &RingVec<LineState>, height: usize) -> Text {
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
//     line_meta: &'a RingVec<LineState>,
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
