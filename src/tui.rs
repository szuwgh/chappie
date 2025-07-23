use crate::byteutil::ByteView;
use crate::byteutil::Endian;
use crate::cli::UIType;
use crate::editor::CacheStr;
use crate::editor::EditLineMeta;
use crate::editor::EditTextWarp;
use crate::editor::GapText;
use crate::editor::HexText;
use crate::editor::MmapText;
use crate::editor::RingVec;
use crate::editor::TextDisplay;
use crate::editor::TextOper;
use crate::editor::TextWarp;
use crate::editor::TextWarpType;
use crate::error::ChapResult;
use crate::fuzzy::Match;
use crate::handle::Handle;
use crate::handle::HandleEdit;
use crate::handle::HandleHex;
use crate::handle::HandleImpl;
use crate::textwarp::LineMeta;
use crate::textwarp::SimpleText;
use crate::textwarp::SimpleTextEngine;
use const_hex::Buffer;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::execute;
use crossterm::terminal::enable_raw_mode;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::{
    cursor::{self, MoveTo},
    event::{self, KeyCode},
    ExecutableCommand,
};
use ratatui::prelude::Backend;
use ratatui::prelude::Constraint;
use ratatui::prelude::CrosstermBackend;
use ratatui::prelude::Direction;
use ratatui::prelude::Layout;
use ratatui::prelude::Position;
use ratatui::prelude::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::symbols::line;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use std::fmt::format;
use std::io;
use std::mem;
use std::path::Path;
use std::process::exit;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;
use unicode_width::UnicodeWidthStr;
//use vectorbase::collection::Collection;

pub(crate) enum ChapMod {
    Edit,   //普通编辑器模式
    Hex,    //16进制编辑器模式
    Text,   //大文本浏览模式
    Vector, //向量分析模式
}

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

pub(crate) struct ChapTui {
    chap_mod: ChapMod,
    pub(crate) warp_type: TextWarpType,
    pub(crate) terminal: Terminal<CrosstermBackend<io::Stdout>>,
    pub(crate) navi: Navigation,
    pub(crate) tv: TextView,
    pub(crate) cmd_title: Rect,
    pub(crate) cmd_inp: CmdInput,
    pub(crate) assist_tv: TextView,
    pub(crate) assist_inp: ChatInput,
    focus: Focus,
    prompt_tx: mpsc::Sender<String>,
    start_row: u16,
    llm_res_rx: mpsc::Receiver<String>,
    ui_type: UIType,
    que: bool,

    pub(crate) back_linenum: Vec<usize>, // 上一行号

    pub(crate) txt_sel: TextSelect,   // 文本选择
    pub(crate) cursor_x: usize,       // 光标x坐标
    pub(crate) cursor_y: usize,       // 光标y坐标
    pub(crate) offset: usize,         // 偏移量
    pub(crate) start_line_num: usize, // 起始行号
    pub(crate) is_last_line: bool,    // 是否是最后一行
    pub(crate) endian: Endian,        // 字节序
}

// 文本编辑器大文件浏览 窗口
pub(crate) struct TextWindow {
    navi: Navigation, //导航
    tv: TextView,
    cmd_inp: CmdInput,
}

// 16进制编辑窗口
pub(crate) struct HexWindow {
    tv: TextView,
    cmd_inp: CmdInput,
}

pub(crate) struct AiChatWindow {}

pub(crate) struct TerminalWindow {}

enum FocusType {
    TxtFuzzy,
    Chat,
}

//焦点
struct Focus {
    current_focus: usize,
}

impl Focus {
    // 创建一个新的 Focus 实例
    fn new() -> Self {
        Focus { current_focus: 0 }
    }

    // 切换焦点
    fn next(&mut self) {
        self.current_focus = (self.current_focus + 1) % mem::variant_count::<FocusType>();
        // 0到3循环
    }

    fn get_colors(&self) -> (Color, Color, Color, Color) {
        let base = Color::White;
        let highlight = Color::Yellow;
        match self.current() {
            FocusType::TxtFuzzy => (highlight, highlight, base, base),
            FocusType::Chat => (base, base, highlight, highlight),
        }
    }

    // 获取当前焦点
    fn current(&self) -> FocusType {
        match self.current_focus {
            0 => FocusType::TxtFuzzy,
            1 => FocusType::Chat,
            _ => {
                todo!()
            }
        }
    }
}

struct Prompt {
    prompt: String,
    _id: String,
}

impl Prompt {
    fn prompt(&self) -> &str {
        &self.prompt
    }
    fn _id(&self) -> &str {
        &self._id
    }
}

#[derive(Debug)]
struct ChatItemIndex(usize, usize);

impl ChatItemIndex {
    fn start(&self) -> usize {
        self.0
    }
    fn end(&self) -> usize {
        self.1
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TextSelect(usize, usize);

impl TextSelect {
    fn new() -> Self {
        TextSelect(0, 0)
    }

    pub(crate) fn from_select(start: usize, end: usize) -> Self {
        TextSelect(start, end)
    }

    fn start(&self) -> usize {
        self.0
    }
    fn end(&self) -> usize {
        self.1
    }

    fn len(&self) -> usize {
        self.end() - self.start()
    }

    fn inc_end(&mut self) {
        self.1 += 1;
    }

    fn has_selected(&self) -> bool {
        self.start() < self.end()
    }

    fn is_selected(&self, pos: usize) -> bool {
        pos >= self.start() && pos <= self.end()
    }

    // 递减end
    fn dec_end(&mut self) {
        if self.1 > self.0 {
            self.1 -= 1;
        }
    }

    pub(crate) fn reset_to_start(&mut self) {
        self.1 = self.0;
    }

    pub(crate) fn set_pos(&mut self, pos: usize) {
        self.0 = pos;
        self.1 = pos;
    }

    pub(crate) fn get_start(&self) -> usize {
        self.0
    }

    pub(crate) fn get_end(&self) -> usize {
        self.1
    }

    pub(crate) fn set_start(&mut self, start: usize) {
        self.0 = start;
    }

    pub(crate) fn set_end(&mut self, end: usize) {
        self.1 = end;
    }

    pub(crate) fn set_select(&mut self, start: usize, end: usize) {
        self.0 = start;
        self.1 = end;
    }
}

#[derive(Default)]
// 聊天框的类型
enum ChatType {
    #[default]
    ChatTv,
    Promt,
    Pattern,
}

impl ChapTui {
    pub(crate) fn new(
        chap_mod: ChapMod,
        prompt_tx: mpsc::Sender<String>,
        // vdb: Option<Collection>,
        llm_res_rx: mpsc::Receiver<String>,
        ui_type: UIType,
        que: bool,
    ) -> ChapResult<ChapTui> {
        enable_raw_mode()?;
        io::stdout().execute(cursor::Show)?;
        let (_, row) = cursor::position()?; // (x, y) 返回的是光标的 (列号, 行号)
        let backend = CrosstermBackend::new(std::io::stdout());
        let mut terminal: Terminal<CrosstermBackend<io::Stdout>> = Terminal::new(backend)?;
        let size = terminal.size()?;
        let (tui_height, tui_width, start_row) = match ui_type {
            UIType::Full => {
                execute!(
                    terminal.backend_mut(),
                    crossterm::terminal::EnterAlternateScreen
                )?;
                (size.height, size.width, 0)
            }
            UIType::Lite => {
                let tui_height = (size.height as f32 * 0.4) as u16;
                let tui_width = size.width;
                let mut start_row = row;
                // 终端宽度
                if size.height - row < tui_height {
                    for _ in 0..tui_height.saturating_sub(size.height - row) {
                        println!(); // 打印空白
                        start_row -= 1;
                    }
                }
                (tui_height, tui_width, start_row)
            }
        };

        let nav_with = match chap_mod {
            ChapMod::Edit => 5,
            ChapMod::Hex => 8,
            ChapMod::Text => 5,
            ChapMod::Vector => 5,
        };

        // 文本框显示内容的高度
        let tv_heigth = (tui_height - 1) as usize;
        // 文本框显示内容的宽度
        let tv_width = (tui_width as f32 * 0.5) as usize - 3;

        let assist_tv_width = (tui_width as f32 * 0.5) as usize; //(tui_width as f32 * 0.0) as usize - 3;

        let max_line = (tui_height - 3) as usize;

        let rect = Rect::new(0, start_row, tui_width, tui_height);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)].as_ref())
            .split(rect);

