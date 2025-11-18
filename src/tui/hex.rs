use crate::byteutil::ByteView;
use crate::byteutil::Endian;
use crate::cli::UIType;
use crate::common::error::ChapResult;
use crate::common::ring_vec::RingVec;
use crate::handle::Handle;
use crate::handle::HandleEdit;
use crate::handle::HandleHex;
use crate::handle::HandleImpl;
use crate::lua::LuaPlugin;
use crate::textwarp::edit::GapText;
use crate::textwarp::hex::HexText;
use crate::textwarp::text::MmapText;
use crate::textwarp::CacheStr;
use crate::textwarp::EditLineMeta;
use crate::textwarp::EditTextWarp;
use crate::textwarp::TextDisplay;
use crate::textwarp::TextOper;
use crate::textwarp::TextWarp;
use crate::textwarp::TextWarpType;
use crate::tui::TextSelect;
// use crate::textwarp::LineMeta;
use const_hex::Buffer;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::execute;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::{
    cursor,
    event::{self, KeyCode},
    ExecutableCommand,
};
use ratatui::init;
use ratatui::prelude::Constraint;
use ratatui::prelude::CrosstermBackend;
use ratatui::prelude::Direction;
use ratatui::prelude::Layout;
use ratatui::prelude::Rect;
use ratatui::prelude::Size;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use std::io;
use std::mem;
use std::path::Path;
use std::process::exit;
use tokio::sync::mpsc;

//u8类型
enum U8Category {
    Null,
    AsciiPrintable,
    AsciiWhitespace,
    AsciiOther,
    NonAscii,
}

impl U8Category {
    fn color(self) -> Color {
        match self {
            U8Category::Null => Color::LightRed,
            U8Category::AsciiPrintable => Color::LightGreen,
            U8Category::AsciiWhitespace => Color::LightBlue,
            U8Category::AsciiOther => Color::Yellow,
            U8Category::NonAscii => Color::White,
        }
    }
}

struct Byte(u8);

impl Byte {
    fn category(self) -> U8Category {
        if self.0 == 0x00 {
            U8Category::Null
        } else if self.0.is_ascii_alphanumeric()
            || self.0.is_ascii_punctuation()
            || self.0.is_ascii_graphic()
        {
            U8Category::AsciiPrintable
        } else if self.0.is_ascii_whitespace() {
            U8Category::AsciiWhitespace
        } else if self.0.is_ascii() {
            U8Category::AsciiOther
        } else {
            U8Category::NonAscii
        }
    }
}

fn n_chars(s: &str, n: usize) -> (&str, &str, &str) {
    // 使用 char_indices 获取每个字符的起始字节位置
    let mut iter = s.char_indices();
    // 获取第 n 个字符的起始字节位置；如果不存在则取整个字符串长度
    let start = iter.nth(n).map(|(idx, _)| idx).unwrap_or(s.len());
    let end = iter.next().map(|(i, _)| i).unwrap_or(s.len());
    (&s[..start], &s[start..end], &s[end..])
}

fn bytes_to_string_with_dot(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b.is_ascii() && !b.is_ascii_control() {
                b as char
            } else {
                '.'
            }
        })
        .collect()
}

fn format_hex_slice(slice: &[u8], j: &mut usize) -> String {
    let mut line = String::with_capacity(slice.len() * 3); // Adjust capacity based on expected size
    for b in slice.iter() {
        let mut buffer = Buffer::<1>::new();
        let c = buffer.format(&[*b]);
        line.push_str(c);
        line.push_str(if (*j + 1) % 8 == 0 { "  " } else { " " });
        *j += 1;
    }
    line
}

type ParserFn = fn(&ByteView) -> String;

fn format_data_inspector<T: std::fmt::Display>(data: T) -> String {
    format!("{:<40}|", data)
}

static FIELDS: &[(&str, ParserFn)] = &[
    ("| Binary (8bit)      | ", |bv| {
        format_data_inspector(bv.to_binary_8bit())
    }),
    ("| Binary Len         | ", |bv| {
        format_data_inspector(bv.len())
    }),
    ("| uint8_t            | ", |bv| {
        format_data_inspector(bv.to_u8())
    }),
    ("| uint16_t           | ", |bv| {
        format_data_inspector(bv.to_u16())
    }),
    ("| int16_t            | ", |bv| {
        format_data_inspector(bv.to_i16())
    }),
    ("| uint32_t           | ", |bv| {
        format_data_inspector(bv.to_u32())
    }),
    ("| int32_t            | ", |bv| {
        format_data_inspector(bv.to_i32())
    }),
    ("| uint64_t           | ", |bv| {
        format_data_inspector(bv.to_u64())
    }),
    ("| int64_t            | ", |bv| {
        format_data_inspector(bv.to_i64())
    }),
    ("| half float(f16)    | ", |bv| {
        format_data_inspector(bv.to_f16())
    }),
    ("| float              | ", |bv| {
        format_data_inspector(bv.to_f32())
    }),
    ("| double             | ", |bv| {
        format_data_inspector(bv.to_f64())
    }),
    ("| String             | ", |bv| {
        format_data_inspector(bv.to_str())
    }),
    ("| pgvarint           | ", |bv| {
        format_data_inspector(bv.to_varlena())
    }),
];

