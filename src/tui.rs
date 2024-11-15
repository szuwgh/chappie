use crate::chatapi::grop::ApiGroq;
use crate::error::ChapResult;
use crate::fuzzy::Match;
use crate::text::LineTxt;
use crate::text::SimpleTextEngine;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use crossterm::{
    cursor::{self, MoveTo},
    event::{self, Event, KeyCode},
    style::{self, Print},
    terminal::{self, Clear, ClearType},
    ExecutableCommand,
};
use groq_api_rs::completion::client::Groq;
use memmap2::Mmap;
use once_cell::sync::Lazy;
use ratatui::prelude::Constraint;
use ratatui::prelude::CrosstermBackend;
use ratatui::prelude::Direction;
use ratatui::prelude::Layout;
use ratatui::prelude::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use std::io;
use std::mem;
use std::process::exit;
use tokio::sync::mpsc;
use tokio::time::Duration;

pub(crate) struct ChapUI {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    navi: Navigation,
    tv: TextView,
    fuzzy_inp: FuzzyInput,
    chat_tv: TextView,
    chat_inp: ChatInput,
    focus: Focus,
    prompt_tx: mpsc::Sender<String>,
}

enum FocusType {
    Txt,
    FuzzyInput,
    ChatTxt,
    ChatInput,
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
            FocusType::Txt => (highlight, base, base, base),
            FocusType::FuzzyInput => (base, highlight, base, base),
            FocusType::ChatTxt => (base, base, highlight, base),
            FocusType::ChatInput => (base, base, base, highlight),
        }
    }

    // 获取当前焦点
    fn current(&self) -> FocusType {
        match self.current_focus {
            0 => FocusType::Txt,
            1 => FocusType::FuzzyInput,
            2 => FocusType::ChatTxt,
            3 => FocusType::ChatInput,
            _ => {
                todo!()
            }
        }
    }
}

