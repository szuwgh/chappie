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
//use vectorbase::collection::Collection;

// pub(crate) enum ChapMod {
//     Edit,   //普通编辑器模式
//     Hex,    //16进制编辑器模式
//     Text,   //大文本浏览模式
//     Vector, //向量分析模式
// }

// //u8类型
// enum U8Category {
//     Null,
//     AsciiPrintable,
//     AsciiWhitespace,
//     AsciiOther,
//     NonAscii,
// }

// impl U8Category {
//     fn color(self) -> Color {
//         match self {
//             U8Category::Null => Color::LightRed,
//             U8Category::AsciiPrintable => Color::LightGreen,
//             U8Category::AsciiWhitespace => Color::LightBlue,
//             U8Category::AsciiOther => Color::Yellow,
//             U8Category::NonAscii => Color::White,
//         }
//     }
// }

// struct Byte(u8);

// impl Byte {
//     fn category(self) -> U8Category {
//         if self.0 == 0x00 {
//             U8Category::Null
//         } else if self.0.is_ascii_alphanumeric()
//             || self.0.is_ascii_punctuation()
//             || self.0.is_ascii_graphic()
//         {
//             U8Category::AsciiPrintable
//         } else if self.0.is_ascii_whitespace() {
//             U8Category::AsciiWhitespace
//         } else if self.0.is_ascii() {
//             U8Category::AsciiOther
//         } else {
//             U8Category::NonAscii
//         }
//     }
// }

// pub(crate) struct TuiElement {
//     pub(crate) navi: Navigation,
//     pub(crate) tv: TextView,
//     pub(crate) cmd_title: Rect,
//     pub(crate) cmd_inp: CmdInput,
//     pub(crate) assist_tv1: TextView,
//     pub(crate) assist_tv2: TextView,
// }

// pub(crate) struct ChapTui {
//     chap_mod: ChapMod,
//     ui_type: UIType,
//     size: Size,
//     pub(crate) warp_type: TextWarpType,
//     pub(crate) terminal: Terminal<CrosstermBackend<io::Stdout>>,
//     pub(crate) elem: TuiElement,
//     pub(crate) back_linenum: Vec<usize>, // 上一行号
//     pub(crate) txt_sel: TextSelect,      // 文本选择
//     pub(crate) cursor_x: usize,          // 光标x坐标
//     pub(crate) cursor_y: usize,          // 光标y坐标
//     pub(crate) column_offset: usize,     // 列偏移量
//     pub(crate) bytes_cursor: usize,      //字节偏移量
//     pub(crate) bytes_cursor_size: usize, //字节偏移量
//     pub(crate) start_line_num: usize,    // 起始行号
//     pub(crate) is_last_line: bool,       // 是否是最后一行
//     pub(crate) endian: Endian,           // 字节序
//     pub(crate) assist_tv2_data: String,
// }

// // 文本编辑器大文件浏览 窗口
// pub(crate) struct TextWindow {
//     navi: Navigation, //导航
//     tv: TextView,
//     cmd_inp: CmdInput,
// }

// // 16进制编辑窗口
// pub(crate) struct HexWindow {
//     tv: TextView,
//     cmd_inp: CmdInput,
// }

// pub(crate) struct AiChatWindow {}

// pub(crate) struct TerminalWindow {}

// enum FocusType {
//     TxtFuzzy,
//     Chat,
// }

// //焦点
// struct Focus {
//     current_focus: usize,
// }

// impl Focus {
//     // 创建一个新的 Focus 实例
//     fn new() -> Self {
//         Focus { current_focus: 0 }
//     }

//     // 切换焦点
//     fn next(&mut self) {
//         self.current_focus = (self.current_focus + 1) % mem::variant_count::<FocusType>();
//         // 0到3循环
//     }

//     fn get_colors(&self) -> (Color, Color, Color, Color) {
//         let base = Color::White;
//         let highlight = Color::Yellow;
//         match self.current() {
//             FocusType::TxtFuzzy => (highlight, highlight, base, base),
//             FocusType::Chat => (base, base, highlight, highlight),
//         }
//     }

//     // 获取当前焦点
//     fn current(&self) -> FocusType {
//         match self.current_focus {
//             0 => FocusType::TxtFuzzy,
//             1 => FocusType::Chat,
//             _ => {
//                 todo!()
//             }
//         }
//     }
// }

