pub(crate) mod edit;
pub(crate) mod hex;
pub(crate) mod text;
use crate::byteutil::Endian;
use crate::cli::UIType;
use crate::command::Command;
use crate::common::error::ChapResult;
use crate::common::ring_vec::RingVec;
use crate::fuzzy::Match;
use crate::handle::text::HandleText;
use crate::handle::Handle;
use crate::handle::HandleEdit;
use crate::handle::HandleHex;
use crate::handle::HandleImpl;
use crate::lua::LuaPlugin;
use crate::textwarp::edit::GapText;
use crate::textwarp::edit_block::GapBlockText;
use crate::textwarp::hex::HexText;
use crate::textwarp::text::MmapText;
use crate::textwarp::CacheStr;
use crate::textwarp::EditTextWarp;
use crate::textwarp::LineParts;
use crate::textwarp::LineState;
use crate::textwarp::TextDisplay;
use crate::textwarp::TextOper;
use crate::textwarp::TextSelect;
use crate::textwarp::TextWarp;
use crate::textwarp::TextWarpType;
use crate::tui::edit::EditBuildContent;
use crate::tui::hex::get_data_inspector_content;
use crate::tui::hex::get_hex_content;
use crate::tui::text::TextBuildContent;
use crate::undo::undo::UndoFile;
use crossterm::event::EnableBracketedPaste;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::execute;
use crossterm::{
    cursor,
    event::{self, KeyCode},
};
use ratatui::prelude::Constraint;
use ratatui::prelude::CrosstermBackend;
use ratatui::prelude::Direction;
use ratatui::prelude::Layout;
use ratatui::prelude::Rect;
use ratatui::prelude::Size;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use utf8_iter::Utf8CharsEx;

pub(crate) struct Content<'a> {
    pub(crate) navi: Text<'a>,
    //pub(crate) visible_content: Text<'a>,
    pub(crate) byte_cursor: usize,
    pub(crate) last_char_bytes_size: usize,
}

pub(crate) struct SearchResultEntry {
    pub(crate) result_line_index: usize,
    pub(crate) source_state: LineState,
    pub(crate) highlight: Vec<Match>,
}

pub(crate) struct SearchResultStore {
    pub(crate) td: TextDisplay,
    pub(crate) entries: Vec<SearchResultEntry>,
    pub(crate) pattern_len: usize,
}

pub(crate) enum HighlightSource<'a> {
    None,
    CurrentFind {
        line_index: usize,
        matches: &'a [Match],
    },
    SearchResult {
        entries: &'a [SearchResultEntry],
    },
}

impl<'a> HighlightSource<'a> {
    pub(crate) fn for_line(&self, line_index: usize) -> &'a [Match] {
        match self {
            HighlightSource::None => &[],
            HighlightSource::CurrentFind {
                line_index: target,
                matches,
            } if *target == line_index => matches,
            HighlightSource::SearchResult { entries } => entries
                .get(line_index)
                .filter(|entry| entry.result_line_index == line_index)
                .map(|entry| entry.highlight.as_slice())
                .unwrap_or(&[]),
            _ => &[],
        }
    }
}

pub(crate) struct EditContext<'a> {
    pub(crate) height: usize,
    pub(crate) column_offset: usize,
    pub(crate) cursor_y: usize,
    pub(crate) cursor_x: usize,
    pub(crate) is_txt_model: bool,
    // pub(crate) find_highlight_offset: usize,
    // pub(crate) find_line_index: Option<usize>,
    // pub(crate) highlight_len: usize,
    pub(crate) highlights: HighlightSource<'a>,
    pub(crate) highlight_len: usize,
}

pub(crate) trait BuildContent {
    fn build_content<'a>(
        lines: &mut Vec<Line<'a>>,
        txts: &'a RingVec<CacheStr>,
        with: usize,
        line_meta: &'a RingVec<LineState>,
        cur_line: usize,
        select_line: &Option<(usize, usize)>,
        ed_ctx: &EditContext<'_>,
    ) -> Content<'a>;

    fn command_focus() -> bool;
}

pub(crate) enum ChapMod {
    Edit,      //普通编辑器模式
    EditBlock, //普通编辑器模式
    Hex,       //16进制编辑器模式
    Text,      //大文本浏览模式
    Vector,    //向量分析模式
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

#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum InputFocus {
    Text,
    Command,
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

pub(crate) struct TuiElement {
    pub(crate) navi: Navigation,
    pub(crate) tv: TextView,
    pub(crate) cmd_title: Rect,
    pub(crate) cmd_inp: CmdInput,
    pub(crate) assist_tv1: TextView,
    pub(crate) assist_tv2: TextView,
}

pub(crate) enum ViewMode {
    Normal,
    SearchResult,
}

pub(crate) enum RenderSource {
    File(PathBuf),
    StdinTemp(NamedTempFile),
}

pub(crate) struct ChapTui {
    pub(crate) chap_mod: ChapMod,
    ui_type: UIType,
    size: Size,
    pub(crate) warp_type: TextWarpType,
    pub(crate) terminal: Terminal<CrosstermBackend<io::Stdout>>,
    pub(crate) elem: TuiElement,
    pub(crate) back_linenum: Vec<usize>, // 上一行号
    pub(crate) txt_sel: TextSelect,      // 文本选择
    pub(crate) cursor_x: usize,          //文本 光标x坐标 视觉坐标
    pub(crate) cursor_y: usize,          //文本 光标y坐标 视觉坐标
    pub(crate) inp_cursor_x: usize,      //命令行 文本 光标x坐标 视觉坐标
    pub(crate) inp_cursor_y: usize,      //命令行 文本 光标y坐标 视觉坐标
    pub(crate) column_offset: usize,     // 列偏移量
    pub(crate) bytes_cursor: usize,      //字节偏移量
    pub(crate) bytes_cursor_size: usize, //字节偏移量
    //pub(crate) start_line_num: usize,    // 起始行号
    pub(crate) start_line_state: LineState, //起始的状态
    pub(crate) is_last_line: bool,          // 是否是最后一行
    pub(crate) endian: Endian,              // 字节序
    pub(crate) assist_tv2_data: String,     // 辅助窗口2数据
    pub(crate) undo: Option<UndoFile>,
    pub(crate) find_list: Option<Vec<LineState>>,
    pub(crate) find_index: usize, // 搜索的时候跳转到第几个find_list的条目
    pub(crate) find_highlight_index: usize, //一行搜索的关键字中 高亮第几个关键字
    pub(crate) highlight_len: usize, //关键字高亮的长度
    pub(crate) cur_cmd: Command,
    pub(crate) input_focus: InputFocus,

