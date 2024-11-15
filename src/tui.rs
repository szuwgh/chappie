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
use memmap2::Mmap;
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
use std::fs::File;
use std::io;
use std::mem;
use std::process::exit;
pub(crate) struct ChapUI {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    navi: Navigation,
    tv: TextView,
    fuzzy_inp: FuzzyInput,
    chat_tv: ChatText,
    chat_inp: ChatInput,
    focus: Focus,
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
    pub(crate) fn new() -> ChapResult<ChapUI> {
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

        let chat_tv = ChatText {
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
        })
    }

    pub(crate) fn render(&mut self, bytes: &[u8]) -> ChapResult<()> {
        let mut eg = SimpleTextEngine::new(bytes, self.tv.get_height(), self.tv.get_width());
        loop {
            let (inp, is_exact) = self.fuzzy_inp.get_inp_exact();
            let content = eg.get_line(self.tv.get_scroll(), inp, is_exact);
            self.terminal.draw(|f| {
                // 左下输入框区
                let (txt_clr, inp_clr, chat_clr_, chat_inp_clr) = self.focus.get_colors();
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
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title("LLM Chat")
                    .style(Style::default().fg(chat_clr_));
                f.render_widget(block, self.chat_tv.get_rect());
                // 右侧部分可以显示空白或其他内容
                // let block = Block::default().borders(Borders::ALL).title("LLM Chat");
                let input_box = Paragraph::new(Text::raw(self.chat_inp.get_inp()))
                    .block(Block::default().title("prompt").borders(Borders::ALL))
                    .style(Style::default().fg(chat_inp_clr)); // 设置输入框样式
                f.render_widget(input_box, self.chat_inp.get_rect());
            })?;
            loop {
                // 监听键盘输入
                if let event::Event::Key(KeyEvent {
                    code, modifiers, ..
                }) = event::read()?
                {
                    match (code, modifiers) {
                        (KeyCode::Esc, _) => {
                            self.fuzzy_inp.clear();
                            break;
                        } // 按下Esc退出
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
                            FocusType::ChatInput => {}
                            _ => {}
                        },
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            // 下一页
                            // 删除最后一个字符
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
                                        //scroll = (scroll - 1).max(1);
                                    } else {
                                        self.navi.up_line();
                                    }
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
                                _ => {}
                            }

                            break;
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
        Ok(())
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
            self.scroll = (self.scroll + self.height).min(max_scroll_num)
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
    rect: Rect,
}

impl ChatText {
    fn get_rect(&self) -> Rect {
        self.rect
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

// pub(crate) fn show_file() -> io::Result<()> {
//     enable_raw_mode()?;
//     let stdout = std::io::stdout();
//     let backend = CrosstermBackend::new(stdout);
//     let mut terminal = Terminal::new(backend)?;

//     // 清空终端屏幕
//     terminal.clear()?;
//     let file_path = "/opt/rsproject/chappie/vectorbase/src/disk.rs";
//     let mmap = map_file(file_path)?;

//     // 获取终端尺寸
//     let size = terminal.size()?;
//     //终端高度
//     let terminal_height = size.height as usize;
//     let terminal_with = size.width as usize;
//     // 用于控制显示内容的偏移量
//     let heigth = terminal_height - 4;
//     let mut max_line = terminal_height - 6;
//     let min_line = 0;
//     let mut cur_line = max_line;
//     let mut eg = SimpleTextEngine::new(&mmap, heigth, terminal_with / 2 - 1 - 1 - 1);
//     let mut scroll = 1;
//     // 输入内容缓冲区
//     let mut input = String::new();
//     let mut is_exact: bool = true;
//     loop {
//         let inp = if let Some(first_char) = &input.chars().next() {
//             if *first_char == '/' {
//                 is_exact = false;
//                 &input[1..]
//             } else {
//                 is_exact = true;
//                 &input
//             }
//         } else {
//             is_exact = true;
//             &input
//         };
//         let content = eg.get_line(scroll, inp, is_exact);
//         // 在终端显示文件内容
//         terminal.draw(|f| {
//             let chunks = Layout::default()
//                 .direction(Direction::Horizontal)
//                 .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
//                 .split(f.area());

//             //文本框和输入框
//             let left_chunks = Layout::default()
//                 .direction(Direction::Vertical)
//                 .constraints([Constraint::Min(3), Constraint::Length(3)].as_ref())
//                 .split(chunks[0]); // chunks[1] 是左侧区域

//             let nav_text_chunks = Layout::default()
//                 .direction(Direction::Horizontal)
//                 .constraints([Constraint::Length(1), Constraint::Percentage(100)].as_ref())
//                 .split(left_chunks[0]); // chunks[1] 是左侧区域

//             let nav = Layout::default()
//                 .direction(Direction::Vertical)
//                 .constraints([Constraint::Length(1), Constraint::Percentage(100)].as_ref())
//                 .split(nav_text_chunks[0]); // chunks[1] 是左侧区域

//             // 左下输入框区
//             let input_box = Paragraph::new(Text::raw(&input))
//                 .block(Block::default().title("Input1").borders(Borders::ALL))
//                 .style(Style::default().fg(Color::Green)); // 设置输入框样式
//             f.render_widget(input_box, left_chunks[1]);

//             let block = Block::default().borders(Borders::ALL).title("File Content");
//             if let Some(c) = &content {
//                 let (navi, visible_content) = get_content(c, cur_line, heigth);
//                 let text_para = Paragraph::new(visible_content).block(block);
//                 f.render_widget(text_para, nav_text_chunks[1]);
//                 let nav_paragraph = Paragraph::new(navi);
//                 f.render_widget(nav_paragraph, nav[1]);
//                 // max_line = c.len();
//             } else {
//                 // println!("scorll:{}", scroll);
//                 //  println!("get_max_scroll_num:{:?}", eg.get_max_scroll_num());
//             }

//             // 右侧部分可以显示空白或其他内容
//             let block = Block::default().borders(Borders::ALL).title("LLM Chat");
//             f.render_widget(block, chunks[1]);

//             let input_box = Paragraph::new(Text::raw(""))
//                 .block(Block::default().title("Input1").borders(Borders::ALL))
//                 .style(Style::default().fg(Color::Green)); // 设置输入框样式
//             f.render_widget(input_box, left_chunks[1]);
//         })?;

//         loop {
//             // 监听键盘输入
//             if let event::Event::Key(KeyEvent {
//                 code, modifiers, ..
//             }) = event::read()?
//             {
//                 match (code, modifiers) {
//                     (KeyCode::Esc, _) => {
//                         input.clear();
//                         break;
//                     } // 按下Esc退出
//                     (KeyCode::Enter, _) => {
//                         input.clear();
//                         if let Some(c) = &content {
//                             if cur_line < c.len() {
//                                 scroll = c[cur_line].get_line_num();
//                                 cur_line = min_line;
//                             }
//                         }
//                         break;
//                     }
//                     (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
//                         // 下一页
//                         // 删除最后一个字符
//                         terminal.clear()?;
//                         disable_raw_mode()?;
//                         exit(0);
//                     }
//                     (KeyCode::Down, KeyModifiers::CONTROL) => {
//                         // 下一页
//                         // 删除最后一个字符
//                         if let Some(max_scroll_num) = eg.get_max_scroll_num() {
//                             scroll = (scroll + heigth).min(max_scroll_num)
//                         } else {
//                             scroll = scroll + heigth;
//                         }
//                         break;
//                     }
//                     (KeyCode::Up, KeyModifiers::CONTROL) => {
//                         if scroll > heigth {
//                             scroll = (scroll - heigth).max(1);
//                         } else {
//                             scroll = 1;
//                         }
//                         break;
//                     }
//                     (KeyCode::Up, _) => {
//                         // 向上滚动
//                         if cur_line == min_line {
//                             scroll = (scroll - 1).max(1);
//                         } else if cur_line > min_line {
//                             cur_line = cur_line - 1;
//                         }
//                         break;
//                     }
//                     (KeyCode::Down, _) => {
//                         // 向下滚动
//                         if cur_line == max_line {
//                             if let Some(max_scroll_num) = eg.get_max_scroll_num() {
//                                 if scroll <= max_scroll_num {
//                                     scroll = scroll + 1;
//                                 }
//                             } else {
//                                 scroll = scroll + 1;
//                             }
//                         } else if cur_line < max_line {
//                             cur_line = cur_line + 1;
//                         }
//                         break;
//                     }
//                     (KeyCode::Char(c), _) => {
//                         input.push(c); // 添加字符到输入缓冲区
//                         scroll = 1;
//                         break;
//                     }
//                     (KeyCode::Backspace, _) => {
//                         input.pop(); // 删除最后一个字符
//                         scroll = 1;
//                         break;
//                     }

//                     _ => {}
//                 }
//             }
//         }
//     }

//     Ok(())
// }

// // 使用 mmap 映射文件到内存
// fn map_file(path: &str) -> io::Result<Mmap> {
//     let file = File::open(path)?;
//     let mmap = unsafe { Mmap::map(&file)? };
//     Ok(mmap)
// }

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
                Style::default().bg(Color::Yellow), // 设置背景颜色为蓝色
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
                    Line::from(Span::styled(">", Style::default().fg(Color::Yellow)))
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