// struct Prompt {
//     prompt: String,
//     _id: String,
// }

// impl Prompt {
//     fn prompt(&self) -> &str {
//         &self.prompt
//     }
//     fn _id(&self) -> &str {
//         &self._id
//     }
// }

// #[derive(Debug)]
// struct ChatItemIndex(usize, usize);

// impl ChatItemIndex {
//     fn start(&self) -> usize {
//         self.0
//     }
//     fn end(&self) -> usize {
//         self.1
//     }
// }

// #[derive(Debug, Clone)]
// pub(crate) struct TextSelect(usize, usize);

// impl TextSelect {
//     fn new() -> Self {
//         TextSelect(0, 0)
//     }

//     pub(crate) fn from_select(start: usize, end: usize) -> Self {
//         TextSelect(start, end)
//     }

//     fn start(&self) -> usize {
//         self.0
//     }
//     fn end(&self) -> usize {
//         self.1
//     }

//     fn len(&self) -> usize {
//         self.end() - self.start()
//     }

//     fn inc_end(&mut self) {
//         self.1 += 1;
//     }

//     fn has_selected(&self) -> bool {
//         self.start() < self.end()
//     }

//     fn is_selected(&self, pos: usize) -> bool {
//         pos >= self.start() && pos <= self.end()
//     }

//     // 递减end
//     fn dec_end(&mut self) {
//         if self.1 > self.0 {
//             self.1 -= 1;
//         }
//     }

//     pub(crate) fn reset_to_start(&mut self) {
//         self.1 = self.0;
//     }

//     pub(crate) fn set_pos(&mut self, pos: usize) {
//         self.0 = pos;
//         self.1 = pos;
//     }

//     pub(crate) fn get_start(&self) -> usize {
//         self.0
//     }

//     pub(crate) fn get_end(&self) -> usize {
//         self.1
//     }

//     pub(crate) fn set_start(&mut self, start: usize) {
//         self.0 = start;
//     }

//     pub(crate) fn set_end(&mut self, end: usize) {
//         self.1 = end;
//     }

//     pub(crate) fn set_select(&mut self, start: usize, end: usize) {
//         self.0 = start;
//         self.1 = end;
//     }
// }

// #[derive(Default)]
// // 聊天框的类型
// enum ChatType {
//     #[default]
//     ChatTv,
//     Promt,
//     Pattern,
// }

// pub(crate) struct Navigation {
//     min_line: usize,
//     max_line: usize,
//     cur_line: usize,
//     select_line: Option<(usize, usize)>,
//     rect: Rect,
// }

// impl Navigation {
//     pub(crate) fn clear(&mut self) {
//         self.select_line = None;
//     }

//     fn get_rect(&self) -> Rect {
//         self.rect
//     }

//     fn is_top(&self) -> bool {
//         self.cur_line == self.min_line
//     }

//     fn is_bottom(&self) -> bool {
//         self.cur_line == self.max_line
//     }

//     fn down_line(&mut self) {
//         if self.cur_line < self.max_line {
//             self.cur_line += 1;
//         }
//     }

//     fn up_line(&mut self) {
//         if self.cur_line > self.min_line {
//             self.cur_line -= 1;
//         }
//     }

//     fn get_cur_line(&self) -> usize {
//         self.cur_line
//     }

//     fn to_min_line(&mut self) {
//         self.cur_line = self.min_line
//     }

//     fn to_max_line(&mut self) {
//         self.cur_line = self.max_line
//     }

//     fn set_cur_line(&mut self, cur_line: usize) {
//         self.cur_line = cur_line
//     }
// }

// pub(crate) struct TextView {
//     height: usize,
//     width: usize,
//     scroll: usize, //当前页 第一行 行数
//     rect: Rect,
// }

// impl TextView {
//     fn get_rect(&self) -> Rect {
//         self.rect
//     }

//     pub(crate) fn get_height(&self) -> usize {
//         self.height
//     }

//     pub(crate) fn get_width(&self) -> usize {
//         self.width
//     }

//     fn get_scroll(&self) -> usize {
//         self.scroll
//     }

//     fn set_scroll(&mut self, scroll: usize) {
//         self.scroll = scroll
//     }

//     fn up_line(&mut self) {
//         self.scroll = (self.scroll - 1).max(1);
//     }

