use crate::byteutil::ByteView;
use crate::byteutil::Endian;
use crate::common::ring_vec::RingVec;
use crate::textwarp::CacheStr;
use crate::textwarp::LineState;
use crate::textwarp::TextSelect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;

// ========== 编译期静态查找表 ==========

/// 十六进制大写字节对表
/// HEX_UPPER_TABLE[i] = [高位ASCII, 低位ASCII]
const HEX_UPPER_TABLE: [[u8; 2]; 256] = {
    let mut t = [[0u8; 2]; 256];
    let mut i = 0usize;
    while i < 256 {
        let hi = (i >> 4) as u8;
        let lo = (i & 0xf) as u8;
        t[i][0] = if hi < 10 { b'0' + hi } else { b'A' + hi - 10 };
        t[i][1] = if lo < 10 { b'0' + lo } else { b'A' + lo - 10 };
        i += 1;
    }
    t
};

/// ASCII 字符显示表
/// 可打印 ASCII（0x20–0x7E）保留原字节，其余存 b'.'
const CHAR_REPR_TABLE: [u8; 256] = {
    let mut t = [b'.'; 256];
    let mut i = 0x20usize;
    while i <= 0x7e {
        t[i] = i as u8;
        i += 1;
    }
    t
};

/// 行末填充字符串表，共 17 档
/// PADDING_TABLE[i] = "   ".repeat(i + 1)
/// 用法：PADDING_TABLE[16.saturating_sub(line_byte_count)]
const PADDING_TABLE: [&str; 17] = [
    "   ",                                                               //  3 spaces (×1)
    "      ",                                                            //  6 spaces (×2)
    "         ",                                                         //  9 spaces (×3)
    "            ",                                                      // 12 spaces (×4)
    "               ",                                                   // 15 spaces (×5)
    "                  ",                                                // 18 spaces (×6)
    "                     ",                                             // 21 spaces (×7)
    "                        ",                                          // 24 spaces (×8)
    "                           ",                                       // 27 spaces (×9)
    "                              ",                                    // 30 spaces (×10)
    "                                 ",                                 // 33 spaces (×11)
    "                                    ",                              // 36 spaces (×12)
    "                                       ",                           // 39 spaces (×13)
    "                                          ",                        // 42 spaces (×14)
    "                                             ",                     // 45 spaces (×15)
    "                                                ",                  // 48 spaces (×16)
    "                                                   ",               // 51 spaces (×17)
];

// ========== 优化 1+2：编译期 Style 查找表 ==========
// 替代 Byte(*byte).category().color() + Style::default().fg(color) 的每字节双重构造
//
// 字节分类规则（与原 U8Category 完全一致）：
//   0x00              → Null        → LightRed
//   0x01–0x08         → AsciiOther  → Yellow   (ASCII 控制，非空白)
//   0x09 \t           → Whitespace  → LightBlue
//   0x0A \n           → Whitespace  → LightBlue
//   0x0B \v           → AsciiOther  → Yellow   (Rust is_ascii_whitespace 不含 0x0B)
//   0x0C \f           → Whitespace  → LightBlue
//   0x0D \r           → Whitespace  → LightBlue
//   0x0E–0x1F         → AsciiOther  → Yellow
//   0x20 space        → Whitespace  → LightBlue
//   0x21–0x7E         → Printable   → LightGreen
//   0x7F DEL          → AsciiOther  → Yellow
//   0x80–0xFF         → NonAscii    → White

// 5 种基础 Style（无高亮，仅前景色）
const S_NULL: Style = Style {
    fg: Some(Color::LightRed),
    bg: None,
    underline_color: None,
    add_modifier: Modifier::empty(),
    sub_modifier: Modifier::empty(),
};
const S_GREEN: Style = Style {
    fg: Some(Color::LightGreen),
    bg: None,
    underline_color: None,
    add_modifier: Modifier::empty(),
    sub_modifier: Modifier::empty(),
};
const S_BLUE: Style = Style {
    fg: Some(Color::LightBlue),
    bg: None,
    underline_color: None,
    add_modifier: Modifier::empty(),
    sub_modifier: Modifier::empty(),
};
const S_YELLOW: Style = Style {
    fg: Some(Color::Yellow),
    bg: None,
    underline_color: None,
    add_modifier: Modifier::empty(),
    sub_modifier: Modifier::empty(),
};
const S_WHITE: Style = Style {
    fg: Some(Color::White),
    bg: None,
    underline_color: None,
    add_modifier: Modifier::empty(),
    sub_modifier: Modifier::empty(),
};