    pub(crate) search_result: Option<SearchResultStore>,
    pub(crate) view_mode: ViewMode,
    pub(crate) search_result_index: usize,
}

impl ChapTui {
    pub(crate) fn enter_command_mode(&mut self) {
        self.input_focus = InputFocus::Command;
    }

    fn enter_text_mode(&mut self) {
        self.input_focus = InputFocus::Text;
        self.find_list = None;
        self.find_index = 0;
        self.highlight_len = 0;
    }

    pub(crate) fn in_command_mode(&self) -> bool {
        self.input_focus == InputFocus::Command
    }

    pub(crate) fn new(
        chap_mod: ChapMod,
        ui_type: UIType,
        que: bool,
        undo: Option<UndoFile>,
    ) -> ChapResult<ChapTui> {
        let (_, row) = cursor::position()?; // (x, y) 返回的是光标的 (列号, 行号)
                                            //let backend = CrosstermBackend::new(std::io::stdout());
        let mut terminal = ratatui::init();
        execute!(terminal.backend_mut(), EnableBracketedPaste)?;
        let size = terminal.size()?;
        let elem = Self::get_react(&ui_type, &chap_mod, &size)?;
        Ok(ChapTui {
            chap_mod: chap_mod,
            size: size,
            warp_type: TextWarpType::SoftWrap,
            terminal: terminal,
            elem: elem,
            ui_type: ui_type,
            back_linenum: Vec::with_capacity(10), // 初始化上一行号
            txt_sel: TextSelect::new(),
            cursor_x: 0,
            cursor_y: 0,
            inp_cursor_x: 0,
            inp_cursor_y: 0,
            column_offset: 0,
            bytes_cursor: 0,
            bytes_cursor_size: 0,
            start_line_state: LineState::default(),
            is_last_line: false,
            endian: Endian::Little, // 默认字节序为小端
            assist_tv2_data: String::new(),
            undo: undo,
            find_list: None,
            find_index: 0,
            find_highlight_index: 0,
            highlight_len: 0,
            cur_cmd: Command::Empty,
            input_focus: InputFocus::Text,
            search_result: None,
            view_mode: ViewMode::Normal,
            search_result_index: 0,
        })
    }

    //
    fn get_react(ui_type: &UIType, chap_mod: &ChapMod, size: &Size) -> ChapResult<TuiElement> {
        let (tui_height, tui_width, start_row) = match ui_type {
            UIType::Full => (size.height, size.width, 0),
            UIType::Lite => {
                todo!()
            }
        };

        let nav_with = match chap_mod {
            ChapMod::EditBlock => 5,
            ChapMod::Edit => 5,
            ChapMod::Hex => 8,
            ChapMod::Text => 5,
            ChapMod::Vector => 5,
        };

        // 文本框显示内容的高度
        // let tv_heigth = (tui_height - 1) as usize;
        // 文本框显示内容的宽度
        //let tv_width = (tui_width as f32) as usize;

        //let assist_tv_width = (tui_width as f32 * 0.5) as usize; //(tui_width as f32 * 0.0) as usize - 3;

        let max_line = (tui_height - 3) as usize;
        let hex_with = if 82 < tui_width { 82 } else { tui_width };
        let p = match chap_mod {
            ChapMod::EditBlock => 100,
            ChapMod::Edit => 100,
            ChapMod::Hex => ((hex_with as f32 / tui_width as f32) * 100.0) as u16,
            ChapMod::Text => 100,
            ChapMod::Vector => 0,
        };
        let rect = Rect::new(0, start_row, tui_width, tui_height);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(p), Constraint::Percentage(100 - p)].as_ref())
            .split(rect);