impl ChapUI {
    pub(crate) fn new(prompt_tx: mpsc::Sender<String>) -> ChapResult<ChapUI> {
        enable_raw_mode()?;
        let stdout = std::io::stdout();
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        // 显示光标
        io::stdout().execute(cursor::Show)?;
        // 清空终端屏幕
        terminal.clear()?;
        // 获取终端尺寸
        let size = terminal.size()?;

        // 终端高度
        let terminal_height = size.height as usize;
        // 终端宽度
        let terminal_width = size.width as usize;

        // 文本框显示内容的高度
        let tv_heigth = terminal_height - 4;
        // 文本框显示内容的宽度
        let tv_width = (terminal_width as f32 * 0.6) as usize - 3;

        let chat_tv_width = (terminal_width as f32 * 0.4) as usize - 2;

        let max_line = terminal_height - 6;

        let rect = Rect::new(0, 0, size.width, size.height);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
            .split(rect);

        //文本框和输入框
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)].as_ref())
            .split(chunks[0]); // chunks[1] 是左侧区域

        //LLM聊天和输入框
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)].as_ref())
            .split(chunks[1]); // chunks[1] 是左侧区域

        //导航栏和文本框
        let nav_text_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(1), Constraint::Percentage(100)].as_ref())
            .split(left_chunks[0]); // chunks[1] 是左侧区域

        //导航栏
        let nav = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Percentage(100)].as_ref())
            .split(nav_text_chunks[0]); // chunks[1] 是左侧区域

        let navi = Navigation {
            max_line: max_line,
            min_line: 0,
            cur_line: 1,
            rect: nav[1],
        };

        let tv = TextView {
            height: tv_heigth,
            width: tv_width,
            scroll: 1,
            rect: nav_text_chunks[1],
        };

        let fuzzy_inp = FuzzyInput {
            input: String::new(),
            rect: left_chunks[1],
        };

        let chat_tv = TextView {
            height: tv_heigth,
            width: chat_tv_width,
            scroll: 1,
            rect: right_chunks[0],
        };

        let chat_inp = ChatInput {
            input: String::new(),
            rect: right_chunks[1],
        };

        Ok(ChapUI {
            terminal: terminal,
            navi: navi,
            tv: tv,
            fuzzy_inp: fuzzy_inp,
            chat_tv: chat_tv,
            chat_inp: chat_inp,
            focus: Focus::new(),
            prompt_tx: prompt_tx,
        })
    }

    pub(crate) async fn render(
        &mut self,
        bytes: &[u8],
        mut llm_res_rx: mpsc::Receiver<String>,
    ) -> ChapResult<()> {
        let mut eg: SimpleTextEngine<'_> =
            SimpleTextEngine::new(bytes, self.tv.get_height(), self.tv.get_width());
        let mut chat_txt = String::with_capacity(1024);
        loop {
            let (inp, is_exact) = self.fuzzy_inp.get_inp_exact();
            let content = eg.get_line(self.tv.get_scroll(), inp, is_exact);

            let mut chat_eg = SimpleTextEngine::new(
                chat_txt.as_bytes(),
                self.chat_tv.get_height(),
                self.chat_tv.get_width(),
            );
            self.terminal.draw(|f| {
                // 左下输入框区
                let (txt_clr, inp_clr, chat_clr_, chat_inp_clr) = self.focus.get_colors();
                let input_box = Paragraph::new(Text::raw(self.fuzzy_inp.get_inp()))
                    .block(Block::default().title("search").borders(Borders::ALL))
                    .style(Style::default().fg(inp_clr)); // 设置输入框样式
                f.render_widget(input_box, self.fuzzy_inp.get_rect());

                // 左下输入框区
                let input_box = Paragraph::new(Text::raw(self.fuzzy_inp.get_inp()))
                    .block(Block::default().title("search").borders(Borders::ALL))
                    .style(Style::default().fg(inp_clr)); // 设置输入框样式
                f.render_widget(input_box, self.fuzzy_inp.get_rect());
                let block = Block::default().borders(Borders::ALL).title("File Content");
                if let Some(c) = &content {
                    let (navi, visible_content) =
                        get_content(c, self.navi.get_cur_line(), self.tv.get_height());
                    let text_para = Paragraph::new(visible_content)
                        .block(block)
                        .style(Style::default().fg(txt_clr));
                    f.render_widget(text_para, self.tv.get_rect());
                    let nav_paragraph = Paragraph::new(navi);
                    f.render_widget(nav_paragraph, self.navi.get_rect());
                    // max_line = c.len();
                } else {
                    // println!("scorll:{}", scroll);
                    //  println!("get_max_scroll_num:{:?}", eg.get_max_scroll_num());
                }
                let content2 = chat_eg.get_line(self.chat_tv.get_scroll(), "", is_exact);
                if let Some(c) = content2 {
                    let chat_content = get_chat_content(&c);
                    let chat_tv = Paragraph::new(chat_content)
                        .block(Block::default().title("Chat LLM").borders(Borders::ALL))
                        .style(Style::default().fg(chat_clr_));
                    f.render_widget(chat_tv, self.chat_tv.get_rect());
                } else {
                    let chat_tv = Paragraph::new("")
                        .block(Block::default().title("Chat LLM").borders(Borders::ALL))
                        .style(Style::default().fg(chat_clr_));
                    f.render_widget(chat_tv, self.chat_tv.get_rect());
                }

                // 右侧部分可以显示空白或其他内容
                // let block = Block::default().borders(Borders::ALL).title("LLM Chat");
                let input_box = Paragraph::new(Text::raw(self.chat_inp.get_inp()))
                    .block(Block::default().title("prompt").borders(Borders::ALL))
                    .style(Style::default().fg(chat_inp_clr)); // 设置输入框样式
                f.render_widget(input_box, self.chat_inp.get_rect());
            })?;

            loop {
                tokio::select! {
                    Some(msg) = llm_res_rx.recv() => {
                        chat_txt.push_str(&msg);
                        chat_txt.push_str("\n\n");
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_millis(25)) => {
                    }
                }
                // 监听键盘输入
                if event::poll(Duration::from_millis(25)).unwrap() {
                    if let event::Event::Key(KeyEvent {
                        code, modifiers, ..
                    }) = event::read()?
                    {
                        match (code, modifiers) {
                            (KeyCode::Esc, _) => {
                                self.fuzzy_inp.clear();
                                self.chat_inp.clear();
                                break;
                            }
                            (KeyCode::Tab, _) => {
                                // 按下 Tab 键，切换焦点
                                self.focus.next();
                                break;
                            }
                            (KeyCode::Enter, _) => match self.focus.current() {
                                FocusType::Txt => {
                                    self.fuzzy_inp.clear();
                                    if let Some(c) = &content {
                                        let cur_line = self.navi.get_cur_line();
                                        if cur_line < c.len() {
                                            self.tv.set_scroll(c[cur_line].get_line_num());
                                            self.navi.to_min_line();
                                        }
                                    }
                                    break;
                                }
                                FocusType::ChatInput => {
                                    let message = self.chat_inp.get_inp().to_string();
                                    if message.trim().len() > 0 {
                                        // if let Some(max_line_num) = chat_eg.get_max_scroll_num() {
                                        self.chat_tv.set_scroll(chat_eg.get_last_line().max(1));
                                        // }
                                        chat_txt.push_str(&format!("----------------------------\n{}\n----------------------------\n",message));
                                        let _ = self.prompt_tx.send(message.to_string()).await;
                                        self.chat_inp.clear();
                                        break;
                                    }
                                }
                                _ => {}
                            },
                            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                                // 按下Esc退出
                                self.terminal.clear()?;
                                disable_raw_mode()?;
                                exit(0);
                            }
                            (KeyCode::Down, KeyModifiers::CONTROL) => {
                                // 下一页
                                match self.focus.current() {
                                    FocusType::Txt => {
                                        self.tv.down_page(eg.get_max_scroll_num());
                                        break;
                                    }
                                    FocusType::ChatTxt => {
                                        self.chat_tv.down_page(chat_eg.get_max_scroll_num());
                                        break;
                                    }
                                    _ => {}
                                }

                                break;
                            }
                            (KeyCode::Up, KeyModifiers::CONTROL) => {
                                match self.focus.current() {
                                    FocusType::Txt => {
                                        self.tv.up_page();
                                        break;
                                    }
                                    FocusType::ChatTxt => {
                                        self.chat_tv.up_page();
                                        break;
                                    }
                                    _ => {}
                                }

                                break;
                            }
                            (KeyCode::Up, _) => {
                                // 向上滚动
                                match self.focus.current() {
                                    FocusType::Txt => {
                                        if self.navi.is_top() {
                                            self.tv.up_line();
                                        } else {
                                            self.navi.up_line();
                                        }
                                        break;
                                    }
                                    FocusType::ChatTxt => {
                                        self.chat_tv.up_line();
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            (KeyCode::Down, _) => {
                                // 向下滚动
                                match self.focus.current() {
                                    FocusType::Txt => {
                                        if self.navi.is_bottom() {
                                            self.tv.down_line(eg.get_max_scroll_num());
                                        } else {
                                            self.navi.down_line();
                                        }
                                        break;
                                    }
                                    FocusType::ChatTxt => {
                                        self.chat_tv.down_line(chat_eg.get_max_scroll_num());
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            (KeyCode::Char(c), _) => {
                                match self.focus.current() {
                                    FocusType::FuzzyInput => {
                                        self.fuzzy_inp.push(c); // 添加字符到输入缓冲区
                                        self.tv.set_scroll(1);
                                        break;
                                    }
                                    FocusType::ChatInput => {
                                        self.chat_inp.push(c); // 添加字符到输入缓冲区
                                        break;
                                    }
                                    _ => {}
                                }

                                break;
                            }
                            (KeyCode::Backspace, _) => {
                                match self.focus.current() {
                                    FocusType::FuzzyInput => {
                                        self.fuzzy_inp.pop();
                                        self.tv.set_scroll(1);
                                        break;
                                    }
                                    FocusType::ChatInput => {
                                        self.chat_inp.pop();
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

struct Navigation {
    min_line: usize,
    max_line: usize,
    cur_line: usize,
    rect: Rect,
}

impl Navigation {
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

struct TextView {
    height: usize,
    width: usize,
    scroll: usize,
    rect: Rect,
}

impl TextView {
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
            self.scroll = ((self.scroll + self.height).min(max_scroll_num)).max(1)
        } else {
            self.scroll += self.height;
        }
    }
}

struct FuzzyInput {
    input: String,
    rect: Rect,
}

impl FuzzyInput {
    fn get_rect(&self) -> Rect {
        self.rect
    }

    fn clear(&mut self) {
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

struct ChatInput {
    input: String,
    rect: Rect,
}

impl ChatInput {
    fn get_rect(&self) -> Rect {
        self.rect
    }
    fn clear(&mut self) {
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

fn get_chat_content<'a>(content: &'a Vec<LineTxt<'a>>) -> (Text<'a>) {
    let mut lines = Vec::new();
    for (i, line) in content.into_iter().enumerate() {
        let mut spans = Vec::new();
        let text = line.get_text();
        let mf = line.get_match(); //fuzzy_search(input, text, false);
        if let Some(m) = mf {
            match m {
                Match::Char(_) => {
                    todo!()
                }
                Match::Byte(v) => {
                    let mut current_idx = 0;
                    for bm in v.into_iter() {
                        if current_idx < bm.start && bm.start <= text.len() {
                            spans.push(Span::raw(&text[current_idx..bm.start]));
                        }
                        // 添加高亮文本
                        if bm.start < text.len() && bm.end <= text.len() {
                            spans.push(Span::styled(
                                &text[bm.start..bm.end],
                                Style::default().bg(Color::Green),
                            ));
                        }
                        // 更新当前索引为高亮区间的结束位置
                        current_idx = bm.end;
                    }
                    // 添加剩余的文本（如果有）
                    if current_idx < text.len() {
                        spans.push(Span::raw(&text[current_idx..]));
                    }
                }
            }
        }
        if spans.len() > 0 {
            lines.push(Line::from(spans));
        } else {
            lines.push(Line::from(text));
        }
    }
    let text = Text::from(lines);
    text
}

fn get_content<'a>(
    content: &'a Vec<LineTxt<'a>>,
    cur_line: usize,
    height: usize,
) -> (Text<'a>, Text<'a>) {
    let mut lines = Vec::new();
    for (i, line) in content.into_iter().enumerate() {
        let mut spans = Vec::new();
        let text = line.get_text();
        let mf = line.get_match(); //fuzzy_search(input, text, false);
        if let Some(m) = mf {
            match m {
                Match::Char(_) => {
                    todo!()
                }
                Match::Byte(v) => {
                    let mut current_idx = 0;
                    for bm in v.into_iter() {
                        if current_idx < bm.start && bm.start <= text.len() {
                            spans.push(Span::raw(&text[current_idx..bm.start]));
                        }
                        // 添加高亮文本
                        if bm.start < text.len() && bm.end <= text.len() {
                            spans.push(Span::styled(
                                &text[bm.start..bm.end],
                                Style::default().bg(Color::Green),
                            ));
                        }
                        // 更新当前索引为高亮区间的结束位置
                        current_idx = bm.end;
                    }
                    // 添加剩余的文本（如果有）
                    if current_idx < text.len() {
                        spans.push(Span::raw(&text[current_idx..]));
                    }
                }
            }
        }

        if i == cur_line {
            lines.push(Line::from(Span::styled(
                text,
                Style::default().bg(Color::LightRed), // 设置背景颜色为蓝色
            )));
        } else {
            if spans.len() > 0 {
                lines.push(Line::from(spans));
            } else {
                lines.push(Line::from(text));
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
    fn test_mmap() -> io::Result<()> {
        // let file_path = "/root/start_vpn.sh";
        // let mmap = map_file(file_path)?;
        // let (navi, visible_content, length) = get_visible_content(&mmap, 0, 30, 5, "");
        // println!("{},{}", visible_content, length);
        // Ok(())
        Ok(())
    }
}