//     fn down_line(&mut self, max_num: Option<usize>) {
//         if let Some(max_scroll_num) = max_num {
//             if self.scroll <= max_scroll_num {
//                 self.scroll += 1;
//             }
//         } else {
//             self.scroll += 1;
//         }
//     }

//     fn up_page(&mut self) {
//         if self.scroll > self.height {
//             self.scroll = (self.scroll - self.height).max(1);
//         } else {
//             self.scroll = 1;
//         }
//     }

//     fn down_page(&mut self, max_num: Option<usize>) {
//         if let Some(max_scroll_num) = max_num {
//             self.scroll = ((self.scroll + self.height).min(max_scroll_num)).max(1)
//         } else {
//             self.scroll += self.height;
//         }
//     }
// }

// pub(crate) struct CmdInput {
//     input: String,
//     rect: Rect,
// }

// impl CmdInput {
//     pub(crate) fn new(rect: Rect) -> Self {
//         CmdInput {
//             input: String::new(),
//             rect,
//         }
//     }

//     fn get_rect(&self) -> Rect {
//         self.rect
//     }

//     pub(crate) fn clear(&mut self) {
//         self.input.clear();
//     }

//     pub(crate) fn push(&mut self, c: char) {
//         self.input.push(c);
//     }

//     pub(crate) fn push_str(&mut self, c: &str) {
//         self.input.push_str(c);
//     }

//     pub(crate) fn pop(&mut self) {
//         self.input.pop();
//     }

//     pub(crate) fn len(&mut self) -> usize {
//         self.input.len()
//     }

//     pub(crate) fn get_inp(&self) -> &str {
//         &self.input
//     }

//     fn get_inp_exact(&self) -> (&str, bool) {
//         return if let Some(first_char) = &self.input.chars().next() {
//             if *first_char == '/' {
//                 (&self.input[1..].trim(), false)
//             } else {
//                 (&self.input.trim(), true)
//             }
//         } else {
//             (&self.input.trim(), true)
//         };
//     }
// }

struct ChatText {
    height: usize,
    width: usize,
    scroll: usize,
    rect: Rect,
}

impl ChatText {
    fn get_rect(&self) -> Rect {
        self.rect
    }

    fn get_height(&self) -> usize {
        self.height
    }

    fn get_width(&self) -> usize {
        self.width
    }
    fn get_scroll(&self) -> usize {
        self.scroll
    }

    fn set_scroll(&mut self, scroll: usize) {
        self.scroll = scroll
    }

    fn up_line(&mut self) {
        self.scroll = (self.scroll - 1).max(1);
    }

    fn down_line(&mut self, max_num: Option<usize>) {
        if let Some(max_scroll_num) = max_num {
            if self.scroll <= max_scroll_num {
                self.scroll += 1;
            }
        } else {
            self.scroll += 1;
        }
    }

    fn up_page(&mut self) {
        if self.scroll > self.height {
            self.scroll = (self.scroll - self.height).max(1);
        } else {
            self.scroll = 1;
        }
    }

    fn down_page(&mut self, max_num: Option<usize>) {
        if let Some(max_scroll_num) = max_num {
            self.scroll = (self.scroll + self.height).min(max_scroll_num)
        } else {
            self.scroll += self.height - 1;
        }
    }
}

pub(crate) struct ChatInput {
    input: String,
    rect: Rect,
}

impl ChatInput {
    fn get_rect(&self) -> Rect {
        self.rect
    }
    pub(crate) fn clear(&mut self) {
        self.input.clear();
    }

    fn push(&mut self, c: char) {
        self.input.push(c);
    }

    fn pop(&mut self) {
        self.input.pop();
    }

    fn get_inp(&self) -> &str {
        &self.input
    }
}

// fn get_chat_content<'a>(
//     txts: &Vec<&'a str>,
//     line_meta: &Vec<LineMeta>,
//     chat_item: &ChatItemIndex,
// ) -> Text<'a> {
//     let mut lines = Vec::with_capacity(line_meta.len());
//     //  debug!("{:?},{:?}", line_meta, chat_item);
//     for (i, txt) in txts.into_iter().enumerate() {
//         let line_num = line_meta[i].get_line_num();
//         // debug!("text: {:?}", *text);
//         if line_num >= chat_item.start() && line_num <= chat_item.end() {
//             lines.push(Line::from(Span::styled(
//                 *txt,
//                 Style::default().fg(Color::Green),
//             )));
//         } else {
//             lines.push(Line::from(*txt));
//         }
//     }
//     let text = Text::from(lines);
//     text
// }