        let (nav_chk, tv_chk, inp_title_chk, seach_chk, assist_tv_chk1, assist_tv_chk2) = {
            //文本框和输入框
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(100), Constraint::Length(1)].as_ref())
                .split(chunks[0]); // chunks[1] 是左侧区域

            //LLM聊天和输入框
            let right_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(chunks[1]); // chunks[1] 是右侧区域

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
            height: tv_chk.height as usize,
            width: tv_chk.width as usize,
            scroll: 1,
            rect: tv_chk,
        };

        let cmd_inp = CmdInput::new(seach_chk);

        let assist_tv1 = TextView {
            height: assist_tv_chk1.height as usize,
            width: assist_tv_chk1.width as usize,
            scroll: 1,
            rect: assist_tv_chk1,
        };
        let assist_tv2 = TextView {
            height: assist_tv_chk2.height as usize,
            width: assist_tv_chk2.width as usize,
            scroll: 1,
            rect: assist_tv_chk2,
        };

        Ok(TuiElement {
            navi: navi,
            tv: tv,
            cmd_title: inp_title_chk,
            cmd_inp: cmd_inp,
            assist_tv1: assist_tv1,
            assist_tv2: assist_tv2,
        })
    }

    pub(crate) fn set_endian(&mut self, endian: Endian) {
        self.endian = endian;
    }

    fn render_hex<'a>(
        &mut self,
        visible_content: Text,
        content: Content,
        hex_sel: TextSelect,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        self.terminal.draw(|f| {
            let text_para = Paragraph::new(visible_content)
                .block(Block::default())
                .style(Style::default().fg(Color::White));
            f.render_widget(text_para, self.elem.tv.get_rect());

            let nav_paragraph = Paragraph::new(content.navi);
            f.render_widget(nav_paragraph, self.elem.navi.get_rect());

            let sel_content = td.get_text_from_sel(&hex_sel);
            let assist =
                get_data_inspector_content(hex_sel.get_start(), sel_content, self.endian.clone());
            let assist_para1 = Paragraph::new(assist)
                .block(Block::default())
                .style(Style::default().fg(Color::White));
            f.render_widget(assist_para1, self.elem.assist_tv1.get_rect());

            let assist_para2 = Paragraph::new(Text::raw(&self.assist_tv2_data))
                .block(Block::default())
                .style(Style::default().fg(Color::White));
            f.render_widget(assist_para2, self.elem.assist_tv2.get_rect());

            let input_title_box = Paragraph::new(Text::raw(" >: "))
                .block(Block::default())
                .style(Style::default().fg(Color::White)); // 设置输入框样式
            f.render_widget(input_title_box, self.elem.cmd_title);

            let input_box = Paragraph::new(Text::raw(self.elem.cmd_inp.get_inp()))
                .block(Block::default())
                .style(Style::default().fg(Color::White));
            f.render_widget(input_box, self.elem.cmd_inp.get_rect());
        })?;
        Ok(())
    }

    fn get_hex_content<'a>(
        &mut self,
        cursor_x: usize,
        cursor_y: usize,
        hex_sel: TextSelect,
        td: &'a TextDisplay,
    ) -> ChapResult<(&'a RingVec<LineState>, Content<'a>)> {
        let (content, meta) = td.get_current_page()?;
        let visible_content = get_hex_content(
            content,
            &meta,
            self.elem.navi.get_cur_line(),
            &hex_sel,
            self.elem.tv.get_height(),
            cursor_y,
            cursor_x,
        );

        Ok((meta, visible_content))
    }

    pub(crate) fn render_source<P2: AsRef<Path>>(
        &mut self,
        source: RenderSource,
        plugin: P2,
    ) -> ChapResult<()> {
        match source {
            RenderSource::File(path) => {
                if let Err(e) = self.render(path, plugin) {
                    eprintln!("Error rendering file: {}", e);
                }
            }
            RenderSource::StdinTemp(temp_file) => {
                if let Err(e) = self.render(temp_file.path(), plugin) {
                    eprintln!("Error rendering temp file: {}", e);
                }
            }
        }
        Ok(())
    }

    fn active_text_display<'a>(&'a self, td: &'a TextDisplay) -> &'a TextDisplay {
        if matches!(self.view_mode, ViewMode::SearchResult) {
            self.search_result.as_ref().map(|s| &s.td).unwrap_or(td)
        } else {
            td
        }
    }

    pub(crate) fn render<P1: AsRef<Path>, P2: AsRef<Path>>(
        &mut self,
        p: P1,
        plugin: P2,
    ) -> ChapResult<()> {
        let hand = match self.chap_mod {
            ChapMod::Edit => HandleImpl::Edit(HandleEdit::new()),
            ChapMod::EditBlock => HandleImpl::Edit(HandleEdit::new()),
            ChapMod::Text => HandleImpl::Text(HandleText::new()),
            ChapMod::Hex => HandleImpl::Hex(HandleHex::new(LuaPlugin::new(plugin))),
            _ => {
                todo!()
            }
        };

        loop {
            let size = self.terminal.size()?;
            let elem = Self::get_react(&self.ui_type, &self.chap_mod, &size)?;
            self.size = size;
            self.elem = elem;
            self.cursor_x = 0;
            self.cursor_y = 0;
            let twy = self.warp_type;
            let mut td: TextDisplay = match self.chap_mod {
                ChapMod::EditBlock => TextDisplay::EditBlock(EditTextWarp::new(
                    GapBlockText::from_file_path(&p)?,
                    self.elem.tv.get_height(),
                    self.elem.tv.get_width(),
                    twy,
                )),
                ChapMod::Text => {
                    // return Ok(());
                    TextDisplay::Text(TextWarp::new(
                        MmapText::from_file_path(&p)?,
                        self.elem.tv.get_height(),
                        self.elem.tv.get_width(),
                        twy,
                    ))
                }
                ChapMod::Hex => TextDisplay::Hex(EditTextWarp::new(
                    HexText::from_file_path(&p, self.elem.tv.get_height() - 2)?,
                    self.elem.tv.get_height() - 2,
                    self.elem.tv.get_width(),
                    TextWarpType::NoWrap,
                )),
                ChapMod::Edit => TextDisplay::Edit(EditTextWarp::new(
                    GapText::from_file_path(&p)?,
                    self.elem.tv.get_height(),
                    self.elem.tv.get_width(),
                    twy,
                )),
                _ => {
                    todo!()
                }
            };

            td.get_one_page_from_state(&self.start_line_state)?;
            'tui: loop {
                let mut lines = Vec::with_capacity(self.elem.tv.get_height());
                let size = self.terminal.size()?;
                if size != self.size {
                    break 'tui;
                }
                let active_td: *const TextDisplay =
                    if matches!(self.view_mode, ViewMode::SearchResult) {
                        let search_result = self.search_result.as_ref().unwrap();
                        &search_result.td as *const TextDisplay
                    } else {
                        &td as *const TextDisplay
                    };

                let (line_meta, content) = match self.view_mode {
                    ViewMode::Normal => match self.chap_mod {
                        ChapMod::Edit => self.get_content::<EditBuildContent>(
                            &mut lines,
                            self.column_offset,
                            &td,
                        )?,
                        ChapMod::EditBlock => self.get_content::<EditBuildContent>(
                            &mut lines,
                            self.column_offset,
                            &td,
                        )?,
                        ChapMod::Text => {
                            unsafe {
                                self.get_content::<TextBuildContent>(
                                    &mut lines,
                                    self.column_offset,
                                    &*active_td,
                                )?
                            }
                            //  }
                        }
                        ChapMod::Hex => self.get_hex_content(
                            self.cursor_x,
                            self.cursor_y,
                            self.txt_sel.clone(),
                            &td,
                        )?,
                        _ => {
                            todo!()
                        }
                    },
                    ViewMode::SearchResult => unsafe {
                        self.get_content::<TextBuildContent>(
                            &mut lines,
                            self.column_offset,
                            &*active_td,
                        )?
                    },
                };

                let text = Text::from(lines);
                match self.view_mode {
                    ViewMode::Normal => match self.chap_mod {
                        ChapMod::Edit => self.render_content::<EditBuildContent>(text, content)?,
                        ChapMod::EditBlock => {
                            self.render_content::<EditBuildContent>(text, content)?
                        }
                        ChapMod::Text => self.render_content::<TextBuildContent>(text, content)?,
                        ChapMod::Hex => {
                            self.render_hex(text, content, self.txt_sel.clone(), &td)?
                        }
                        _ => {
                            todo!()
                        }
                    },
                    ViewMode::SearchResult => {
                        self.render_content::<TextBuildContent>(text, content)?
                    }
                }

                if let Some(start_line_meta) = line_meta.get(0) {
                    // self.start_line_num = start_line_meta.get_line_num();
                    self.start_line_state = start_line_meta.clone();
                }
                'key: loop {
                    match event::read()? {
                        event::Event::Key(KeyEvent {
                            code, modifiers, ..
                        }) => {
                            match (code, modifiers) {
                                (KeyCode::Esc, _) => {
                                    hand.handle_esc(self)?;
                                }
                                (KeyCode::Up, KeyModifiers::CONTROL) => {
                                    if let Err(e) = hand.handle_shift_up(self, &line_meta, &td) {
                                        self.assist_tv2_data = e.to_string(); // 记录错误信息
                                    }
                                }
                                (KeyCode::Down, KeyModifiers::CONTROL) => {
                                    if let Err(e) = hand.handle_shift_down(self, &line_meta, &td) {
                                        self.assist_tv2_data = e.to_string(); // 记录错误信息
                                    }
                                }
                                (KeyCode::Right, KeyModifiers::CONTROL) => {
                                    if let Err(e) = hand.handle_shift_right(self, &line_meta, &td) {
                                        self.assist_tv2_data = e.to_string(); // 记录错误信息
                                    }
                                }
                                (KeyCode::Left, KeyModifiers::CONTROL) => {
                                    if let Err(e) = hand.handle_shift_left(self, &line_meta, &td) {
                                        self.assist_tv2_data = e.to_string(); // 记录错误信息
                                    }
                                }
                                (KeyCode::Up, _) => {
                                    if let Err(e) =
                                        hand.handle_up(self, &line_meta, unsafe { &*active_td })
                                    {
                                        self.assist_tv2_data = e.to_string(); // 记录错误信息
                                    }
                                }
                                (KeyCode::Down, _) => {
                                    if let Err(e) =
                                        hand.handle_down(self, &line_meta, unsafe { &*active_td })
                                    {
                                        self.assist_tv2_data = e.to_string(); // 记录错误信息
                                    }
                                }
                                (KeyCode::Left, _) => {
                                    if let Err(e) = hand.handle_left(self, &line_meta, &td) {
                                        self.assist_tv2_data = e.to_string(); // 记录错误信息
                                    }
                                }
                                (KeyCode::Right, _) => {
                                    if let Err(e) = hand.handle_right(self, &line_meta, &td) {
                                        self.assist_tv2_data = e.to_string(); // 记录错误信息
                                    }
                                }
                                (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                                    if let Err(e) = hand.handle_ctrl_c(self) {
                                        self.assist_tv2_data = e.to_string(); // 记录错误信息
                                    }
                                }
                                (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                                    if let Err(e) = hand.handle_ctrl_s(self, &p, &mut td) {
                                        self.assist_tv2_data = e.to_string(); // 记录错误信息
                                    }
                                }
                                (KeyCode::Char('z'), KeyModifiers::CONTROL) => {
                                    if let Err(e) = hand.handle_ctrl_z(self, &td) {
                                        self.assist_tv2_data = e.to_string(); // 记录错误信息
                                    }
                                }
                                (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                                    self.elem.cmd_inp.clear();
                                    self.enter_text_mode();
                                }
                                (KeyCode::Char('x'), KeyModifiers::CONTROL) => {
                                    if let Some(meta) = line_meta.get(self.cursor_y) {
                                        let line = unsafe { (&*active_td).get_line_data(meta)? };
                                        ratatui::restore();
                                        use std::io::Write;
                                        let mut stdout = std::io::stdout();
                                        stdout.write_all(line.as_slice())?;
                                        stdout.write_all(b"\n")?;
                                        stdout.flush()?;
                                        std::process::exit(0);
                                    }
                                }
                                (KeyCode::Enter, _) => {
                                    if let Err(e) = hand.handle_enter(self, line_meta, &td) {
                                        self.assist_tv2_data = e.to_string();
                                        // 记录错误信息
                                    }
                                }
                                (KeyCode::Backspace, _) => {
                                    if let Err(e) = hand.handle_backspace(self, line_meta, &td) {
                                        self.assist_tv2_data = e.to_string();
                                        // 记录错误信息
                                    }
                                }
                                (KeyCode::Char(c), _) => {
                                    if let Err(e) = hand.handle_char(self, line_meta, &td, c) {
                                        self.assist_tv2_data = e.to_string();
                                        // 记录错误信息
                                    }
                                }

                                _ => {
                                    continue;
                                }
                            }
                            break 'key;
                        }
                        event::Event::Paste(mut pasted_string) => {
                            pasted_string = pasted_string.replace('\r', "\n");
                            if matches!(self.chap_mod, ChapMod::Edit) && self.in_command_mode() {
                                self.elem.cmd_inp.push_str(&pasted_string);
                            } else {
                                if let Err(e) =
                                    hand.handle_paste(self, &line_meta, &td, &pasted_string)
                                {
                                    self.assist_tv2_data = e.to_string(); // 记录错误信息
                                }
                            }
                            break 'key;
                        }
                        _ => {
                            continue;
                        }
                    }
                }
            }
        }
    }

    fn current_highlight_for_render(
        &self,
        meta: &RingVec<LineState>,
    ) -> (usize, Option<usize>, usize) {
        if matches!(self.view_mode, ViewMode::SearchResult) {
            let Some(store) = &self.search_result else {
                return (0, None, 0);
            };

            let row = self.cursor_y.min(meta.len().saturating_sub(1));
            let Some(result_meta) = meta.get(row) else {
                return (0, None, 0);
            };

            let Some(entry) = store.entries.get(result_meta.line_index) else {
                return (0, None, 0);
            };

            let Some(offset) = entry.highlight.get(self.find_highlight_index) else {
                return (0, None, 0);
            };

            return (
                offset.start,
                Some(result_meta.line_index),
                store.pattern_len,
            );
        }

        if let Some(find_list) = self.find_list.as_ref() {
            let state = &find_list[self.find_index];
            if let Some(h) = &state.highlight {
                return (
                    h[self.find_highlight_index].start,
                    Some(state.line_index),
                    self.highlight_len,
                );
            }
        }

        (0, None, 0)
    }

    pub(crate) fn render_content<T: BuildContent>(
        &mut self,
        text: Text,
        content: Content,
    ) -> ChapResult<()> {
        let command_focus = matches!(self.chap_mod, ChapMod::EditBlock) && self.in_command_mode();
        let cmd_rect = self.elem.cmd_inp.get_rect();
        // let cmd_input_len = self.elem.cmd_inp.get_inp().len() as u16;
        let tv_rect = self.elem.tv.get_rect();
        let tv_height = self.elem.tv.get_height();
        let tv_width = self.elem.tv.get_width();
        let navi_rect = self.elem.navi.get_rect();
        let navi_cur_line = self.elem.navi.get_cur_line();
        self.terminal.draw(|f| {
            let text_para = Paragraph::new(text)
                .block(Block::default())
                .style(Style::default().fg(Color::White));
            f.render_widget(text_para, tv_rect);

            let nav_paragraph = Paragraph::new(content.navi);
            f.render_widget(nav_paragraph, navi_rect);

            let prompt = if command_focus || T::command_focus() {
                ">: "
            } else {
                ""
            };
            let input_title_box = Paragraph::new(Text::raw(prompt))
                .block(Block::default())
                .style(Style::default().fg(Color::White));
            f.render_widget(input_title_box, self.elem.cmd_title);

            let input = self.elem.cmd_inp.get_inp();
            let input_text = if command_focus || T::command_focus() {
                let input_parts = [input.as_bytes()];
                let input_char_count = [self.inp_cursor_x];
                let (spans, _, _) =
                    build_cursor_line(&input_parts, input_char_count[0], &input_char_count, 0);
                Text::from(Line::from(spans))
            } else {
                Text::raw(input)
            };
            let input_para = Paragraph::new(input_text)
                .block(Block::default())
                .style(Style::default().fg(Color::White));
            f.render_widget(input_para, cmd_rect);
        })?;
        Ok(())
    }

    pub(crate) fn get_content<'a, T: BuildContent>(
        &mut self,
        lines: &mut Vec<Line<'a>>,
        offset: usize,
        td: &'a TextDisplay,
    ) -> ChapResult<(&'a RingVec<LineState>, Content<'a>)> {
        //let line_meta = {
        let (line_content, line_meta) = td.get_current_page()?;
        let command_focus = matches!(self.chap_mod, ChapMod::EditBlock) && self.in_command_mode();
        let cmd_rect = self.elem.cmd_inp.get_rect();
        // let cmd_input_len = self.elem.cmd_inp.get_inp().len() as u16;
        let tv_rect = self.elem.tv.get_rect();
        let tv_height = self.elem.tv.get_height();
        let tv_width = self.elem.tv.get_width();
        let navi_rect = self.elem.navi.get_rect();
        let navi_cur_line = self.elem.navi.get_cur_line();
        let select_line = self.elem.navi.select_line;
        let cursor_x_vis = self.cursor_x;
        let cursor_y_vis = self.cursor_y;
        let highlights = if matches!(self.view_mode, ViewMode::SearchResult) {
            self.search_result
                .as_ref()
                .map(|store| HighlightSource::SearchResult {
                    entries: store.entries.as_slice(),
                })
                .unwrap_or(HighlightSource::None)
        } else if let Some(find_list) = self.find_list.as_ref() {
            let state = &find_list[self.find_index];
            state
                .highlight
                .as_deref()
                .map(|matches| HighlightSource::CurrentFind {
                    line_index: state.line_index,
                    matches,
                })
                .unwrap_or(HighlightSource::None)
        } else {
            HighlightSource::None
        };

        let ed_ctx = EditContext {
            height: tv_height,
            column_offset: content_column_offset(self.warp_type, offset, self.elem.tv.width),
            cursor_y: cursor_y_vis,
            cursor_x: cursor_x_vis,
            is_txt_model: !command_focus,
            highlights,
            highlight_len: self.highlight_len,
        };
        let content  = //(navi, visible_content, byte_cursor, last_char_bytes_size)
                    T::build_content(
                        lines,
                        line_content,
                        tv_width,
                        &line_meta,
                        navi_cur_line,
                        &select_line,
                        &ed_ctx,
                    );
        self.bytes_cursor = content.byte_cursor;
        self.bytes_cursor_size = content.last_char_bytes_size;
        //let column_offset = self.column_offset;
        // let text = Text::from(lines);
        // self.terminal.draw(|f| {
        //     let text_para = Paragraph::new(text)
        //         .block(Block::default())
        //         .style(Style::default().fg(Color::White));
        //     f.render_widget(text_para, tv_rect);

        //     let nav_paragraph = Paragraph::new(content.navi);
        //     f.render_widget(nav_paragraph, navi_rect);

        //     let prompt = if command_focus || T::command_focus() {
        //         ">: "
        //     } else {
        //         ""
        //     };
        //     let input_title_box = Paragraph::new(Text::raw(prompt))
        //         .block(Block::default())
        //         .style(Style::default().fg(Color::White));
        //     f.render_widget(input_title_box, self.elem.cmd_title);

        //     let input = self.elem.cmd_inp.get_inp();
        //     let input_text = if command_focus || T::command_focus() {
        //         let input_parts = [input.as_bytes()];
        //         let input_char_count = [self.inp_cursor_x];
        //         let (spans, _, _) =
        //             build_cursor_line(&input_parts, input_char_count[0], &input_char_count, 0);
        //         Text::from(Line::from(spans))
        //     } else {
        //         Text::raw(input)
        //     };
        //     let input_para = Paragraph::new(input_text)
        //         .block(Block::default())
        //         .style(Style::default().fg(Color::White));
        //     f.render_widget(input_para, cmd_rect);
        // })?;
        //  meta
        //};
        return Ok((line_meta, content));
    }
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
    // 去掉多余的 .enumerate()：idx 从未被使用，直接取 char_indices() 的 (byte_index, ch)
    for (byte_index, ch) in s.char_indices() {
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

/// 单次字符遍历，直接返回字符范围 [char_start, char_end) 对应的可见字节切片（至多 2 段）。
///
/// # 优化说明
/// 原先两步：char_range_to_byte_range（扫描一遍求字节边界）+
///           slice_parts_range（再扫描一遍切取子切片）。
/// 本函数融合两步：在遍历中同时记录 char_start 所在位置，
/// 到达 char_end 时即刻提取切片返回，字节只扫描一遍，节省约 50% 扫描量。
///
/// 可见区域跨越 3+ 段时取前两段（与原 slice_parts_range 行为一致）。
// pub(crate) fn char_range_to_visible<'a>(
//     parts: &[&'a [u8]],
//     char_start: usize,
//     char_end: usize,
// ) -> LineParts<&'a [u8]> {
//     char_range_to_visible_with_byte_range(parts, char_start, char_end).0
// }

pub(crate) fn char_range_to_visible_with_byte_range<'a>(
    parts: &[&'a [u8]],
    char_start: usize,
    char_end: usize,
) -> (LineParts<&'a [u8]>, usize, usize) {
    let (visible, visible_byte_start, visible_byte_end, _) =
        char_range_to_visible_with_cursor_byte(parts, char_start, char_end, None);
    (visible, visible_byte_start, visible_byte_end)
}

pub(crate) fn char_range_to_visible_with_cursor_byte<'a>(
    parts: &[&'a [u8]],
    char_start: usize,
    char_end: usize,
    cursor_x: Option<usize>,
) -> (LineParts<&'a [u8]>, usize, usize, Option<usize>) {
    let mut visible = LineParts::empty();
    if char_end <= char_start || parts.is_empty() {
        return (visible, 0, 0, None);
    }

    let cursor_target = cursor_x.map(|x| char_start + x);
    let mut cursor_abs_byte = None;
    let mut char_count = 0usize;
    let mut start = None;
    let mut end = None;
    let mut base = 0usize;

    'outer: for (pi, part) in parts.iter().enumerate() {
        for (byte_idx, _) in part.char_indices() {
            if start.is_none() && char_count == char_start {
                start = Some((pi, byte_idx, base + byte_idx));
            }
            if cursor_abs_byte.is_none() && cursor_target == Some(char_count) {
                cursor_abs_byte = Some(base + byte_idx);
            }
            if char_count == char_end {
                end = Some((pi, byte_idx, base + byte_idx));
                break 'outer;
            }
            char_count += 1;
        }
        base += part.len();
    }

    let Some((start_pi, start_byte, visible_byte_start)) = start else {
        return (visible, 0, 0, None);
    };
    let (end_pi, end_byte, visible_byte_end) = end.unwrap_or_else(|| {
        let last_pi = parts.len() - 1;
        (last_pi, parts[last_pi].len(), base)
    });
    if cursor_abs_byte.is_none() && cursor_target == Some(char_count) {
        cursor_abs_byte = Some(visible_byte_end);
    }

    for pi in start_pi..=end_pi {
        let part_start = if pi == start_pi { start_byte } else { 0 };
        let part_end = if pi == end_pi {
            end_byte
        } else {
            parts[pi].len()
        };
        if part_start < part_end {
            visible.append(&parts[pi][part_start..part_end]);
        }
    }
    let cursor_byte = cursor_abs_byte
        .filter(|b| *b >= visible_byte_start && *b <= visible_byte_end)
        .map(|b| b - visible_byte_start);

    (visible, visible_byte_start, visible_byte_end, cursor_byte)
}