        let (nav_chk, tv_chk, inp_title_chk, seach_chk, assist_tv_chk, assist_inp_chk) = {
            //文本框和输入框
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(100), Constraint::Length(1)].as_ref())
                .split(chunks[0]); // chunks[1] 是左侧区域

            //LLM聊天和输入框
            let right_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(2)].as_ref())
                .split(chunks[1]); // chunks[1] 是左侧区域

            //导航栏和文本框
            let nav_text_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(nav_with), Constraint::Percentage(100)].as_ref())
                .split(left_chunks[0]); // chunks[1] 是左侧区域

            let search_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(4), Constraint::Percentage(100)].as_ref())
                .split(left_chunks[1]); // chunks[1] 是左侧区域
            (
                nav_text_chunks[0],
                nav_text_chunks[1],
                search_chunks[0],
                search_chunks[1],
                right_chunks[0],
                right_chunks[1],
            )
        };

        let navi = Navigation {
            max_line: max_line,
            min_line: 0,
            cur_line: 0,
            rect: nav_chk,
            select_line: None,
        };

        let tv = TextView {
            height: tv_heigth,
            width: tv_width,
            scroll: 1,
            rect: tv_chk,
        };

        let cmd_inp = CmdInput::new(seach_chk);

        let assist_tv = TextView {
            height: tv_heigth,
            width: assist_tv_width,
            scroll: 1,
            rect: assist_tv_chk,
        };

        let assist_inp = ChatInput {
            input: String::new(),
            rect: assist_inp_chk,
        };

        Ok(ChapTui {
            chap_mod: chap_mod,
            warp_type: TextWarpType::NoWrap,
            terminal: terminal,
            navi: navi,
            tv: tv,
            cmd_title: inp_title_chk,
            cmd_inp: cmd_inp,
            assist_tv: assist_tv,
            assist_inp: assist_inp,
            focus: Focus::new(),
            prompt_tx: prompt_tx,
            start_row: start_row,
            llm_res_rx: llm_res_rx,
            ui_type: ui_type,
            que: que,
            back_linenum: Vec::with_capacity(10), // 初始化上一行号
            txt_sel: TextSelect::new(),
            cursor_x: 0,
            cursor_y: 0,
            offset: 0,
            start_line_num: 0,
            is_last_line: false,
            endian: Endian::Big, // 默认字节序为小端
        })
    }

    pub(crate) fn set_endian(&mut self, endian: Endian) {
        self.endian = endian;
    }

    fn render_hex<'a>(
        &mut self,
        cursor_x: usize,
        cursor_y: usize,
        hex_sel: TextSelect,
        td: &'a TextDisplay,
    ) -> ChapResult<&'a RingVec<EditLineMeta>> {
        let line_meta = {
            let (content, meta) = td.get_current_page()?;
            self.terminal.draw(|f| {
                let (navi, visible_content) = get_hex_content(
                    content,
                    &meta,
                    self.navi.get_cur_line(),
                    &hex_sel,
                    self.tv.get_height(),
                    cursor_y,
                    cursor_x,
                );
                let text_para = Paragraph::new(visible_content)
                    .block(Block::default())
                    .style(Style::default().fg(Color::White));
                f.render_widget(text_para, self.tv.get_rect());

                let nav_paragraph = Paragraph::new(navi);
                f.render_widget(nav_paragraph, self.navi.get_rect());

                let sel_content = td.get_text_from_sel(&hex_sel);
                let assist = get_data_inspector_content(
                    hex_sel.get_start(),
                    sel_content,
                    self.endian.clone(),
                );
                let assist_para = Paragraph::new(assist)
                    .block(Block::default())
                    .style(Style::default().fg(Color::White));
                f.render_widget(assist_para, self.assist_tv.get_rect());

                let input_title_box = Paragraph::new(Text::raw(" >:"))
                    .block(Block::default())
                    .style(Style::default().fg(Color::White)); // 设置输入框样式
                f.render_widget(input_title_box, self.cmd_title);

                let input_box = Paragraph::new(Text::raw(self.cmd_inp.get_inp()))
                    .block(Block::default())
                    .style(Style::default().fg(Color::White)); // 设置输入框样式
                f.render_widget(input_box, self.cmd_inp.get_rect());
            })?;

            meta
        };
        Ok(line_meta)
    }

    pub(crate) fn handle_ctrl_c(&mut self) -> ChapResult<()> {
        crossterm::terminal::disable_raw_mode()?;
        execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen // 离开备用屏幕
        )?;
        io::stdout().execute(cursor::Show)?;
        exit(0);
    }

    pub(crate) fn handle_ctrl_s<P: AsRef<Path>>(
        &mut self,
        p: P,
        td: &mut TextDisplay,
    ) -> ChapResult<()> {
        match self.chap_mod {
            ChapMod::Edit => {
                self.cmd_inp.clear();
                //保存
                if let Ok(_) = td.save(&p) {
                    self.cmd_inp.push_str("saved");
                } else {
                    self.cmd_inp.push_str("save fail");
                }
            }
            ChapMod::Text => {
                todo!()
            }
            ChapMod::Hex => {
                todo!()
                // is_last = false;
            }
            _ => {}
        };
        Ok(())
    }

    pub(crate) fn handle_char(
        &mut self,
        c: char,
        cursor_x: &mut usize,
        cursor_y: &mut usize,
        offset: &mut usize,
        is_last: &mut bool,
        start_line_num: usize,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        match self.chap_mod {
            ChapMod::Edit => {
                self.cmd_inp.clear();
                if *cursor_x == 0 && *is_last {
                    td.insert(
                        *cursor_y - 1,
                        self.tv.get_width(),
                        line_meta.get(*cursor_y - 1).unwrap(),
                        c,
                    )?;
                    *is_last = false;
                } else {
                    td.insert(*cursor_y, *cursor_x, line_meta.get(*cursor_y).unwrap(), c)?;
                }
                if *cursor_x < self.tv.get_width() {
                    *cursor_x += 1;
                    if *cursor_x >= self.tv.get_width() && *cursor_y < self.tv.get_height() {
                        //不断添加字符 还是续接上一行
                        *is_last = true;
                        *cursor_x = 0;
                        *cursor_y += 1;
                    }
                }
                td.get_one_page(start_line_num)?;
            }
            ChapMod::Text => {
                todo!()
            }
            ChapMod::Hex => {
                todo!()
                // is_last = false;
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_backspace(
        &mut self,
        cursor_x: &mut usize,
        cursor_y: &mut usize,
        start_line_num: usize,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        match self.chap_mod {
            ChapMod::Edit => {
                self.cmd_inp.clear();
                if *cursor_y == 0 && *cursor_x == 0 {
                    return Ok(());
                }
                td.backspace(*cursor_y, *cursor_x, line_meta.get(*cursor_y).unwrap())?;
                if *cursor_x == 0 {
                    *cursor_x = line_meta.get(*cursor_y - 1).unwrap().get_txt_len();
                    *cursor_y = cursor_y.saturating_sub(1);
                } else {
                    *cursor_x = cursor_x.saturating_sub(1);
                }
                td.get_one_page(start_line_num)?;
            }
            ChapMod::Text => {
                todo!()
            }
            ChapMod::Hex => {
                todo!()
                // is_last = false;
            }
            _ => {}
        };
        Ok(())
    }

    pub(crate) fn handle_enter<'a>(
        &mut self,
        cursor_x: &mut usize,
        cursor_y: &mut usize,
        start_line_num: usize,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match self.chap_mod {
            ChapMod::Edit => {
                self.cmd_inp.clear();
                td.insert_newline(*cursor_y, *cursor_x, line_meta.get(*cursor_y).unwrap())?;
                if *cursor_y < self.tv.get_height() - 1 {
                    *cursor_y += 1;
                }
                *cursor_x = 0;
                td.get_one_page(start_line_num)?;
            }
            ChapMod::Text => {
                todo!()
            }
            ChapMod::Hex => {
                todo!()
                // is_last = false;
            }
            _ => {}
        };
        Ok(())
    }

    pub(crate) fn handle_up<'a>(
        &self,
        cursor_x: &mut usize,
        cursor_y: &mut usize,
        offset: &mut usize,
        is_last: &mut bool,
        mut line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match self.chap_mod {
            ChapMod::Edit => {
                match self.warp_type {
                    TextWarpType::NoWrap => {
                        if *cursor_y == 0 {
                            //滚动上一行
                            td.scroll_pre_one_line(line_meta.get(0).unwrap())?;
                            line_meta = td.get_current_line_meta()?;
                        }
                        *cursor_y = cursor_y.saturating_sub(1);
                        if *cursor_x >= line_meta.get(*cursor_y).unwrap().get_char_len() {
                            *cursor_x = line_meta.get(*cursor_y).unwrap().get_char_len();
                        }
                        let meta = line_meta.get(*cursor_y).unwrap();
                        if *offset >= meta.get_char_len() {
                            *offset = meta.get_char_len();
                        }
                        *is_last = false;
                    }
                    TextWarpType::SoftWrap => {
                        if *cursor_y == 0 {
                            //滚动上一行
                            td.scroll_pre_one_line(line_meta.get(0).unwrap())?;
                            line_meta = td.get_current_line_meta()?;
                        }
                        *cursor_y = cursor_y.saturating_sub(1);
                        if *cursor_x >= line_meta.get(*cursor_y).unwrap().get_char_len() {
                            *cursor_x = line_meta.get(*cursor_y).unwrap().get_char_len();
                        }

                        *is_last = false;
                    }
                }
            }
            ChapMod::Text => {
                todo!()
            }
            ChapMod::Hex => {
                if *cursor_y == 0 {
                    //滚动上一行
                    td.scroll_pre_one_line(line_meta.get(0).unwrap())?;
                    line_meta = td.get_current_line_meta()?;
                }
                *cursor_y = cursor_y.saturating_sub(1);
                if *cursor_x >= line_meta.get(*cursor_y).unwrap().get_hex_len() {
                    *cursor_x = line_meta.get(*cursor_y).unwrap().get_hex_len();
                }
            }
            _ => {}
        };
        return Ok(());
    }

    pub(crate) fn handle_down<'a>(
        &self,
        cursor_x: &mut usize,
        cursor_y: &mut usize,
        offset: &mut usize,
        is_last: &mut bool,
        mut line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match self.chap_mod {
            ChapMod::Edit => {
                match self.warp_type {
                    TextWarpType::NoWrap => {
                        if *cursor_y < self.tv.get_height() - 1 {
                            *cursor_y += 1;
                        } else {
                            //滚动下一行
                            td.scroll_next_one_line(line_meta.last().unwrap())?;
                            line_meta = td.get_current_line_meta()?;
                        }
                        if *cursor_x >= line_meta.get(*cursor_y).unwrap().get_char_len() {
                            *cursor_x = line_meta.get(*cursor_y).unwrap().get_char_len();
                        }
                        let meta = line_meta.get(*cursor_y).unwrap();
                        if *offset >= meta.get_char_len() {
                            *offset = meta.get_char_len();
                        }
                        *is_last = false;
                    }
                    TextWarpType::SoftWrap => {
                        if *cursor_y < self.tv.get_height() - 1 {
                            *cursor_y += 1;
                        } else {
                            //滚动下一行
                            td.scroll_next_one_line(line_meta.last().unwrap())?;
                            line_meta = td.get_current_line_meta()?;
                        }
                        if *cursor_x >= line_meta.get(*cursor_y).unwrap().get_char_len() {
                            *cursor_x = line_meta.get(*cursor_y).unwrap().get_char_len();
                        }
                        *is_last = false;
                    }
                }
            }
            ChapMod::Text => {
                todo!()
            }
            ChapMod::Hex => {
                if *cursor_y < line_meta.len() - 1 {
                    *cursor_y += 1;
                } else {
                    //滚动下一行
                    td.scroll_next_one_line(line_meta.last().unwrap())?;
                    line_meta = td.get_current_line_meta()?;
                }
                if *cursor_x >= line_meta.get(*cursor_y).unwrap().get_hex_len() {
                    *cursor_x = line_meta.get(*cursor_y).unwrap().get_hex_len();
                };
            }
            _ => {}
        };
        Ok(())
    }

    pub(crate) fn handle_left(
        &self,
        cursor_x: &mut usize,
        cursor_y: &mut usize,
        offset: &mut usize,
        is_last: &mut bool,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        match self.chap_mod {
            ChapMod::Edit => {
                match self.warp_type {
                    TextWarpType::NoWrap => {
                        *cursor_x = cursor_x.saturating_sub(1);
                        *offset = offset.saturating_sub(1);
                    }
                    TextWarpType::SoftWrap => {
                        if *cursor_x == 0 {
                            // 这个判断说明当前行已经读完了
                            if line_meta.get(*cursor_y).unwrap().get_line_offset() == 0 {
                                //无需操作
                            } else {
                                *cursor_x =
                                    line_meta.get(*cursor_y - 1).unwrap().get_char_len() - 1;
                                *cursor_y = cursor_y.saturating_sub(1);
                            }
                        } else {
                            *cursor_x = cursor_x.saturating_sub(1);
                        }
                        *is_last = false;
                    }
                }
            }
            ChapMod::Text => {
                todo!()
            }
            ChapMod::Hex => {
                if *cursor_x == 0 {
                    // 这个判断说明当前行已经读完了
                    if line_meta.get(*cursor_y).unwrap().get_line_offset() == 0 {
                        //无需操作
                    } else {
                        *cursor_x = line_meta.get(*cursor_y - 1).unwrap().get_char_len() - 1;
                        *cursor_y = cursor_y.saturating_sub(1);
                    }
                } else {
                    *cursor_x = cursor_x.saturating_sub(1);
                }
                // is_last = false;
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_right_shift(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        match self.chap_mod {
            ChapMod::Edit => {
                todo!()
            }
            ChapMod::Text => {
                todo!()
            }
            ChapMod::Hex => {
                if chap_tui.cursor_x < line_meta.get(chap_tui.cursor_y).unwrap().get_hex_len() {
                    chap_tui.cursor_x += 1;
                }
                chap_tui.txt_sel.set_end(
                    line_meta
                        .get(chap_tui.cursor_y)
                        .unwrap()
                        .get_line_file_start()
                        + chap_tui.cursor_x,
                );
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_left_shift(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        match self.chap_mod {
            ChapMod::Edit => {
                todo!()
            }
            ChapMod::Text => {
                todo!()
            }
            ChapMod::Hex => {
                if chap_tui.cursor_x < line_meta.get(chap_tui.cursor_y).unwrap().get_hex_len() {
                    chap_tui.cursor_x += 1;
                }
                chap_tui.txt_sel.set_end(
                    line_meta
                        .get(chap_tui.cursor_y)
                        .unwrap()
                        .get_line_file_start()
                        + chap_tui.cursor_x,
                );
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_right(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        match self.chap_mod {
            ChapMod::Edit => {
                match self.warp_type {
                    TextWarpType::NoWrap => {
                        let meta = line_meta.get(chap_tui.cursor_y).unwrap();
                        if chap_tui.cursor_x < meta.get_char_len()
                            && chap_tui.cursor_x < self.tv.width
                        {
                            chap_tui.cursor_x += 1;
                        }
                        if chap_tui.offset <= meta.get_char_len() {
                            chap_tui.offset += 1;
                        }
                    }
                    TextWarpType::SoftWrap => {
                        if chap_tui.cursor_x
                            < line_meta.get(chap_tui.cursor_y).unwrap().get_char_len()
                        {
                            chap_tui.cursor_x += 1;

                            if chap_tui.cursor_x
                                >= line_meta.get(chap_tui.cursor_y).unwrap().get_char_len()
                                && chap_tui.cursor_y < self.tv.get_height()
                            {
                                //判断当前行是否读完
                                if line_meta.get(chap_tui.cursor_y).unwrap().get_line_end()
                                    < td.get_text_len_from_index(
                                        line_meta.get(chap_tui.cursor_y).unwrap().get_line_index(),
                                    )
                                {
                                    chap_tui.cursor_x = 0;
                                    chap_tui.cursor_y += 1;
                                }
                            }
                        }
                        chap_tui.is_last_line = false;
                    }
                }
            }
            ChapMod::Text => {
                todo!()
            }
            ChapMod::Hex => {
                if chap_tui.cursor_x < line_meta.get(chap_tui.cursor_y).unwrap().get_txt_len() {
                    chap_tui.cursor_x += 1;
                }
                chap_tui.txt_sel.set_pos(
                    line_meta
                        .get(chap_tui.cursor_y)
                        .unwrap()
                        .get_line_file_start()
                        + chap_tui.cursor_x,
                );
            }
            _ => {}
        };
        Ok(())
    }

    pub(crate) async fn render<P: AsRef<Path>>(&mut self, p: P) -> ChapResult<()> {
        loop {
            let twy = TextWarpType::NoWrap;
            let mut td: TextDisplay = match self.chap_mod {
                ChapMod::Edit => TextDisplay::Edit(EditTextWarp::new(
                    GapText::from_file_path(&p)?,
                    self.tv.get_height(),
                    self.tv.get_width(),
                    twy,
                )),
                ChapMod::Text => TextDisplay::Text(TextWarp::new(
                    MmapText::from_file_path(&p)?,
                    self.tv.get_height(),
                    self.tv.get_width(),
                    twy,
                )),
                ChapMod::Hex => TextDisplay::Hex(TextWarp::new(
                    HexText::from_file_path(&p)?,
                    self.tv.get_height() - 2,
                    self.tv.get_width(),
                    twy,
                )),
                _ => {
                    todo!()
                }
            };
            log::info!("TextDisplay height: {:?}", self.tv.get_height() - 2);
            let hand = match self.chap_mod {
                ChapMod::Edit => HandleImpl::Edit(HandleEdit::new()),
                ChapMod::Text => todo!(),
                ChapMod::Hex => HandleImpl::Hex(HandleHex::new()),
                _ => {
                    todo!()
                }
            };
            // let mut txt_sel = TextSelect::new();
            // let mut cursor_x: usize = 0;
            // let mut cursor_y: usize = 0;
            // let mut offset: usize = 0;
            // let mut is_last: bool = false; //是否在行的末尾添加 否则在所在行的头添加
            // let mut start_line_num = 1;
            td.get_one_page(1)?;

            'tui: loop {
                let line_meta = match self.chap_mod {
                    ChapMod::Edit => {
                        self.render_edit(self.cursor_x, self.cursor_y, self.offset, &td)?
                    }
                    ChapMod::Text => {
                        todo!()
                    }
                    ChapMod::Hex => {
                        self.render_hex(self.cursor_x, self.cursor_y, self.txt_sel.clone(), &td)?
                    }
                    _ => {
                        todo!()
                    }
                };
                if let Some(start_line_meta) = line_meta.get(0) {
                    self.start_line_num = start_line_meta.get_line_num();
                }
                'key: loop {
                    if let event::Event::Key(KeyEvent {
                        code, modifiers, ..
                    }) = event::read()?
                    {
                        match (code, modifiers) {
                            (KeyCode::Esc, _) => {
                                hand.handle_esc(self)?;
                            }
                            (KeyCode::Right, KeyModifiers::SHIFT) => {
                                hand.handle_shift_right(self, &line_meta, &td)?;
                            }
                            (KeyCode::Left, KeyModifiers::SHIFT) => {
                                hand.handle_shift_left(self, &line_meta, &td)?;
                            }
                            (KeyCode::Up, _) => {
                                hand.handle_up(self, &line_meta, &td)?;
                            }
                            (KeyCode::Down, _) => {
                                hand.handle_down(self, &line_meta, &td)?;
                            }
                            (KeyCode::Left, _) => {
                                hand.handle_left(self, &line_meta, &td)?;
                            }
                            (KeyCode::Right, _) => {
                                hand.handle_right(self, &line_meta, &td)?;
                            }
                            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                                hand.handle_ctrl_c(self)?
                            }
                            (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                                hand.handle_ctrl_s(self, &p, &mut td)?;
                            }
                            (KeyCode::Enter, _) => hand.handle_enter(self, line_meta, &td)?,
                            (KeyCode::Backspace, _) => {
                                hand.handle_backspace(self, line_meta, &td)?
                            }

                            (KeyCode::Char(c), _) => hand.handle_char(self, line_meta, &td, c)?,
                            _ => {}
                        }
                    }
                    break 'key;
                }
            }
        }
    }

    pub(crate) fn render_edit<'a>(
        &mut self,
        cursor_x: usize,
        cursor_y: usize,
        offset: usize,
        td: &'a TextDisplay,
    ) -> ChapResult<&'a RingVec<EditLineMeta>> {
        let line_meta = {
            let (content, meta) = td.get_current_page()?;
            self.terminal.draw(|f| {
                let (navi, visible_content) = get_edit_content(
                    content,
                    &meta,
                    self.navi.get_cur_line(),
                    &self.navi.select_line,
                    self.tv.get_height(),
                    offset.saturating_sub(self.tv.width),
                    cursor_y,
                    cursor_x,
                );
                let text_para = Paragraph::new(visible_content)
                    .block(Block::default())
                    .style(Style::default().fg(Color::White));
                f.render_widget(text_para, self.tv.get_rect());

                let nav_paragraph = Paragraph::new(navi);
                f.render_widget(nav_paragraph, self.navi.get_rect());

                let input_box = Paragraph::new(Text::raw(self.cmd_inp.get_inp()))
                    .block(Block::default().title(":"))
                    .style(Style::default().fg(Color::White)); // 设置输入框样式
                f.render_widget(input_box, self.cmd_inp.get_rect());
            })?;
            meta
        };
        return Ok(line_meta);
    }

    pub(crate) async fn render_text<T: SimpleText>(&mut self, bytes: T) -> ChapResult<()> {
        let mut eg = SimpleTextEngine::new(bytes, self.tv.get_height(), self.tv.get_width());
        let mut chat_eg = SimpleTextEngine::new(
            String::with_capacity(1024),
            self.assist_tv.get_height(),
            self.assist_tv.get_width(),
        );
        let mut chat_index: usize = 0;
        let mut chat_item: Vec<ChatItemIndex> = Vec::new();
        let chat_type = ChatType::default();
        loop {
            let line_meta = {
                let (inp, is_exact) = self.cmd_inp.get_inp_exact();
                let (txt, line_meta) = eg.get_line(self.tv.get_scroll(), inp, is_exact);
                self.terminal.draw(|f| {
                    let (txt_clr, inp_clr, chat_clr_, assist_inp_clr) = self.focus.get_colors();
                    // 左下输入框区
                    let input_box = Paragraph::new(Text::raw(self.cmd_inp.get_inp()))
                        .block(
                            Block::default()
                                .title("search")
                                .borders(Borders::TOP | Borders::LEFT),
                        )
                        .style(Style::default().fg(inp_clr)); // 设置输入框样式
                    f.render_widget(input_box, self.cmd_inp.get_rect());
                    let block = Block::default().borders(Borders::LEFT);
                    if let Some(c) = &txt {
                        let (navi, visible_content) = get_content(
                            c,
                            &line_meta,
                            self.navi.get_cur_line(),
                            &self.navi.select_line,
                            self.tv.get_height(),
                        );
                        let text_para = Paragraph::new(visible_content)
                            .block(block)
                            .style(Style::default().fg(txt_clr));
                        f.render_widget(text_para, self.tv.get_rect());
                        let nav_paragraph = Paragraph::new(navi);
                        f.render_widget(nav_paragraph, self.navi.get_rect());
                    } else {
                        let text_para = Paragraph::new("")
                            .block(block)
                            .style(Style::default().fg(txt_clr));
                        f.render_widget(text_para, self.tv.get_rect());
                        let nav_paragraph = Paragraph::new("");
                        f.render_widget(nav_paragraph, self.navi.get_rect());
                    }

                    match chat_type {
                        ChatType::ChatTv => {
                            let (chat_line_meta, meta) =
                                chat_eg.get_line(self.assist_tv.get_scroll(), "", is_exact);
                            if let Some(c) = &chat_line_meta {
                                let chat_content =
                                    get_chat_content(c, &meta, &chat_item[chat_index]);
                                let assist_tv = Paragraph::new(chat_content)
                                    .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
                                    .style(Style::default().fg(chat_clr_));
                                f.render_widget(assist_tv, self.assist_tv.get_rect());
                            } else {
                                let assist_tv = Paragraph::new("")
                                    .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
                                    .style(Style::default().fg(chat_clr_));
                                f.render_widget(assist_tv, self.assist_tv.get_rect());
                            }
                        }
                        ChatType::Promt => {}
                        ChatType::Pattern => {}
                    }
                    // 右侧部分可以显示空白或其他内容
                    // let block = Block::default().borders(Borders::ALL).title("LLM Chat");
                    let input_box = Paragraph::new(Text::raw(self.assist_inp.get_inp()))
                        .block(
                            Block::default()
                                .title("prompt")
                                .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT),
                        )
                        .style(Style::default().fg(assist_inp_clr)); // 设置输入框样式
                    f.render_widget(input_box, self.assist_inp.get_rect());

                    match self.focus.current() {
                        FocusType::TxtFuzzy => {
                            // 将光标移动到输入框中合适的位置
                            let inp_len = self.cmd_inp.get_inp().width();
                            let x = if inp_len == 0 {
                                self.cmd_inp.get_rect().x + 2
                            } else {
                                self.cmd_inp.get_rect().x + inp_len as u16 + 1
                            };

                            let y = self.cmd_inp.get_rect().y + 1; // 输入框的 Y 起点
                            f.set_cursor_position(Position { x, y });
                        }
                        FocusType::Chat => {
                            // 将光标移动到输入框中合适的位置
                            let inp_len = self.assist_inp.get_inp().width();
                            let x = if inp_len == 0 {
                                self.assist_inp.get_rect().x + 2
                            } else {
                                self.assist_inp.get_rect().x + inp_len as u16 + 1
                            };
                            let y = self.assist_inp.get_rect().y + 1; // 输入框的 Y 起点
                            f.set_cursor_position(Position { x, y });
                        }
                        _ => {}
                    };
                })?;
                line_meta
            };

            loop {
                tokio::select! {
                    Some(msg) = self.llm_res_rx.recv() => {
                        let chat_item_start = chat_eg.get_line_count().max(1);
                        chat_eg.push_str(&msg);
                        chat_eg.push_str("\n");
                        let chat_item_end = chat_eg.get_line_count().max(1);
                        //debug!("chat_item:{:?}",(chat_item_start, chat_item_end-1));
                        chat_item
                            .push(ChatItemIndex(chat_item_start, chat_item_end-1));
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_millis(25)) => {
                    }
                }
                // 监听键盘输入
                if event::poll(Duration::from_millis(50)).unwrap() {
                    if let event::Event::Key(KeyEvent {
                        code, modifiers, ..
                    }) = event::read()?
                    {
                        match (code, modifiers) {
                            (KeyCode::Esc, _) => {
                                self.cmd_inp.clear();
                                self.assist_inp.clear();
                                self.navi.select_line = None;

                                break;
                            }
                            (KeyCode::Tab, _) => {
                                // 按下 Tab 键，切换焦点
                                self.focus.next();
                                break;
                            }
                            (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                                let assist_inp = self.assist_inp.get_inp();
                                if assist_inp.len() == 0 {
                                    break;
                                }
                                let mut message = String::new();
                                if let Some((start_line, end_line)) = self.navi.select_line {
                                    if let (Some(line), _) = eg.get_start_end(start_line, end_line)
                                    {
                                        for l in line.iter() {
                                            message.push_str(l);
                                        }
                                    }
                                    message.push_str("\n");
                                }
                                message.push_str(assist_inp);
                                break;
                            }
                            (KeyCode::Char('x'), KeyModifiers::CONTROL) => {
                                crossterm::terminal::disable_raw_mode()?;
                                match self.ui_type {
                                    UIType::Full => {
                                        self.terminal.clear()?;
                                        execute!(
                                            self.terminal.backend_mut(),
                                            LeaveAlternateScreen // 离开备用屏幕
                                        )?;
                                    }
                                    UIType::Lite => {
                                        self.terminal.show_cursor()?; // 确保光标可见
                                        self.terminal
                                            .backend_mut()
                                            .execute(MoveTo(0, self.start_row))?; // 假设从当前光标位置下移2行开始清除
                                        self.terminal.backend_mut().clear_region(
                                            ratatui::backend::ClearType::AfterCursor,
                                        )?; // 清除光标下方的区域
                                    }
                                }
                                if chat_item.len() > 0 {
                                    // 在下一行打印退出消息

                                    self.terminal
                                        .backend_mut()
                                        .execute(cursor::MoveToNextLine(1))?;
                                    let item = &chat_item[chat_index];
                                    if let (Some(msg), _) =
                                        chat_eg.get_start_end(item.start(), item.end())
                                    {
                                        //println!("{}", msg.join(""));
                                    }
                                }
                                // self.terminal.backend_mut().flush()?;
                                exit(0);
                                //break;
                            }
                            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                                crossterm::terminal::disable_raw_mode()?;
                                match self.ui_type {
                                    UIType::Full => {
                                        execute!(
                                            self.terminal.backend_mut(),
                                            LeaveAlternateScreen // 离开备用屏幕
                                        )?;
                                    }
                                    UIType::Lite => {
                                        self.terminal.show_cursor()?; // 确保光标可见
                                        self.terminal
                                            .backend_mut()
                                            .execute(MoveTo(0, self.start_row))?; // 假设从当前光标位置下移2行开始清除
                                        self.terminal.backend_mut().clear_region(
                                            ratatui::backend::ClearType::AfterCursor,
                                        )?; // 清除光标下方的区域
                                    }
                                }
                                exit(0);
                            }
                            (KeyCode::Enter, _) => match self.focus.current() {
                                FocusType::TxtFuzzy => {
                                    if let Some((_, _)) = self.navi.select_line {
                                        self.focus.next();
                                    } else {
                                        self.cmd_inp.clear();
                                        let cur_line = self.navi.get_cur_line();
                                        if cur_line < line_meta.len() {
                                            self.tv.set_scroll(line_meta[cur_line].get_line_num());
                                            self.navi.to_min_line();
                                        }
                                    }
                                    break;
                                }
                                FocusType::Chat => {
                                    let assist_inp = self.assist_inp.get_inp();
                                    if assist_inp.len() == 0 {
                                        break;
                                    }
                                    let mut message = String::new();
                                    if let Some((start_line, end_line)) = self.navi.select_line {
                                        // if let Some(line) = eg.get_start_end(start_line, end_line) {
                                        //     for l in line.iter() {
                                        //         message.push_str(l.get_txt());
                                        //     }
                                        // }
                                        message.push_str("\n");
                                    }
                                    message.push_str(assist_inp);

                                    if message.trim().len() > 0 {
                                        let chat_item_start = chat_eg.get_line_count().max(1);
                                        self.assist_tv.set_scroll(chat_item_start);
                                        chat_eg.push_str(&format!("----------------------------\n{}\n----------------------------\n",message));
                                        let chat_item_end = chat_eg.get_line_count().max(1);
                                        chat_item.push(ChatItemIndex(
                                            chat_item_start,
                                            chat_item_end - 1,
                                        ));
                                        self.assist_inp.clear();
                                        chat_index = chat_item.len() - 1;
                                        let _ = self.prompt_tx.send(message.to_string()).await;
                                        break;
                                    }
                                }
                                _ => {}
                            },
                            (KeyCode::Down, KeyModifiers::SHIFT) => {
                                // 下一页
                                match self.focus.current() {
                                    FocusType::TxtFuzzy => {
                                        let sel_line = if self.navi.is_bottom() {
                                            self.tv.down_line(eg.get_max_scroll_num());
                                            self.tv.scroll + self.tv.get_height()
                                        } else {
                                            self.navi.down_line();
                                            let cur_line = self.navi.get_cur_line();
                                            if cur_line >= line_meta.len() {
                                                break;
                                            }
                                            let sel_line = line_meta[cur_line].get_line_num();
                                            sel_line
                                        };
                                        match self.navi.select_line {
                                            Some((st, en)) => {
                                                if sel_line == en {
                                                    if let Some(max_num) = eg.get_max_scroll_num() {
                                                        if sel_line == max_num {
                                                            self.navi.select_line =
                                                                Some((st, sel_line));
                                                        } else {
                                                            self.navi.select_line =
                                                                Some((sel_line, sel_line));
                                                        }
                                                    } else {
                                                        self.navi.select_line =
                                                            Some((sel_line, sel_line));
                                                    }
                                                } else if sel_line > st && sel_line > en {
                                                    self.navi.select_line = Some((st, sel_line));
                                                } else if sel_line > st && sel_line < en {
                                                    self.navi.select_line = Some((sel_line, en));
                                                }
                                            }
                                            None => {
                                                self.navi.select_line =
                                                    Some((sel_line - 1, sel_line - 1));
                                            }
                                        }

                                        break;
                                    }
                                    FocusType::Chat => {
                                        self.assist_tv.down_line(chat_eg.get_max_scroll_num());
                                        break;
                                    }
                                    _ => {}
                                }
                                break;
                            }
                            (KeyCode::Up, KeyModifiers::SHIFT) => {
                                match self.focus.current() {
                                    FocusType::TxtFuzzy => {
                                        let sel_line = if self.navi.is_top() {
                                            self.tv.up_line();
                                            (self.tv.scroll - 1).max(1)
                                        } else {
                                            self.navi.up_line();

                                            let cur_line = self.navi.get_cur_line();
                                            if cur_line >= line_meta.len() {
                                                break;
                                            }
                                            let sel_line = line_meta[cur_line].get_line_num();
                                            sel_line
                                        };

                                        match self.navi.select_line {
                                            Some((st, en)) => {
                                                if sel_line == st {
                                                    if sel_line == 1 {
                                                        self.navi.select_line = Some((st, en));
                                                    } else {
                                                        self.navi.select_line =
                                                            Some((sel_line, sel_line));
                                                    }
                                                } else if sel_line > st && sel_line < en {
                                                    self.navi.select_line = Some((st, sel_line));
                                                } else if sel_line < st {
                                                    self.navi.select_line = Some((sel_line, en));
                                                }
                                            }
                                            None => {
                                                self.navi.select_line =
                                                    Some((sel_line + 1, sel_line + 1));
                                            }
                                        }
                                    }

                                    FocusType::Chat => {
                                        // self.assist_tv.up_line();
                                    }
                                    _ => {}
                                }
                                break;
                            }
                            (KeyCode::Down, KeyModifiers::CONTROL) => {
                                // 下一页
                                match self.focus.current() {
                                    FocusType::TxtFuzzy => {
                                        self.tv.down_page(eg.get_max_scroll_num());
                                    }
                                    FocusType::Chat => {}
                                    _ => {}
                                }
                                break;
                            }
                            (KeyCode::Up, KeyModifiers::CONTROL) => {
                                match self.focus.current() {
                                    FocusType::TxtFuzzy => {
                                        self.tv.up_page();
                                    }
                                    FocusType::Chat => {}
                                    _ => {}
                                }

                                break;
                            }
                            (KeyCode::Up, _) => {
                                // 向上滚动
                                match self.focus.current() {
                                    FocusType::TxtFuzzy => {
                                        if self.navi.is_top() {
                                            self.tv.up_line();
                                        } else {
                                            self.navi.up_line();
                                        }
                                        break;
                                    }
                                    FocusType::Chat => {
                                        // self.assist_tv.up_line();
                                        if chat_index >= chat_item.len() {
                                            break;
                                        }
                                        let chat_item_index = &chat_item[chat_index];
                                        let start = chat_item_index.start();
                                        let pre_scorll = (self
                                            .assist_tv
                                            .get_scroll()
                                            .saturating_sub(self.assist_tv.get_height()))
                                        .max(1);
                                        if pre_scorll >= start {
                                            self.assist_tv.set_scroll(pre_scorll);
                                        } else if self.assist_tv.get_scroll() <= start {
                                            chat_index = chat_index.saturating_sub(1);
                                            let pre_chat_item_index = &chat_item[chat_index];
                                            let pre_start = pre_chat_item_index.start();
                                            self.assist_tv.set_scroll(pre_start);
                                        } else if pre_scorll < start {
                                            self.assist_tv.set_scroll(start);
                                        }

                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            (KeyCode::Down, _) => {
                                // 向下滚动
                                match self.focus.current() {
                                    FocusType::TxtFuzzy => {
                                        if self.navi.is_bottom() {
                                            self.tv.down_line(eg.get_max_scroll_num());
                                        } else {
                                            self.navi.down_line();
                                        }
                                        break;
                                    }
                                    FocusType::Chat => {
                                        match chat_type {
                                            ChatType::ChatTv => {
                                                // 下一个item
                                                if chat_index >= chat_item.len() {
                                                    break;
                                                }
                                                let chat_item_index = &chat_item[chat_index];
                                                let start = chat_item_index.start();
                                                let end = chat_item_index.end();
                                                if self.assist_tv.get_scroll() < start {
                                                    self.assist_tv.set_scroll(start);
                                                } else if self.assist_tv.get_scroll()
                                                    + self.assist_tv.get_height()
                                                    <= end
                                                {
                                                    self.assist_tv.set_scroll(
                                                        self.assist_tv.get_scroll()
                                                            + self.assist_tv.get_height(),
                                                    );
                                                } else if self.assist_tv.get_scroll()
                                                    + self.assist_tv.get_height()
                                                    > end
                                                {
                                                    if chat_index + 1 < chat_item.len() {
                                                        chat_index += 1;
                                                        let next_chat_item_index =
                                                            &chat_item[chat_index];
                                                        let next_start =
                                                            next_chat_item_index.start();
                                                        self.assist_tv.set_scroll(next_start);
                                                    }
                                                }
                                            }
                                            ChatType::Promt => {}
                                            ChatType::Pattern => {}
                                        }

                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            (KeyCode::Char(c), _) => {
                                match self.focus.current() {
                                    FocusType::TxtFuzzy => {
                                        if self.cmd_inp.get_inp().len() >= 10 {
                                            break;
                                        }
                                        self.cmd_inp.push(c); // 添加字符到输入缓冲区
                                        self.tv.set_scroll(1);
                                        break;
                                    }
                                    FocusType::Chat => {
                                        self.assist_inp.push(c); // 添加字符到输入缓冲区
                                        break;
                                    }
                                    _ => {}
                                }

                                break;
                            }
                            (KeyCode::Backspace, _) => {
                                match self.focus.current() {
                                    FocusType::TxtFuzzy => {
                                        self.cmd_inp.pop();
                                        self.tv.set_scroll(1);
                                        break;
                                    }
                                    FocusType::Chat => {
                                        self.assist_inp.pop();
                                        break;
                                    }
                                    _ => {}
                                }
                                break;
                            }

                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

pub(crate) struct Navigation {
    min_line: usize,
    max_line: usize,
    cur_line: usize,
    select_line: Option<(usize, usize)>,
    rect: Rect,
}

impl Navigation {
    pub(crate) fn clear(&mut self) {
        self.select_line = None;
    }

    fn get_rect(&self) -> Rect {
        self.rect
    }

    fn is_top(&self) -> bool {
        self.cur_line == self.min_line
    }

    fn is_bottom(&self) -> bool {
        self.cur_line == self.max_line
    }

    fn down_line(&mut self) {
        if self.cur_line < self.max_line {
            self.cur_line += 1;
        }
    }

    fn up_line(&mut self) {
        if self.cur_line > self.min_line {
            self.cur_line -= 1;
        }
    }

    fn get_cur_line(&self) -> usize {
        self.cur_line
    }

    fn to_min_line(&mut self) {
        self.cur_line = self.min_line
    }

    fn to_max_line(&mut self) {
        self.cur_line = self.max_line
    }

    fn set_cur_line(&mut self, cur_line: usize) {
        self.cur_line = cur_line
    }
}

pub(crate) struct TextView {
    height: usize,
    width: usize,
    scroll: usize, //当前页 第一行 行数
    rect: Rect,
}

impl TextView {
    fn get_rect(&self) -> Rect {
        self.rect
    }

    pub(crate) fn get_height(&self) -> usize {
        self.height
    }

    pub(crate) fn get_width(&self) -> usize {
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
            self.scroll = ((self.scroll + self.height).min(max_scroll_num)).max(1)
        } else {
            self.scroll += self.height;
        }
    }
}

pub(crate) struct CmdInput {
    input: String,
    rect: Rect,
}

impl CmdInput {
    pub(crate) fn new(rect: Rect) -> Self {
        CmdInput {
            input: String::new(),
            rect,
        }
    }

    fn get_rect(&self) -> Rect {
        self.rect
    }

    pub(crate) fn clear(&mut self) {
        self.input.clear();
    }

    pub(crate) fn push(&mut self, c: char) {
        self.input.push(c);
    }

    pub(crate) fn push_str(&mut self, c: &str) {
        self.input.push_str(c);
    }

    pub(crate) fn pop(&mut self) {
        self.input.pop();
    }

    pub(crate) fn len(&mut self) -> usize {
        self.input.len()
    }

    pub(crate) fn get_inp(&self) -> &str {
        &self.input
    }

    fn get_inp_exact(&self) -> (&str, bool) {
        return if let Some(first_char) = &self.input.chars().next() {
            if *first_char == '/' {
                (&self.input[1..].trim(), false)
            } else {
                (&self.input.trim(), true)
            }
        } else {
            (&self.input.trim(), true)
        };
    }
}

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

fn get_chat_content<'a>(
    txts: &Vec<&'a str>,
    line_meta: &Vec<LineMeta>,
    chat_item: &ChatItemIndex,
) -> Text<'a> {
    let mut lines = Vec::with_capacity(line_meta.len());
    //  debug!("{:?},{:?}", line_meta, chat_item);
    for (i, txt) in txts.into_iter().enumerate() {
        let line_num = line_meta[i].get_line_num();
        // debug!("text: {:?}", *text);
        if line_num >= chat_item.start() && line_num <= chat_item.end() {
            lines.push(Line::from(Span::styled(
                *txt,
                Style::default().fg(Color::Green),
            )));
        } else {
            lines.push(Line::from(*txt));
        }
    }
    let text = Text::from(lines);
    text
}

fn n_chars_skip_control_mem_opt(s: &str, n: usize) -> (&str, &str, &str) {
    let mut count = 0;
    let mut start_idx = None;
    let mut end_idx = None;

    for (idx, ch) in s.char_indices() {
        if ch.is_control() {
            continue;
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
    let start = start_idx.unwrap_or_else(|| s.len());
    let end = end_idx.unwrap_or_else(|| s.len());

    (&s[..start], &s[start..end], &s[end..])
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
    format!("{:<20.10}|", data)
}

static FIELDS: &[(&str, ParserFn)] = &[
    ("| Binary (8bit)      | ", |bv| {
        format_data_inspector(bv.to_binary_8bit())
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
    ("| uint24_t           | ", |bv| {
        format_data_inspector(bv.to_u24())
    }),
    ("| int24_t            | ", |bv| {
        format_data_inspector(bv.to_i24())
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
];

fn get_data_inspector_content<'a>(seek: usize, buf: Vec<u8>, endian: Endian) -> Text<'a> {
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

const HEX_TOP: &'static str =
    "00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F  10 11 12       ASCII";

fn get_hex_content<'a>(
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

        spans.push(Span::raw("   ".repeat(20 - txt.len() + 1)));
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

fn get_edit_content<'a>(
    txts: &'a RingVec<CacheStr>,
    line_meta: &'a RingVec<EditLineMeta>,
    cur_line: usize,
    select_line: &Option<(usize, usize)>,
    height: usize,
    offset: usize,
    cursor_y: usize,
    cursor_x: usize,
) -> (Text<'a>, Text<'a>) {
    assert!(txts.len() == line_meta.len());
    let mut lines = Vec::with_capacity(line_meta.len());
    for (i, txt) in txts.iter().enumerate() {
        let (str1, str2) = txt.text(offset..);
        if cursor_y == i {
            let mut spans = Vec::new();
            if cursor_x < str1.chars().count() {
                let (a, b, c) = n_chars_skip_control_mem_opt(str1.as_ref(), cursor_x);
                if b.len() > 0 {
                    spans.push(Span::raw(a.to_string()));
                    spans.push(Span::styled(
                        b.to_string(),
                        Style::default().bg(Color::LightRed),
                    ));
                    spans.push(Span::raw(c.to_string()));
                    spans.push(Span::raw(str2));
                } else {
                    spans.push(Span::raw(str1));
                    spans.push(Span::raw(str2));
                    let diff = cursor_x.saturating_sub(txt.len());
                    let padding = " ".repeat(diff);
                    spans.push(Span::raw(padding));
                    // 在填充后显示高亮的光标
                    spans.push(Span::styled(" ", Style::default().bg(Color::LightRed)));
                }
            } else {
                let (a, b, c) = n_chars(str2.as_ref(), cursor_x - str1.chars().count());
                if b.len() > 0 {
                    spans.push(Span::raw(str1));
                    spans.push(Span::raw(a.to_string()));
                    spans.push(Span::styled(
                        b.to_string(),
                        Style::default().bg(Color::LightRed),
                    ));
                    spans.push(Span::raw(c.to_string()));
                } else {
                    spans.push(Span::raw(str1));
                    spans.push(Span::raw(str2));
                    let diff = cursor_x.saturating_sub(txt.len());
                    let padding = " ".repeat(diff);
                    spans.push(Span::raw(padding));
                    // 在填充后显示高亮的光标
                    spans.push(Span::styled(" ", Style::default().bg(Color::LightRed)));
                }
            }

            lines.push(Line::from(spans));
        } else {
            let spans = vec![Span::raw(str1), Span::raw(str2)];
            lines.push(Line::from(spans));
        }
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
                if i > line_meta.len() {
                    return Line::raw("");
                }
                Line::from(Span::styled(
                    format!("{:>4} ", line_meta.get(i).unwrap().get_line_num()),
                    Style::default().fg(Color::White),
                ))
                // 高亮当前行
            })
            .collect::<Vec<Line>>(),
    );

    let text = Text::from(lines);
    (nav_text, text)
}

fn get_content<'a>(
    txts: &'a Vec<&str>,
    line_meta: &'a Vec<LineMeta>,
    cur_line: usize,
    select_line: &Option<(usize, usize)>,
    height: usize,
) -> (Text<'a>, Text<'a>) {
    // assert!(content.len() == line_meta.len());
    let mut lines = Vec::with_capacity(line_meta.len());
    for (i, txt) in txts.into_iter().enumerate() {
        let mut spans = Vec::new();
        let mf = line_meta[i].get_match(); //fuzzy_search(input, text, false);
        if let Some(m) = mf {
            match m {
                Match::Char(_) => {
                    todo!()
                }
                Match::Byte(v) => {
                    let mut current_idx = 0;
                    for bm in v.into_iter() {
                        if current_idx < bm.start && bm.start <= txt.len() {
                            spans.push(Span::raw(&txt[current_idx..bm.start]));
                        }
                        // 添加高亮文本
                        if bm.start < txt.len() && bm.end <= txt.len() {
                            spans.push(Span::styled(
                                &txt[bm.start..bm.end],
                                Style::default().bg(Color::Green),
                            ));
                        }
                        // 更新当前索引为高亮区间的结束位置
                        current_idx = bm.end;
                    }
                    // 添加剩余的文本（如果有）
                    if current_idx < txt.len() {
                        spans.push(Span::raw(&txt[current_idx..]));
                    }
                }
            }
        }
        if let Some((st, en)) = select_line {
            if (line_meta[i].get_line_num() >= *st && line_meta[i].get_line_num() <= *en)
                || i == cur_line
            {
                lines.push(Line::from(Span::styled(
                    *txt,
                    Style::default().bg(Color::LightRed), // 设置背景颜色为红色
                )));
            } else {
                if spans.len() > 0 {
                    lines.push(Line::from(spans));
                } else {
                    lines.push(Line::from(*txt));
                }
            }
        } else {
            if i == cur_line {
                lines.push(Line::from(Span::styled(
                    *txt,
                    Style::default().bg(Color::LightRed), // 设置背景颜色为蓝色
                )));
            } else {
                if spans.len() > 0 {
                    lines.push(Line::from(spans));
                } else {
                    lines.push(Line::from(*txt));
                }
            }
        }
    }

    let nav_text = Text::from(
        (0..height)
            .enumerate()
            .map(|(i, _)| {
                if i == cur_line {
                    Line::from(Span::styled(">", Style::default().fg(Color::LightRed)))
                // 高亮当前行
                } else {
                    Line::from(" ") // 非当前行为空白
                }
            })
            .collect::<Vec<Line>>(),
    );

    let text = Text::from(lines);
    (nav_text, text)
}

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
