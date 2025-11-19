use crate::byteutil::ByteView;
use crate::byteutil::Endian;
use crate::common::ring_vec::RingVec;
use crate::textwarp::CacheStr;
use crate::textwarp::EditLineMeta;
use crate::tui::TextSelect;
use const_hex::Buffer;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;

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

    // 添加头部
    let top = Span::styled(
        HEX_TOP,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    lines.push(Line::from(top));
    lines.push(Line::from(""));

    // 处理每一行文本
    for (i, txt) in txts.iter().enumerate() {
        let (slice1, slice2) = txt.as_slice();
        let line_start = line_meta
            .get(i)
            .map(|meta| meta.get_line_file_start())
            .unwrap_or(0);

        let (hex_spans, char_spans) = process_line_bytes(
            slice1,
            slice2,
            line_start,
            hex_sel,
            cursor_y,
            i,
            cursor_x,
            &mut buffer,
        );

        // 添加填充和字符显示
        let padding = "   ".repeat(16_usize.saturating_sub(txt.len()) + 1);
        let mut all_spans = hex_spans;
        all_spans.push(Span::raw(padding));
        all_spans.extend(char_spans);
        lines.push(Line::from(all_spans));
    }

    // 处理光标超出范围的情况
    add_cursor_padding(&mut lines, cursor_y, line_meta.len(), cursor_x);

    let nav_text = create_navigation_text(height, line_meta);
    let text = Text::from(lines);

    (nav_text, text)
}

/// 处理单行字节数据，生成十六进制和字符spans
fn process_line_bytes<'a>(
    slice1: &[u8],
    slice2: &[u8],
    line_start: usize,
    hex_sel: &TextSelect,
    cursor_y: usize,
    current_line: usize,
    cursor_x: usize,
    buffer: &mut Buffer<1>,
) -> (Vec<Span<'a>>, Vec<Span<'a>>) {
    let mut hex_spans = Vec::new();
    let mut char_spans = Vec::new();
    let mut byte_index = 0;

    // 处理所有字节（slice1和slice2）
    for byte_slice in [slice1, slice2] {
        for byte in byte_slice {
            let global_pos = line_start + byte_index;
            let highlight = should_highlight(
                hex_sel,
                global_pos,
                cursor_y,
                current_line,
                byte_index,
                cursor_x,
            );

            let category = Byte(*byte).category();
            let color = category.color();
            let hex_str = buffer.format(&[*byte]).to_uppercase();
            let space = if byte_index != 0 && (byte_index + 1) % 8 == 0 {
                "  "
            } else {
                " "
            };
            let char_repr = if byte.is_ascii() && !byte.is_ascii_control() {
                (*byte as char).to_string()
            } else {
                '.'.to_string()
            };

            let (hex_span, char_span) = create_byte_spans(hex_str, char_repr, color, highlight);
            hex_spans.push(hex_span);
            hex_spans.push(Span::raw(space));
            char_spans.push(char_span);

            byte_index += 1;
        }
    }

    (hex_spans, char_spans)
}

/// 判断是否应该高亮显示字节
fn should_highlight(
    hex_sel: &TextSelect,
    global_pos: usize,
    cursor_y: usize,
    current_line: usize,
    current_x: usize,
    cursor_x: usize,
) -> bool {
    if hex_sel.has_selected() {
        hex_sel.is_selected(global_pos)
    } else {
        cursor_y == current_line && current_x == cursor_x
    }
}

/// 创建单个字节的十六进制和字符spans
fn create_byte_spans<'a>(
    hex_str: String,
    char_repr: String,
    color: Color,
    highlight: bool,
) -> (Span<'a>, Span<'a>) {
    if highlight {
        let highlight_style = Style::default().fg(color).bg(Color::DarkGray);
        (
            Span::styled(hex_str, highlight_style),
            Span::styled(char_repr, highlight_style),
        )
    } else {
        (
            Span::styled(hex_str, Style::default().fg(color)),
            Span::raw(char_repr),
        )
    }
}

/// 添加光标超出范围时的填充行
fn add_cursor_padding(lines: &mut Vec<Line>, cursor_y: usize, line_count: usize, cursor_x: usize) {
    if cursor_y >= line_count {
        let diff = cursor_y - line_count;
        for _ in 0..diff {
            lines.push(Line::raw(""));
        }
        let padding = " ".repeat(cursor_x);
        let cursor_span = Span::styled(" ", Style::default().bg(Color::LightRed));
        lines.push(Line::from(vec![Span::raw(padding), cursor_span]));
    }
}

/// 创建左侧导航地址文本
fn create_navigation_text(height: usize, line_meta: &RingVec<EditLineMeta>) -> Text<'_> {
    Text::from(
        (0..height)
            .enumerate()
            .map(|(i, _)| match i {
                0 => Line::from(Span::raw("Address")),
                1 => Line::from(Span::raw("")),
                _ if i - 2 < line_meta.len() => {
                    let start = line_meta.get(i - 2).unwrap().get_line_file_start();
                    Line::from(Span::styled(
                        format!("{:07x}", start),
                        Style::default().fg(Color::White),
                    ))
                }
                _ => Line::raw(" "),
            })
            .collect::<Vec<Line>>(),
    )
}