// fn n_chars_skip_control_mem_opt(s: &str, n: usize) -> (&str, &str, &str, usize) {
//     let mut count = 0;
//     let mut start_idx = None;
//     let mut end_idx = None;
//     let mut last_start_idx = None;

//     for (idx, ch) in s.char_indices() {
//         if ch.is_control() {
//             continue;
//         }
//         if n > 0 && count == n - 1 {
//             last_start_idx = Some(idx);
//         }
//         if count == n {
//             // 第 n 个非控制字符
//             start_idx = Some(idx);
//         }
//         if count == n + 1 {
//             // 第 n+1 个非控制字符
//             end_idx = Some(idx);
//             break;
//         }
//         count += 1;
//     }

//     // 如果 never set, 默认到末尾
//     let last_start = last_start_idx.unwrap_or_else(|| 0);
//     let start = start_idx.unwrap_or_else(|| s.len());
//     let end = end_idx.unwrap_or_else(|| s.len());

//     (&s[..start], &s[start..end], &s[end..], start - last_start)
// }

// fn n_chars(s: &str, n: usize) -> (&str, &str, &str) {
//     // 使用 char_indices 获取每个字符的起始字节位置
//     let mut iter = s.char_indices();
//     // 获取第 n 个字符的起始字节位置；如果不存在则取整个字符串长度
//     let start = iter.nth(n).map(|(idx, _)| idx).unwrap_or(s.len());
//     let end = iter.next().map(|(i, _)| i).unwrap_or(s.len());
//     (&s[..start], &s[start..end], &s[end..])
// }

// fn bytes_to_string_with_dot(bytes: &[u8]) -> String {
//     bytes
//         .iter()
//         .map(|&b| {
//             if b.is_ascii() && !b.is_ascii_control() {
//                 b as char
//             } else {
//                 '.'
//             }
//         })
//         .collect()
// }

// fn format_hex_slice(slice: &[u8], j: &mut usize) -> String {
//     let mut line = String::with_capacity(slice.len() * 3); // Adjust capacity based on expected size
//     for b in slice.iter() {
//         let mut buffer = Buffer::<1>::new();
//         let c = buffer.format(&[*b]);
//         line.push_str(c);
//         line.push_str(if (*j + 1) % 8 == 0 { "  " } else { " " });
//         *j += 1;
//     }
//     line
// }

// type ParserFn = fn(&ByteView) -> String;

// fn format_data_inspector<T: std::fmt::Display>(data: T) -> String {
//     format!("{:<40}|", data)
// }

// static FIELDS: &[(&str, ParserFn)] = &[
//     ("| Binary (8bit)      | ", |bv| {
//         format_data_inspector(bv.to_binary_8bit())
//     }),
//     ("| Binary Len         | ", |bv| {
//         format_data_inspector(bv.len())
//     }),
//     ("| uint8_t            | ", |bv| {
//         format_data_inspector(bv.to_u8())
//     }),
//     ("| uint16_t           | ", |bv| {
//         format_data_inspector(bv.to_u16())
//     }),
//     ("| int16_t            | ", |bv| {
//         format_data_inspector(bv.to_i16())
//     }),
//     ("| uint32_t           | ", |bv| {
//         format_data_inspector(bv.to_u32())
//     }),
//     ("| int32_t            | ", |bv| {
//         format_data_inspector(bv.to_i32())
//     }),
//     ("| uint64_t           | ", |bv| {
//         format_data_inspector(bv.to_u64())
//     }),
//     ("| int64_t            | ", |bv| {
//         format_data_inspector(bv.to_i64())
//     }),
//     ("| half float(f16)    | ", |bv| {
//         format_data_inspector(bv.to_f16())
//     }),
//     ("| float              | ", |bv| {
//         format_data_inspector(bv.to_f32())
//     }),
//     ("| double             | ", |bv| {
//         format_data_inspector(bv.to_f64())
//     }),
//     ("| String             | ", |bv| {
//         format_data_inspector(bv.to_str())
//     }),
//     ("| pgvarint           | ", |bv| {
//         format_data_inspector(bv.to_varlena())
//     }),
// ];