pub(crate) fn get_data_inspector_content<'a>(
    seek: usize,
    buf: Vec<u8>,
    endian: Endian,
) -> Text<'a> {
    let bv = ByteView::new(buf, endian);
    let mut lines = vec![
        Line::from(Span::styled(
            "Data Inspector",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("| address            | ", Style::default().fg(Color::White)),
            Span::raw(seek.to_string()),
        ]),
    ];
    lines.extend(FIELDS.iter().map(|&(label, f)| {
        let spans = vec![
            Span::styled(label, Style::default().fg(Color::White)),
            Span::raw(f(&bv)),
        ];
        Line::from(spans)
    }));

    let text = Text::from(lines);
    text
}

const HEX_TOP: &'static str = "00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F     ASCII";

pub(crate) fn get_hex_content<'a>(
    txts: &'a RingVec<CacheStr>,
    line_meta: &'a RingVec<EditLineMeta>,
    cur_line: usize,
    hex_sel: &TextSelect,
    height: usize,
    cursor_y: usize,
    cursor_x: usize,
) -> (Text<'a>, Text<'a>) {
    let mut lines = Vec::with_capacity(line_meta.len() + 1);
    let mut buffer = Buffer::<1>::new();

    let top = Span::styled(
        HEX_TOP,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    lines.push(Line::from(top));
    lines.push(Line::from(""));
    for (i, txt) in txts.iter().enumerate() {
        let (slice1, slice2) = txt.as_slice();
        let mut spans = Vec::with_capacity(slice1.len() + slice2.len());
        let mut str_spans = Vec::with_capacity(slice1.len() + slice2.len());
        let mut j = 0;
        if cursor_y == i {
            for b in slice1.iter() {
                let category = Byte(*b).category();
                let color = category.color();
                let c = buffer.format(&[*b]);
                let space = if j != 0 && (j + 1) % 8 == 0 {
                    "  "
                } else {
                    " "
                };

                let b1 = if b.is_ascii() && !b.is_ascii_control() {
                    (*b as char).to_string()
                } else {
                    '.'.to_string()
                };

                if hex_sel.has_selected() {
                    if hex_sel.is_selected(line_meta.get(i).unwrap().get_line_file_start() + j) {
                        spans.push(Span::styled(
                            c.to_string().to_uppercase(),
                            Style::default().fg(color).bg(Color::DarkGray),
                        ));
                        spans.push(Span::styled(space, Style::default().bg(Color::DarkGray)));
                        str_spans.push(Span::styled(b1, Style::default().bg(Color::DarkGray)));
                    } else {
                        spans.push(Span::styled(
                            c.to_string().to_uppercase(),
                            Style::default().fg(color),
                        ));
                        spans.push(Span::raw(space));
                        str_spans.push(Span::raw(b1));
                    }
                } else {
                    if j == cursor_x {
                        spans.push(Span::styled(
                            c.to_string().to_uppercase(),
                            Style::default().fg(color).bg(Color::DarkGray),
                        ));
                        str_spans.push(Span::styled(b1, Style::default().bg(Color::DarkGray)));
                    } else {
                        spans.push(Span::styled(
                            c.to_string().to_uppercase(),
                            Style::default().fg(color),
                        ));
                        str_spans.push(Span::raw(b1));
                    }
                    spans.push(Span::raw(space));
                }
                j += 1;
            }

            for b in slice2.iter() {
                let category = Byte(*b).category();
                let color = category.color();
                let c = buffer.format(&[*b]);
                let space = if j != 0 && (j + 1) % 8 == 0 {
                    "  "
                } else {
                    " "
                };

                let b1 = if b.is_ascii() && !b.is_ascii_control() {
                    (*b as char).to_string()
                } else {
                    '.'.to_string()
                };

                if hex_sel.has_selected() {
                    if hex_sel.is_selected(line_meta.get(i).unwrap().get_line_file_start() + j) {
                        spans.push(Span::styled(
                            c.to_string().to_uppercase(),
                            Style::default().fg(color).bg(Color::DarkGray),
                        ));
                        spans.push(Span::styled(space, Style::default().bg(Color::DarkGray)));
                        str_spans.push(Span::styled(b1, Style::default().bg(Color::DarkGray)));
                    } else {
                        spans.push(Span::styled(
                            c.to_string().to_uppercase(),
                            Style::default().fg(color),
                        ));
                        spans.push(Span::raw(space));
                        str_spans.push(Span::raw(b1));
                    }
                } else {
                    if j == cursor_x {
                        spans.push(Span::styled(
                            c.to_string().to_uppercase(),
                            Style::default().fg(color).bg(Color::DarkGray),
                        ));
                        str_spans.push(Span::styled(b1, Style::default().bg(Color::DarkGray)));
                    } else {
                        spans.push(Span::styled(
                            c.to_string().to_uppercase(),
                            Style::default().fg(color),
                        ));
                        str_spans.push(Span::raw(b1));
                    }
                    spans.push(Span::raw(space));
                }
                j += 1;
            }
        } else {
            for b in slice1.iter() {
                let category = Byte(*b).category();
                let color = category.color();
                let c = buffer.format(&[*b]);

                let space = if j != 0 && (j + 1) % 8 == 0 {
                    "  "
                } else {
                    " "
                };
                let b1 = if b.is_ascii() && !b.is_ascii_control() {
                    (*b as char).to_string()
                } else {
                    '.'.to_string()
                };
                if hex_sel.has_selected() {
                    if hex_sel.is_selected(line_meta.get(i).unwrap().get_line_file_start() + j) {
                        spans.push(Span::styled(
                            c.to_string().to_uppercase(),
                            Style::default().fg(color).bg(Color::DarkGray),
                        ));
                        spans.push(Span::styled(space, Style::default().bg(Color::DarkGray)));
                        str_spans.push(Span::styled(b1, Style::default().bg(Color::DarkGray)));
                    } else {
                        spans.push(Span::styled(
                            c.to_string().to_uppercase(),
                            Style::default().fg(color),
                        ));
                        spans.push(Span::raw(space));
                        str_spans.push(Span::raw(b1));
                    }
                } else {
                    spans.push(Span::styled(
                        c.to_string().to_uppercase(),
                        Style::default().fg(color),
                    ));
                    spans.push(Span::raw(space));
                    str_spans.push(Span::raw(b1));
                }
                j += 1;
            }

            for b in slice2.iter() {
                let category = Byte(*b).category();
                let color = category.color();
                let c = buffer.format(&[*b]);
                let space = if j != 0 && (j + 1) % 8 == 0 {
                    "  "
                } else {
                    " "
                };
                let b1 = if b.is_ascii() && !b.is_ascii_control() {
                    (*b as char).to_string()
                } else {
                    '.'.to_string()
                };
                if hex_sel.has_selected() {
                    if hex_sel.is_selected(line_meta.get(i).unwrap().get_line_file_start() + j) {
                        spans.push(Span::styled(
                            c.to_string().to_uppercase(),
                            Style::default().fg(color).bg(Color::DarkGray),
                        ));
                        spans.push(Span::styled(space, Style::default().bg(Color::DarkGray)));
                        str_spans.push(Span::styled(b1, Style::default().bg(Color::DarkGray)));
                    } else {
                        spans.push(Span::styled(
                            c.to_string().to_uppercase(),
                            Style::default().fg(color),
                        ));
                        spans.push(Span::raw(space));
                        str_spans.push(Span::raw(b1));
                    }
                } else {
                    spans.push(Span::styled(
                        c.to_string().to_uppercase(),
                        Style::default().fg(color),
                    ));
                    spans.push(Span::raw(space));
                    str_spans.push(Span::raw(b1));
                }
                j += 1;
            }
        }

        spans.push(Span::raw(
            "   ".repeat(16_usize.saturating_sub(txt.len()) + 1),
        ));
        spans.extend_from_slice(&str_spans);
        lines.push(Line::from(spans));
    }
    if cursor_y >= line_meta.len() {
        let diff = cursor_y - line_meta.len();
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
    let nav_text = Text::from(
        (0..height)
            .enumerate()
            .map(|(i, _)| {
                if i == 0 {
                    return Line::from(Span::raw("Address"));
                }
                if i == 1 {
                    return Line::from(Span::raw(""));
                }
                if i - 2 >= line_meta.len() {
                    return Line::raw(" ");
                }
                Line::from(Span::styled(
                    format!(
                        "{:07x}",
                        line_meta.get(i - 2).unwrap().get_line_file_start()
                    ),
                    Style::default().fg(Color::White),
                ))
            })
            .collect::<Vec<Line>>(),
    );

    let text = Text::from(lines);
    (nav_text, text)
}