fn content_column_offset(warp_type: TextWarpType, offset: usize, width: usize) -> usize {
    match warp_type {
        TextWarpType::SoftWrap => 0,
        TextWarpType::NoWrap => offset.saturating_sub(width),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_wrap_render_does_not_apply_horizontal_column_offset() {
        assert_eq!(content_column_offset(TextWarpType::SoftWrap, 82, 80), 0);
    }

    #[test]
    fn no_wrap_render_keeps_horizontal_column_offset() {
        assert_eq!(content_column_offset(TextWarpType::NoWrap, 82, 80), 2);
    }

    #[test]
    fn char_range_to_visible_with_cursor_byte_maps_multibyte_cursor_in_one_scan() {
        let left = "中a".as_bytes();
        let right = "文bc\n".as_bytes();

        let (visible, start, end, cursor) =
            char_range_to_visible_with_cursor_byte(&[left, right], 1, 4, Some(1));

        assert_eq!(visible.as_parts(), [&b"a"[..], "文b".as_bytes()]);
        assert_eq!(start, 3);
        assert_eq!(end, 8);
        assert_eq!(cursor, Some(1));
    }

    #[test]
    fn char_range_to_visible_with_cursor_byte_maps_visible_end() {
        let line = "中abc".as_bytes();

        let (visible, start, end, cursor) =
            char_range_to_visible_with_cursor_byte(&[line], 1, 4, Some(3));

        assert_eq!(visible.as_parts(), [&b"abc"[..]]);
        assert_eq!(start, 3);
        assert_eq!(end, 6);
        assert_eq!(cursor, Some(3));
    }
}

// part 是一个连续的字节块，highlight_start和highlight_end 是高亮开始和结束的地方，然后这个函数把part
//切成非高亮块和高亮块 highlight_start和highlight_end 有可能跨字节块
fn cut_highlight_part<'a>(
    parts: &[&'a [u8]],
    start: usize,
    end: usize,
) -> (Vec<&'a [u8]>, Vec<&'a [u8]>, Vec<&'a [u8]>) {
    let mut pre = Vec::new();
    let mut mid = Vec::new();
    let mut post = Vec::new();
    let mut offset = 0;
    for part in parts {
        let part_len = part.len();
        let next_offset = offset + part_len;

        // 完全在 highlight 前
        if next_offset <= start {
            pre.push(*part);
        }
        // 完全在 highlight 后
        else if offset >= end {
            post.push(*part);
        }
        // 和 highlight 有交集
        else {
            let s = start.saturating_sub(offset);
            let e = (end - offset).min(part_len);
            // part 被切成三段逻辑区域，但我们仍然返回 slice（不能再细切 slice-of-slice）
            if offset < start {
                if s > 0 {
                    pre.push(&part[..s]);
                }
            }
            if e > s {
                mid.push(&part[s..e]);
            }
            if offset + part_len > end {
                if e < part_len {
                    post.push(&part[e..]);
                }
            }
        }
        offset = next_offset;
    }
    (pre, mid, post)
}

fn build_text_spans<'a>(
    parts: &[&'a [u8]],
    meta: &LineState,
    visible_byte_start: usize,
    visible_byte_end: usize,
    offsets: &[Match],
    cursor_x: Option<usize>,
) -> Vec<Span<'a>> {
    let visible_abs_start = meta.line_offset + visible_byte_start;
    let visible_abs_end = meta.line_offset + visible_byte_end;
    debug_assert!(offsets.windows(2).all(|w| w[0].start <= w[1].start));

    let mut spans = Vec::with_capacity(offsets.len().saturating_mul(2).saturating_add(2));
    let mut pos = 0usize;
    let total_len: usize = parts.iter().map(|p| p.len()).sum();
    let mut match_idx = offsets.partition_point(|m| m.end <= visible_abs_start);

    while pos < total_len {
        let abs_pos = visible_abs_start + pos;

        while match_idx < offsets.len() && offsets[match_idx].end <= abs_pos {
            match_idx += 1;
        }

        let mut next_boundary = total_len;
        let mut highlight = false;

        if let Some(m) = offsets
            .get(match_idx)
            .filter(|m| m.start < visible_abs_end && m.end > visible_abs_start)
        {
            let start = m.start.max(visible_abs_start) - visible_abs_start;
            let end = m.end.min(visible_abs_end) - visible_abs_start;

            if pos >= start && pos < end {
                next_boundary = end;
                highlight = true;
            } else if start > pos {
                next_boundary = start;
            }
        }

        if let Some(cursor) = cursor_x {
            if pos < cursor && cursor < next_boundary {
                next_boundary = cursor;
            } else if cursor == pos {
                next_boundary = next_utf8_boundary(parts, pos).unwrap_or(pos + 1);
            }
        }

        let cursor = cursor_x == Some(pos);
        push_range_span(&mut spans, parts, pos, next_boundary, highlight, cursor);
        pos = next_boundary;
    }

    if cursor_x == Some(total_len) {
        spans.push(Span::styled(
            " ",
            Style::default().bg(Color::LightRed).fg(Color::White),
        ));
    }

    spans
}