// fn get_data_inspector_content<'a>(seek: usize, buf: Vec<u8>, endian: Endian) -> Text<'a> {
//     let bv = ByteView::new(buf, endian);
//     let mut lines = vec![
//         Line::from(Span::styled(
//             "Data Inspector",
//             Style::default()
//                 .fg(Color::White)
//                 .add_modifier(Modifier::BOLD),
//         )),
//         Line::from(""),
//         Line::from(vec![
//             Span::styled("| address            | ", Style::default().fg(Color::White)),
//             Span::raw(seek.to_string()),
//         ]),
//     ];
//     lines.extend(FIELDS.iter().map(|&(label, f)| {
//         let spans = vec![
//             Span::styled(label, Style::default().fg(Color::White)),
//             Span::raw(f(&bv)),
//         ];
//         Line::from(spans)
//     }));

//     let text = Text::from(lines);
//     text
// }

// const HEX_TOP: &'static str = "00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F     ASCII";

// fn get_hex_content<'a>(
//     txts: &'a RingVec<CacheStr>,
//     line_meta: &'a RingVec<EditLineMeta>,
//     cur_line: usize,
//     hex_sel: &TextSelect,
//     height: usize,
//     cursor_y: usize,
//     cursor_x: usize,
// ) -> (Text<'a>, Text<'a>) {
//     let mut lines = Vec::with_capacity(line_meta.len() + 1);
//     let mut buffer = Buffer::<1>::new();

//     let top = Span::styled(
//         HEX_TOP,
//         Style::default()
//             .fg(Color::White)
//             .add_modifier(Modifier::BOLD),
//     );
//     lines.push(Line::from(top));
//     lines.push(Line::from(""));
//     for (i, txt) in txts.iter().enumerate() {
//         let (slice1, slice2) = txt.as_slice();
//         let mut spans = Vec::with_capacity(slice1.len() + slice2.len());
//         let mut str_spans = Vec::with_capacity(slice1.len() + slice2.len());
//         let mut j = 0;
//         if cursor_y == i {
//             for b in slice1.iter() {
//                 let category = Byte(*b).category();
//                 let color = category.color();
//                 let c = buffer.format(&[*b]);
//                 let space = if j != 0 && (j + 1) % 8 == 0 {
//                     "  "
//                 } else {
//                     " "
//                 };

//                 let b1 = if b.is_ascii() && !b.is_ascii_control() {
//                     (*b as char).to_string()
//                 } else {
//                     '.'.to_string()
//                 };

//                 if hex_sel.has_selected() {
//                     if hex_sel.is_selected(line_meta.get(i).unwrap().get_line_file_start() + j) {
//                         spans.push(Span::styled(
//                             c.to_string().to_uppercase(),
//                             Style::default().fg(color).bg(Color::DarkGray),
//                         ));
//                         spans.push(Span::styled(space, Style::default().bg(Color::DarkGray)));
//                         str_spans.push(Span::styled(b1, Style::default().bg(Color::DarkGray)));
//                     } else {
//                         spans.push(Span::styled(
//                             c.to_string().to_uppercase(),
//                             Style::default().fg(color),
//                         ));
//                         spans.push(Span::raw(space));
//                         str_spans.push(Span::raw(b1));
//                     }
//                 } else {
//                     if j == cursor_x {
//                         spans.push(Span::styled(
//                             c.to_string().to_uppercase(),
//                             Style::default().fg(color).bg(Color::DarkGray),
//                         ));
//                         str_spans.push(Span::styled(b1, Style::default().bg(Color::DarkGray)));
//                     } else {
//                         spans.push(Span::styled(
//                             c.to_string().to_uppercase(),
//                             Style::default().fg(color),
//                         ));
//                         str_spans.push(Span::raw(b1));
//                     }
//                     spans.push(Span::raw(space));
//                 }
//                 j += 1;
//             }

//             for b in slice2.iter() {
//                 let category = Byte(*b).category();
//                 let color = category.color();
//                 let c = buffer.format(&[*b]);
//                 let space = if j != 0 && (j + 1) % 8 == 0 {
//                     "  "
//                 } else {
//                     " "
//                 };

//                 let b1 = if b.is_ascii() && !b.is_ascii_control() {
//                     (*b as char).to_string()
//                 } else {
//                     '.'.to_string()
//                 };

