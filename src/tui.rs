use crate::text::LineTxt;
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
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Terminal;

use std::fs::File;

use std::io::{self, BufReader, Read, Write};
use std::process::exit;

use crate::fuzzy::fuzzy_search;
use crate::fuzzy::Match;
use crate::text::SimpleTextEngine;
struct ChapUI {}

impl ChapUI {
    fn new() {}
}
const SCROLL_STEP: usize = 1; // 每次滚动的行数
const LINES_PER_PAGE: usize = 20; // 每页显示多少行

pub(crate) fn show_file() -> io::Result<()> {
    enable_raw_mode()?;
    let stdout = std::io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 清空终端屏幕
    terminal.clear()?;
    let file_path = "/opt/rsproject/chappie/vectorbase/src/disk.rs";
    let mmap = map_file(file_path)?;

    // 获取终端尺寸
    let size = terminal.size()?;
    //终端高度
    let terminal_height = size.height as usize;
    let terminal_with = size.width as usize;
    // 用于控制显示内容的偏移量
    let heigth = terminal_height - 4;
    let mut max_line = terminal_height - 6;
    let min_line = 0;
    let mut cur_line = max_line;
    let mut eg = SimpleTextEngine::new(&mmap, heigth, terminal_with / 2 - 1 - 1 - 1);
    let mut scroll = 1;
    // 输入内容缓冲区
    let mut input = String::new();
    loop {
        let content = eg.get_line(scroll, &input);
        // 在终端显示文件内容
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(f.area());

            //文本框和输入框
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)].as_ref())
                .split(chunks[0]); // chunks[1] 是左侧区域

            let nav_text_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(1), Constraint::Percentage(100)].as_ref())
                .split(left_chunks[0]); // chunks[1] 是左侧区域

            let nav = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Percentage(100)].as_ref())
                .split(nav_text_chunks[0]); // chunks[1] 是左侧区域

            // 左下输入框区
            let input_box = Paragraph::new(Text::raw(&input))
                .block(Block::default().title("Input1").borders(Borders::ALL))
                .style(Style::default().fg(Color::Green)); // 设置输入框样式
            f.render_widget(input_box, left_chunks[1]);

            let block = Block::default().borders(Borders::ALL).title("File Content");
            if let Some(c) = &content {
                let (navi, visible_content) = get_content(c, &input, cur_line, heigth);
                let text_para = Paragraph::new(visible_content).block(block);
                f.render_widget(text_para, nav_text_chunks[1]);
                let nav_paragraph = Paragraph::new(navi);
                f.render_widget(nav_paragraph, nav[1]);
                // max_line = c.len();
            } else {
                // println!("scorll:{}", scroll);
                //  println!("get_max_scroll_num:{:?}", eg.get_max_scroll_num());
            }

            // 右侧部分可以显示空白或其他内容
            let block = Block::default().borders(Borders::ALL).title("LLM Chat");
            f.render_widget(block, chunks[1]);
        })?;

        // 监听键盘输入
        if let event::Event::Key(KeyEvent {
            code, modifiers, ..
        }) = event::read()?
        {
            match (code, modifiers) {
                (KeyCode::Esc, _) => break, // 按下Esc退出
                (KeyCode::Enter, _) => {
                    input.clear();
                    if let Some(c) = &content {
                        if cur_line < c.len() {
                            scroll = c[cur_line].get_line_num();
                            cur_line = min_line;
                        }
                    }
                }
                (KeyCode::Down, KeyModifiers::CONTROL) => {
                    // 下一页
                    // 删除最后一个字符
                    if let Some(max_scroll_num) = eg.get_max_scroll_num() {
                        scroll = (scroll + heigth).min(max_scroll_num)
                    } else {
                        scroll = scroll + heigth;
                    }
                }
                (KeyCode::Up, KeyModifiers::CONTROL) => {
                    if scroll > heigth {
                        scroll = (scroll - heigth).max(1);
                    } else {
                        scroll = 1;
                    }
                }
                (KeyCode::Up, _) => {
                    // 向上滚动
                    if cur_line == min_line {
                        scroll = (scroll - 1).max(1);
                    } else if cur_line > min_line {
                        cur_line = cur_line - 1;
                    }
                }
                (KeyCode::Down, _) => {
                    // 向下滚动
                    if cur_line == max_line {
                        if let Some(max_scroll_num) = eg.get_max_scroll_num() {
                            if scroll <= max_scroll_num {
                                scroll = scroll + 1;
                            }
                        } else {
                            scroll = scroll + 1;
                        }
                    } else if cur_line < max_line {
                        cur_line = cur_line + 1;
                    }
                }
                (KeyCode::Char(c), _) => {
                    input.push(c); // 添加字符到输入缓冲区
                }
                (KeyCode::Backspace, _) => {
                    input.pop(); // 删除最后一个字符
                }

                _ => {}
            }
        }
    }
    terminal.clear()?;
    disable_raw_mode()?;
    Ok(())
}

// 使用 mmap 映射文件到内存
fn map_file(path: &str) -> io::Result<Mmap> {
    let file = File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok(mmap)
}

fn get_content<'a>(
    content: &'a Vec<LineTxt<'a>>,
    input: &str,
    cur_line: usize,
    height: usize,
) -> (Text<'a>, Text<'a>) {
    let mut lines = Vec::new();
    for (i, line) in content.into_iter().enumerate() {
        let mut spans = Vec::new();
        let text = line.get_text();
        if input.len() >= 2 {
            let m = fuzzy_search(input, text, false);
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
fn get_visible_content<'a>(
    mmap: &'a [u8],
    offset: usize,
    terminal_height: usize,
    terminal_width: usize,
    cur_line: usize,
    input: &str,
) -> (Text<'a>, Text<'a>, bool) {
    let lines_iter = mmap.split(|&byte| byte == b'\n');
    let mut lines = Vec::new();

    // 设置游标
    let start = offset;
    let mut skip_iter = lines_iter.skip(start).peekable();
    // 迭代器按行读取文件内容
    for (i, line) in skip_iter.by_ref().enumerate() {
        if i >= terminal_height - 2 - 3 {
            break;
        }
        let text = std::str::from_utf8(line).unwrap();
        let mut spans = Vec::new();
        if input.len() >= 2 {
            let m = fuzzy_search(input, text, false);
            match m {
                Match::Char(_) => {
                    todo!()
                }
                Match::Byte(v) => {
                    let mut current_idx = 0;
                    for bm in v.into_iter() {
                        if current_idx < bm.start && bm.start <= line.len() {
                            spans.push(Span::raw(&text[current_idx..bm.start]));
                        }
                        // 添加高亮文本
                        if bm.start < line.len() && bm.end <= line.len() {
                            spans.push(Span::styled(
                                &text[bm.start..bm.end],
                                Style::default().bg(Color::Green),
                            ));
                        }
                        // 更新当前索引为高亮区间的结束位置
                        current_idx = bm.end;
                    }
                    // 添加剩余的文本（如果有）
                    if current_idx < line.len() {
                        spans.push(Span::raw(
                            std::str::from_utf8(&line[current_idx..]).unwrap(),
                        ));
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
    // 判断是否有剩余行
    let is_left = if skip_iter.peek().is_some() {
        true
    } else {
        false
    };
    let nav_text = Text::from(
        lines
            .iter()
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
    (nav_text, text, is_left)
}

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