fn push_range_span<'a>(
    spans: &mut Vec<Span<'a>>,
    parts: &[&'a [u8]],
    start: usize,
    end: usize,
    highlight: bool,
    cursor: bool,
) {
    let mut base = 0usize;

    for part in parts {
        let part_end = base + part.len();

        if part_end <= start || base >= end {
            base = part_end;
            continue;
        }

        let s = start.saturating_sub(base);
        let e = (end - base).min(part.len());
        let text = std::str::from_utf8(&part[s..e]).unwrap_or("☻");

        let style = if cursor {
            Style::default().bg(Color::LightRed).fg(Color::White)
        } else if highlight {
            Style::default().bg(Color::Green)
        } else {
            Style::default()
        };

        spans.push(Span::styled(text, style));
        base = part_end;
    }
}

fn next_utf8_boundary(parts: &[&[u8]], pos: usize) -> Option<usize> {
    let bytes = byte_at(parts, pos)?;
    let len = if bytes < 0x80 {
        1
    } else if bytes & 0b1110_0000 == 0b1100_0000 {
        2
    } else if bytes & 0b1111_0000 == 0b1110_0000 {
        3
    } else if bytes & 0b1111_1000 == 0b1111_0000 {
        4
    } else {
        1
    };

    Some(pos + len)
}