//                 if hex_sel.has_selected() {
//                     if hex_sel.is_selected(line_meta.get(i).unwrap().get_line_file_start() + j) {
//                         spans.push(Span::styled(
//                             c.to_string().to_uppercase(),
//                             Style::default().fg(color).bg(Color::DarkGray),
//                         ));
//                         spans.push(Span::styled(space, Style::default().bg(Color::DarkGray)));
//                         str_spans.push(Span::styled(b1, Style::default().bg(Color::DarkGray)));
//                     } else {
//                         spans.push(Span::styled(
//                             c.to_string().to_uppercase(),
//                             Style::default().fg(color),
//                         ));
//                         spans.push(Span::raw(space));
//                         str_spans.push(Span::raw(b1));
//                     }
//                 } else {
//                     if j == cursor_x {
//                         spans.push(Span::styled(
//                             c.to_string().to_uppercase(),
//                             Style::default().fg(color).bg(Color::DarkGray),
//                         ));
//                         str_spans.push(Span::styled(b1, Style::default().bg(Color::DarkGray)));
//                     } else {
//                         spans.push(Span::styled(
//                             c.to_string().to_uppercase(),
//                             Style::default().fg(color),
//                         ));
//                         str_spans.push(Span::raw(b1));
//                     }
//                     spans.push(Span::raw(space));
//                 }
//                 j += 1;
//             }
//         } else {
//             for b in slice1.iter() {
//                 let category = Byte(*b).category();
//                 let color = category.color();
//                 let c = buffer.format(&[*b]);

//                 let space = if j != 0 && (j + 1) % 8 == 0 {
//                     "  "
//                 } else {
//                     " "
//                 };
//                 let b1 = if b.is_ascii() && !b.is_ascii_control() {
//                     (*b as char).to_string()
//                 } else {
//                     '.'.to_string()
//                 };
//                 if hex_sel.has_selected() {
//                     if hex_sel.is_selected(line_meta.get(i).unwrap().get_line_file_start() + j) {
//                         spans.push(Span::styled(
//                             c.to_string().to_uppercase(),
//                             Style::default().fg(color).bg(Color::DarkGray),
//                         ));
//                         spans.push(Span::styled(space, Style::default().bg(Color::DarkGray)));
//                         str_spans.push(Span::styled(b1, Style::default().bg(Color::DarkGray)));
//                     } else {
//                         spans.push(Span::styled(
//                             c.to_string().to_uppercase(),
//                             Style::default().fg(color),
//                         ));
//                         spans.push(Span::raw(space));
//                         str_spans.push(Span::raw(b1));
//                     }
//                 } else {
//                     spans.push(Span::styled(
//                         c.to_string().to_uppercase(),
//                         Style::default().fg(color),
//                     ));
//                     spans.push(Span::raw(space));
//                     str_spans.push(Span::raw(b1));
//                 }
//                 j += 1;
//             }

//             for b in slice2.iter() {
//                 let category = Byte(*b).category();
//                 let color = category.color();
//                 let c = buffer.format(&[*b]);
//                 let space = if j != 0 && (j + 1) % 8 == 0 {
//                     "  "
//                 } else {
//                     " "
//                 };
//                 let b1 = if b.is_ascii() && !b.is_ascii_control() {
//                     (*b as char).to_string()
//                 } else {
//                     '.'.to_string()
//                 };
//                 if hex_sel.has_selected() {
//                     if hex_sel.is_selected(line_meta.get(i).unwrap().get_line_file_start() + j) {
//                         spans.push(Span::styled(
//                             c.to_string().to_uppercase(),
//                             Style::default().fg(color).bg(Color::DarkGray),
//                         ));
//                         spans.push(Span::styled(space, Style::default().bg(Color::DarkGray)));
//                         str_spans.push(Span::styled(b1, Style::default().bg(Color::DarkGray)));
//                     } else {
//                         spans.push(Span::styled(
//                             c.to_string().to_uppercase(),
//                             Style::default().fg(color),
//                         ));
//                         spans.push(Span::raw(space));
//                         str_spans.push(Span::raw(b1));
//                     }
//                 } else {
//                     spans.push(Span::styled(
//                         c.to_string().to_uppercase(),
//                         Style::default().fg(color),
//                     ));
//                     spans.push(Span::raw(space));
//                     str_spans.push(Span::raw(b1));
//                 }
//                 j += 1;
//             }
//         }