// 5 种高亮 Style（前景色相同，加背景色 DarkGray）
const S_NULL_HL: Style = Style {
    fg: Some(Color::LightRed),
    bg: Some(Color::DarkGray),
    underline_color: None,
    add_modifier: Modifier::empty(),
    sub_modifier: Modifier::empty(),
};
const S_GREEN_HL: Style = Style {
    fg: Some(Color::LightGreen),
    bg: Some(Color::DarkGray),
    underline_color: None,
    add_modifier: Modifier::empty(),
    sub_modifier: Modifier::empty(),
};
const S_BLUE_HL: Style = Style {
    fg: Some(Color::LightBlue),
    bg: Some(Color::DarkGray),
    underline_color: None,
    add_modifier: Modifier::empty(),
    sub_modifier: Modifier::empty(),
};
const S_YELLOW_HL: Style = Style {
    fg: Some(Color::Yellow),
    bg: Some(Color::DarkGray),
    underline_color: None,
    add_modifier: Modifier::empty(),
    sub_modifier: Modifier::empty(),
};
const S_WHITE_HL: Style = Style {
    fg: Some(Color::White),
    bg: Some(Color::DarkGray),
    underline_color: None,
    add_modifier: Modifier::empty(),
    sub_modifier: Modifier::empty(),
};

/// 普通 Style 查找表：STYLE_TABLE[byte] → 该字节的前景色 Style
/// 替代每字节 Byte(b).category().color() + Style::default().fg(color)
const STYLE_TABLE: [Style; 256] = {
    let mut t = [S_WHITE; 256];
    t[0x00] = S_NULL;
    let mut i = 0x01usize;
    while i <= 0x08 {
        t[i] = S_YELLOW;
        i += 1;
    }
    t[0x09] = S_BLUE;
    t[0x0A] = S_BLUE;
    t[0x0B] = S_YELLOW; // \v 不在 Rust is_ascii_whitespace 范围内
    t[0x0C] = S_BLUE;
    t[0x0D] = S_BLUE;
    let mut i = 0x0Eusize;
    while i <= 0x1F {
        t[i] = S_YELLOW;
        i += 1;
    }
    t[0x20] = S_BLUE;
    let mut i = 0x21usize;
    while i <= 0x7E {
        t[i] = S_GREEN;
        i += 1;
    }
    t[0x7F] = S_YELLOW;
    // 0x80–0xFF 已为 S_WHITE
    t
};

/// 高亮 Style 查找表：STYLE_HL_TABLE[byte] → 同前景色 + bg=DarkGray
/// 替代每字节 Style::default().fg(color).bg(Color::DarkGray)
const STYLE_HL_TABLE: [Style; 256] = {
    let mut t = [S_WHITE_HL; 256];
    t[0x00] = S_NULL_HL;
    let mut i = 0x01usize;
    while i <= 0x08 {
        t[i] = S_YELLOW_HL;
        i += 1;
    }
    t[0x09] = S_BLUE_HL;
    t[0x0A] = S_BLUE_HL;
    t[0x0B] = S_YELLOW_HL;
    t[0x0C] = S_BLUE_HL;
    t[0x0D] = S_BLUE_HL;
    let mut i = 0x0Eusize;
    while i <= 0x1F {
        t[i] = S_YELLOW_HL;
        i += 1;
    }
    t[0x20] = S_BLUE_HL;
    let mut i = 0x21usize;
    while i <= 0x7E {
        t[i] = S_GREEN_HL;
        i += 1;
    }
    t[0x7F] = S_YELLOW_HL;
    // 0x80–0xFF 已为 S_WHITE_HL
    t
};

/// 零拷贝取字节对应大写十六进制 &'static str（借用编译期静态表）
#[inline]
fn byte_to_hex(b: u8) -> &'static str {
    // SAFETY：HEX_UPPER_TABLE 所有条目均由 b'0'–b'9' / b'A'–b'F' 构成，为合法 UTF-8
    unsafe { std::str::from_utf8_unchecked(&HEX_UPPER_TABLE[b as usize]) }
}

/// 零拷贝取字节对应字符显示 &'static str（借用编译期静态表）
#[inline]
fn byte_to_char_repr(b: u8) -> &'static str {
    // SAFETY：CHAR_REPR_TABLE 所有条目均为合法 ASCII 字节（0x20–0x7E 或 b'.'）
    unsafe { std::str::from_utf8_unchecked(std::slice::from_ref(&CHAR_REPR_TABLE[b as usize])) }
}

// ========== Data Inspector（不涉及主渲染热路径，保持不变） ==========

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

// ========== 主渲染函数 ==========

const HEX_TOP: &'static str = "00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F     ASCII";