fn byte_at(parts: &[&[u8]], pos: usize) -> Option<u8> {
    let mut base = 0usize;

    for part in parts {
        let end = base + part.len();
        if pos < end {
            return Some(part[pos - base]);
        }
        base = end;
    }

    None
}

pub(crate) fn build_highlight_spans<'a>(
    parts: &[&'a [u8]],
    line_meta: &LineState,
    target_line_index: usize,
    visible_start: usize,
    visible_end: usize,
    find_highlight_offset: &mut usize,
    highlight_len: &mut usize,
) -> Option<Vec<Span<'a>>> {
    if line_meta.get_line_index() != target_line_index {
        return None;
    }
    if !(line_meta.line_offset <= *find_highlight_offset
        && *find_highlight_offset < line_meta.get_line_end())
    {
        return None;
    }
    let highlight_abs_start = *find_highlight_offset;
    let highlight_abs_end = highlight_abs_start + *highlight_len;

    let visible_abs_start = line_meta.line_offset + visible_start;
    let visible_abs_end = line_meta.line_offset + visible_end;

    let start = highlight_abs_start.max(visible_abs_start);
    let end = highlight_abs_end.min(visible_abs_end);

    if start >= end {
        return None;
    }

    let highlight_start = start - visible_abs_start;
    let highlight_end = end - visible_abs_start;

    let (a, b, c) = cut_highlight_part(parts, highlight_start, highlight_end);

    let mut spans = Vec::with_capacity(a.len() + b.len() + c.len());

    for v in a {
        spans.push(Span::raw(str::from_utf8(v).unwrap_or("☻")));
    }
    for v in b {
        spans.push(Span::styled(
            str::from_utf8(v).unwrap_or("☻"),
            Style::default().bg(Color::Green),
        ));
    }
    for v in c {
        spans.push(Span::raw(str::from_utf8(v).unwrap_or("☻")));
    }

    let consumed = end.saturating_sub(highlight_abs_start);
    *highlight_len = highlight_len.saturating_sub(consumed);
    *find_highlight_offset = end;
    Some(spans)
}