//         spans.push(Span::raw(
//             "   ".repeat(16_usize.saturating_sub(txt.len()) + 1),
//         ));
//         spans.extend_from_slice(&str_spans);
//         lines.push(Line::from(spans));
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
//                 if i == 0 {
//                     return Line::from(Span::raw("Address"));
//                 }
//                 if i == 1 {
//                     return Line::from(Span::raw(""));
//                 }
//                 if i - 2 >= line_meta.len() {
//                     return Line::raw(" ");
//                 }
//                 Line::from(Span::styled(
//                     format!(
//                         "{:07x}",
//                         line_meta.get(i - 2).unwrap().get_line_file_start()
//                     ),
//                     Style::default().fg(Color::White),
//                 ))
//             })
//             .collect::<Vec<Line>>(),
//     );

//     let text = Text::from(lines);
//     (nav_text, text)
// }

// fn get_edit_content<'a>(
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

// fn get_content<'a>(
//     txts: &'a Vec<&str>,
//     line_meta: &'a Vec<EditLineMeta>,
//     cur_line: usize,
//     select_line: &Option<(usize, usize)>,
//     height: usize,
// ) -> (Text<'a>, Text<'a>) {
//     // assert!(content.len() == line_meta.len());
//     let mut lines = Vec::with_capacity(line_meta.len());
//     for (i, txt) in txts.into_iter().enumerate() {
//         let mut spans = Vec::new();
//         let mf = line_meta[i].get_match(); //fuzzy_search(input, text, false);
//         if let Some(m) = mf {
//             match m {
//                 Match::Char(_) => {
//                     todo!()
//                 }
//                 Match::Byte(v) => {
//                     let mut current_idx = 0;
//                     for bm in v.into_iter() {
//                         if current_idx < bm.start && bm.start <= txt.len() {
//                             spans.push(Span::raw(&txt[current_idx..bm.start]));
//                         }
//                         // 添加高亮文本
//                         if bm.start < txt.len() && bm.end <= txt.len() {
//                             spans.push(Span::styled(
//                                 &txt[bm.start..bm.end],
//                                 Style::default().bg(Color::Green),
//                             ));
//                         }
//                         // 更新当前索引为高亮区间的结束位置
//                         current_idx = bm.end;
//                     }
//                     // 添加剩余的文本（如果有）
//                     if current_idx < txt.len() {
//                         spans.push(Span::raw(&txt[current_idx..]));
//                     }
//                 }
//             }
//         }
//         if let Some((st, en)) = select_line {
//             if (line_meta[i].get_line_num() >= *st && line_meta[i].get_line_num() <= *en)
//                 || i == cur_line
//             {
//                 lines.push(Line::from(Span::styled(
//                     *txt,
//                     Style::default().bg(Color::LightRed), // 设置背景颜色为红色
//                 )));
//             } else {
//                 if spans.len() > 0 {
//                     lines.push(Line::from(spans));
//                 } else {
//                     lines.push(Line::from(*txt));
//                 }
//             }
//         } else {
//             if i == cur_line {
//                 lines.push(Line::from(Span::styled(
//                     *txt,
//                     Style::default().bg(Color::LightRed), // 设置背景颜色为蓝色
//                 )));
//             } else {
//                 if spans.len() > 0 {
//                     lines.push(Line::from(spans));
//                 } else {
//                     lines.push(Line::from(*txt));
//                 }
//             }
//         }
//     }

//     let nav_text = Text::from(
//         (0..height)
//             .enumerate()
//             .map(|(i, _)| {
//                 if i == cur_line {
//                     Line::from(Span::styled(">", Style::default().fg(Color::LightRed)))
//                 // 高亮当前行
//                 } else {
//                     Line::from(" ") // 非当前行为空白
//                 }
//             })
//             .collect::<Vec<Line>>(),
//     );

//     let text = Text::from(lines);
//     (nav_text, text)
// }

// 获取要显示的内容（根据终端高度和偏移量）

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_n_chars() -> io::Result<()> {
        let s = "Helloworld!";
        let (a, b, c) = n_chars(s, 5);
        println!("a:{},b:{},c:{}", a, b, c);
        Ok(())
    }
}