pub(crate) fn get_hex_content<'a>(
    txts: &'a RingVec<CacheStr>,
    line_meta: &'a RingVec<LineState>,
    cur_line: usize,
    hex_sel: &TextSelect,
    height: usize,
    cursor_y: usize,
    cursor_x: usize,
) -> (Text<'a>, Text<'a>) {
    let mut lines = Vec::with_capacity(line_meta.len() + 2);

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
    // 优化 3：process_line_bytes 直接返回合并后的完整 all_spans，
    //         省去 char_spans 临时 Vec 的分配和 extend 追加
    for (i, txt) in txts.iter().enumerate() {
        let part = txt.as_slice();
        let (slice1, slice2) = part.as_2parts();
        let line_start = line_meta
            .get(i)
            .map(|meta| meta.get_line_file_start())
            .unwrap_or(0);

        let all_spans = process_line_bytes(
            slice1,
            slice2,
            line_start,
            hex_sel,
            cursor_y,
            i,
            cursor_x,
        );
        lines.push(Line::from(all_spans));
    }

    // 处理光标超出范围的情况
    add_cursor_padding(&mut lines, cursor_y, line_meta.len(), cursor_x);

    let nav_text = create_navigation_text(height, line_meta);
    let text = Text::from(lines);

    (nav_text, text)
}

/// 处理单行字节数据，生成完整的 [hex_spans | padding | char_spans] 合并视图
///
/// 优化 3：直接构建一个 Vec<Span>（顺序：十六进制段 + 填充 + ASCII 字符段）。
/// 优化 4：has_sel 提升到循环外。
/// 优化 1+2：通过 STYLE_TABLE / STYLE_HL_TABLE 直接查表。
/// 优化 5（新）：非光标行快路径——当 !has_sel && cursor_y != current_line 时，
///              该行所有字节 highlight 恒为 false，跳过每字节 highlight 判断，
///              hex span 直接用 STYLE_TABLE，char span 直接用 Span::raw。
fn process_line_bytes<'a>(
    slice1: &[u8],
    slice2: &[u8],
    line_start: usize,
    hex_sel: &TextSelect,
    cursor_y: usize,
    current_line: usize,
    cursor_x: usize,
) -> Vec<Span<'a>> {
    let total = slice1.len() + slice2.len();
    // 容量 = 每字节 2 个 hex span（hex文本 + 空格）+ 1 个 padding span + 每字节 1 个 char span
    let mut spans = Vec::with_capacity(total * 3 + 1);

    let has_sel = hex_sel.has_selected();
    let is_cursor_line = cursor_y == current_line;

    if !has_sel && !is_cursor_line {
        // ─── 快路径：无选区且非光标行，所有字节 highlight 恒为 false ───
        let mut byte_index = 0usize;
        for byte_slice in [slice1, slice2] {
            for byte in byte_slice {
                spans.push(Span::styled(byte_to_hex(*byte), STYLE_TABLE[*byte as usize]));
                spans.push(Span::raw(if (byte_index + 1) & 7 == 0 { "  " } else { " " }));
                byte_index += 1;
            }
        }
        spans.push(Span::raw(PADDING_TABLE[16_usize.saturating_sub(total)]));
        for byte_slice in [slice1, slice2] {
            for byte in byte_slice {
                spans.push(Span::raw(byte_to_char_repr(*byte)));
            }
        }
    } else {
        // ─── 慢路径：有选区或光标行，逐字节判断 highlight ───
        let mut byte_index = 0usize;
        for byte_slice in [slice1, slice2] {
            for byte in byte_slice {
                let highlight = if has_sel {
                    hex_sel.is_selected(line_start + byte_index)
                } else {
                    is_cursor_line && byte_index == cursor_x
                };
                spans.push(Span::styled(
                    byte_to_hex(*byte),
                    if highlight { STYLE_HL_TABLE[*byte as usize] } else { STYLE_TABLE[*byte as usize] },
                ));
                spans.push(Span::raw(if (byte_index + 1) & 7 == 0 { "  " } else { " " }));
                byte_index += 1;
            }
        }
        spans.push(Span::raw(PADDING_TABLE[16_usize.saturating_sub(total)]));
        let mut byte_index = 0usize;
        for byte_slice in [slice1, slice2] {
            for byte in byte_slice {
                let highlight = if has_sel {
                    hex_sel.is_selected(line_start + byte_index)
                } else {
                    is_cursor_line && byte_index == cursor_x
                };
                spans.push(if highlight {
                    Span::styled(byte_to_char_repr(*byte), STYLE_HL_TABLE[*byte as usize])
                } else {
                    Span::raw(byte_to_char_repr(*byte))
                });
                byte_index += 1;
            }
        }
    }

    spans
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
fn create_navigation_text(height: usize, line_meta: &RingVec<LineState>) -> Text<'_> {
    let mut v = Vec::with_capacity(height);
    for i in 0..height {
        v.push(match i {
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
        });
    }
    Text::from(v)
}