pub(crate) fn build_cursor_line<'a>(
    str_parts: &[&'a [u8]],
    cursor_x: usize,
    char_count: &[usize],
    char_in_index: usize, //判断光标在那个 part中
) -> (Vec<Span<'a>>, usize, usize) {
    fn last_non_control_char_size(s: &[u8]) -> usize {
        s.char_indices()
            .rev()
            .find(|(_, ch)| !ch.is_control())
            .map(|(_, ch)| ch.len_utf8())
            .unwrap_or(0)
    }

    //let (str1, str2) = txt.text(offset..);
    // 优化2：预分配容量：最多 str_parts.len() 个普通段 + 3 个光标相关 span（前段/光标/后段）
    let mut spans = Vec::with_capacity(str_parts.len() + 3);
    let mut last_char_bytes_size: usize = 0;
    if str_parts.is_empty() {
        spans.push(Span::raw(" ".repeat(cursor_x)));
        spans.push(Span::styled(" ", Style::default().bg(Color::LightRed)));
        return (spans, 0, 0);
    }
    let char_in_index = char_in_index.min(str_parts.len() - 1);
    let byte_cursor;
    let (a, b, c) = if char_in_index == 0 {
        let (a, b, c, last_csz) = n_chars_skip_control_mem_opt(str_parts[0], cursor_x);
        last_char_bytes_size = last_csz;
        (a, b, c)
    } else {
        // 优化2：前几段字符数之和会被用两次，提前计算缓存，避免重复 .iter().sum()
        let prev_char_sum: usize = char_count[..char_in_index].iter().sum();
        let (a, b, c, last_csz) = n_chars_skip_control_mem_opt(
            str_parts[char_in_index],
            cursor_x.saturating_sub(prev_char_sum),
        );
        //如果上一个字符大小是0则光标可能在第一个字符那里
        if last_csz == 0 {
            last_char_bytes_size = last_non_control_char_size(str_parts[char_in_index - 1]);
        } else {
            last_char_bytes_size = last_csz;
        }
        (a, b, c)
    };
    if b.len() > 0 {
        byte_cursor = if char_in_index == 0 {
            a.get_non_control_len()
        } else {
            // 优化3：原来先推入 spans（遍历一次），再 fold 累加字节（遍历第二次），
            // 合并为单次循环：推入 span 的同时累加字节数
            let mut prefix_bytes = 0usize;
            for j in 0..char_in_index {
                let part = str_parts[j];
                prefix_bytes += part.len();
                spans.push(Span::raw(str::from_utf8(part).unwrap_or("☻")));
            }
            prefix_bytes + a.get_non_control_len()
        };
        let (display, color) = if b == b"\n" {
            (" ", Color::LightBlue)
        } else {
            (str::from_utf8(b).unwrap_or("☻"), Color::LightRed)
        };

        spans.push(Span::raw(str::from_utf8(a).unwrap_or("☻")));
        spans.push(Span::styled(display, Style::default().bg(color)));
        spans.push(Span::raw(str::from_utf8(c).unwrap_or("")));
        for j in (char_in_index + 1)..str_parts.len() {
            spans.push(Span::raw(str::from_utf8(str_parts[j]).unwrap_or("☻")));
        }
    } else {
        byte_cursor = if char_in_index == 0 {
            str_parts[0].get_non_control_len()
        } else {
            // 空切片 len()==0 不影响 fold 结果，filter 多余，直接删除
            str_parts[..char_in_index]
                .iter()
                .fold(0, |acc, s| acc + s.len())
                + str_parts[char_in_index].get_non_control_len()
        };
        for j in 0..str_parts.len() {
            spans.push(Span::raw(str::from_utf8(str_parts[j]).unwrap_or("☻")));
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
pub fn build_nav_text(line_meta: &RingVec<LineState>, height: usize) -> Text<'_> {
    let mut next_line_num = line_meta.get(0).map_or(1, |meta| meta.get_line_index() + 1);
    let mut nav_lines = Vec::with_capacity(height);
    for i in 0..height {
        let Some(meta) = line_meta.get(i) else {
            nav_lines.push(Line::raw(""));
            continue;
        };
        if meta.get_line_offset() > 0 {
            nav_lines.push(Line::raw(""));
            continue;
        }
        let line_num = (meta.get_line_index() + 1).max(next_line_num);
        next_line_num = line_num + 1;
        nav_lines.push(Line::from(Span::styled(
            format!("{:>4} ", line_num),
            Style::default().fg(Color::White),
        )));
    }
    Text::from(nav_lines)
}

#[cfg(test)]
impl ChapTui {
    /// 测试专用构造器（带 UndoFile），不初始化真实终端
    pub(crate) fn for_test_with_undo(tv_height: usize, tv_width: usize, undo: UndoFile) -> Self {
        let mut this = Self::for_test(tv_height, tv_width);
        this.undo = Some(undo);
        this
    }

    /// 测试专用构造器，不初始化真实终端
    pub(crate) fn for_test(tv_height: usize, tv_width: usize) -> Self {
        use ratatui::backend::CrosstermBackend;
        let terminal = Terminal::new(CrosstermBackend::new(std::io::stdout())).unwrap();
        let size = Size {
            width: 120,
            height: tv_height as u16 + 2,
        };
        let rect = Rect::new(0, 0, 120, tv_height as u16 + 2);
        let navi = Navigation {
            min_line: 0,
            max_line: tv_height.saturating_sub(2),
            cur_line: 0,
            rect: Rect::new(0, 0, 5, tv_height as u16),
            select_line: None,
        };
        let tv = TextView {
            height: tv_height,
            width: tv_width,
            scroll: 1,
            rect: Rect::new(5, 0, tv_width as u16, tv_height as u16),
        };
        let assist_tv1 = TextView {
            height: tv_height,
            width: 40,
            scroll: 1,
            rect: Rect::new(tv_width as u16 + 5, 0, 40, tv_height as u16 / 2),
        };
        let assist_tv2 = TextView {
            height: tv_height,
            width: 40,
            scroll: 1,
            rect: Rect::new(
                tv_width as u16 + 5,
                tv_height as u16 / 2,
                40,
                tv_height as u16 / 2,
            ),
        };
        let cmd_inp = CmdInput::new(Rect::new(4, tv_height as u16, tv_width as u16, 1));
        ChapTui {
            chap_mod: ChapMod::EditBlock,
            ui_type: crate::cli::UIType::Full,
            size,
            warp_type: crate::textwarp::TextWarpType::SoftWrap,
            terminal,
            elem: TuiElement {
                navi,
                tv,
                cmd_title: Rect::new(0, tv_height as u16, 4, 1),
                cmd_inp,
                assist_tv1,
                assist_tv2,
            },
            back_linenum: Vec::new(),
            txt_sel: crate::textwarp::TextSelect::new(),
            cursor_x: 0,
            cursor_y: 0,
            inp_cursor_x: 0,
            inp_cursor_y: 0,
            column_offset: 0,
            bytes_cursor: 0,
            bytes_cursor_size: 0,
            start_line_state: LineState::default(),
            is_last_line: false,
            endian: crate::byteutil::Endian::Little,
            assist_tv2_data: String::new(),
            undo: None,
            find_list: None,
            find_index: 0,
            find_highlight_index: 0,
            highlight_len: 0,
            cur_cmd: Command::Empty,
            input_focus: InputFocus::Text,

            search_result: None,
            view_mode: ViewMode::Normal,
            search_result_index: 0,
        }
    }
}
