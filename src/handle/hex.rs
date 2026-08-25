use crossterm::cursor;

use crate::command::Command;
use crate::command::Value;
use crate::common::error::ChapResult;
use crate::common::ring_vec::RingVec;
use crate::handle::Handle;
use crate::plugin::Plugin;
use crate::textwarp::LineState;
use crate::textwarp::TextDisplay;
use crate::textwarp::TextOper;
use crate::textwarp::TextSelect;
use crate::ChapTui;

use std::fs::File;

use std::io::Write;
use std::path::Path;

pub(crate) struct HandleHex<T: Plugin> {
    plugin: T,
}

impl<T: Plugin> HandleHex<T> {
    pub(crate) fn new(plugin: T) -> Self {
        HandleHex { plugin }
    }

    fn jump_to_address(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<LineState>,
        addr: usize,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        // if line_meta.is_empty() {
        //     return Ok(());
        // }
        // let addr = addr.min(td.get_file_size() - 1);
        // chap_tui
        //     .back_linenum
        //     .push(line_meta.get(0).unwrap().get_line_num());
        // let with = HEX_WITH;
        // let line_num = (addr / with) + 1;
        // chap_tui.cursor_x = addr % with;
        // chap_tui.cursor_y = 0;
        // chap_tui.txt_sel.set_pos(addr);
        // td.get_one_page(line_num)?;
        Ok(())
    }

    fn find_jump(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<LineState>,
        td: &TextDisplay,
        seek_start: usize,
        pattern: &[u8],
    ) -> ChapResult<()> {
        if pattern.is_empty() {
            return Ok(());
        }
        let seek_start = seek_start + pattern.len();
        if let Some(addr) = td.find(pattern, seek_start) {
            self.jump_to_address(chap_tui, line_meta, addr + seek_start, td)?;
            chap_tui
                .txt_sel
                .set_select(addr + seek_start, addr + seek_start + pattern.len() - 1);
        }
        Ok(())
    }
}

impl<T: Plugin> Handle for HandleHex<T> {
    fn handle_ctrl_s<P: AsRef<Path>>(
        &self,
        chap_tui: &mut ChapTui,
        p: P,
        td: &mut TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_up<'a>(
        &self,
        chap_tui: &mut ChapTui,
        mut line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        if line_meta.is_empty() {
            return Ok(());
        }
        if chap_tui.cursor_y == 0 {
            //滚动上一行
            td.scroll_pre_one_line(line_meta.get(0).unwrap())?;
            line_meta = td.get_current_line_meta()?;
        }
        chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
        if chap_tui.cursor_x
            >= line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_txt_len()
                .saturating_sub(1)
        {
            chap_tui.cursor_x = line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_txt_len()
                .saturating_sub(1);
        };

        chap_tui.txt_sel.set_pos(
            line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_line_file_start()
                + chap_tui.cursor_x,
        );
        Ok(())
    }

    fn handle_down<'a>(
        &self,
        chap_tui: &mut ChapTui,
        mut line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        if line_meta.is_empty() {
            return Ok(());
        }
        if chap_tui.cursor_y < line_meta.len().saturating_sub(1) {
            chap_tui.cursor_y += 1;
        } else {
            //滚动下一行
            td.scroll_next_one_line(line_meta.last().unwrap())?;
            line_meta = td.get_current_line_meta()?;
        }
        if chap_tui.cursor_x
            >= line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_txt_len()
                .saturating_sub(1)
        {
            chap_tui.cursor_x = line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_txt_len()
                .saturating_sub(1);
        };

        chap_tui.txt_sel.set_pos(
            line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_line_file_start()
                + chap_tui.cursor_x,
        );
        Ok(())
    }

    fn handle_left<'a>(
        &self,
        chap_tui: &mut ChapTui,
        mut line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        if line_meta.is_empty() {
            return Ok(());
        }
        if chap_tui.cursor_x == 0 {
            // 这个判断说明当前行已经读完了
            if line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_line_file_start()
                == 0
            {
                //无需操作
            } else {
                if chap_tui.cursor_y == 0 {
                    //滚动上一行
                    td.scroll_pre_one_line(line_meta.get(0).unwrap())?;
                    line_meta = td.get_current_line_meta()?;
                    chap_tui.cursor_x = line_meta
                        .get(chap_tui.cursor_y)
                        .unwrap()
                        .get_txt_len()
                        .saturating_sub(1);
                } else {
                    chap_tui.cursor_x = line_meta
                        .get(chap_tui.cursor_y - 1)
                        .unwrap()
                        .get_txt_len()
                        .saturating_sub(1);
                    chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
                }
            }
        } else {
            chap_tui.cursor_x = chap_tui.cursor_x.saturating_sub(1);
        }

        chap_tui.txt_sel.set_pos(
            line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_line_file_start()
                + chap_tui.cursor_x,
        );
        Ok(())
    }

    fn handle_right<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        if line_meta.is_empty() {
            return Ok(());
        }
        if chap_tui.cursor_x
            < line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_txt_len()
                .saturating_sub(1)
        {
            chap_tui.cursor_x += 1;
        } else {
            chap_tui.cursor_x = 0;
            if chap_tui.cursor_y < line_meta.len().saturating_sub(1) {
                chap_tui.cursor_y += 1;
            }
        }
        chap_tui.txt_sel.set_pos(
            line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_line_file_start()
                + chap_tui.cursor_x,
        );
        Ok(())
    }

    fn handle_enter<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        let cmd_inp = chap_tui.elem.cmd_inp.get_inp();
        let cmd = Command::parse(cmd_inp);
        match cmd {
            Command::Empty => {}
            Command::Back => {
                // if let Some(line_num) = chap_tui.back_linenum.pop() {
                //     chap_tui.cursor_y = 0;
                //     chap_tui.cursor_x = 0;
                //     td.get_one_page(line_num)?;
                // }
            }
            Command::GTop => {
                self.jump_to_address(chap_tui, line_meta, 0, td)?;
            }
            Command::GBottom => {
                self.jump_to_address(chap_tui, line_meta, td.get_file_size(), td)?
            }
            Command::SetEndian(endian) => {
                chap_tui.set_endian(endian);
            }
            Command::Jump(addr) => {
                self.jump_to_address(chap_tui, line_meta, addr, td)?;
            }
            Command::Find(value) => {
                if line_meta.is_empty() {
                    return Ok(());
                }
                let seek_start = line_meta
                    .get(chap_tui.cursor_y)
                    .unwrap()
                    .get_line_file_start()
                    + chap_tui.cursor_x;
                match value {
                    Value::Hex(pattern) => {
                        self.find_jump(chap_tui, line_meta, td, seek_start, pattern.as_slice())?;
                    }
                    Value::Ascii(pattern) => {
                        self.find_jump(chap_tui, line_meta, td, seek_start, pattern.as_bytes())?;
                    }
                }
            }
            Command::Search(_) | Command::Fuzzy(_) => {}
            Command::Cut(c) => {
                let seek_start = line_meta
                    .get(chap_tui.cursor_y)
                    .unwrap()
                    .get_line_file_start()
                    + chap_tui.cursor_x;
                let bytes = td.get_text_from_sel(&TextSelect::from_select(
                    seek_start,
                    seek_start + c.get_count(),
                ));
                // 新建一个文件 把bytes 保存到文件
                let mut file = File::create(c.get_filepath())?;
                // 写入字节数组
                chap_tui.elem.cmd_inp.clear();
                if let Ok(_) = file.write_all(&bytes) {
                    chap_tui.elem.cmd_inp.push_str("save file success");
                } else {
                    chap_tui.elem.cmd_inp.push_str("save file failed");
                }
            }

            Command::CutSel(c) => {
                let bytes =
                    td.get_text_from_sel(&TextSelect::from_select(c.get_start(), c.get_end()));
                // 新建一个文件 把bytes 保存到文件
                let mut file = File::create(c.get_filepath())?;
                // 写入字节数组
                chap_tui.elem.cmd_inp.clear();
                if let Ok(_) = file.write_all(&bytes) {
                    chap_tui.elem.cmd_inp.push_str("save file success");
                } else {
                    chap_tui.elem.cmd_inp.push_str("save file failed");
                }
            }
            Command::Call(function) => {
                let b = td.get_text_from_sel(&chap_tui.txt_sel);
                // let a = function.call(ByteView::new(b, chap_tui.endian.clone()));
                let a = self.plugin.eval(&function, &b)?;
                chap_tui.assist_tv2_data = a;
            }
            Command::ListFunc => {
                chap_tui.assist_tv2_data = self.plugin.list()?;
            }
            Command::HexInput(x) => {
                let cursor_x = chap_tui.cursor_x;
                let cursor_y = chap_tui.cursor_y;
                log::debug!(
                    "Handle hex char input: {:02x?}, cursor_x: {}, cursor_y: {}",
                    x,
                    cursor_x,
                    cursor_y
                );
                td.insert_bytes(
                    cursor_y,
                    cursor_x,
                    line_meta.get(cursor_y).unwrap(),
                    &x,
                    true,
                )?;
                td.get_one_page_from_state(&chap_tui.start_line_state)?;
            }
            Command::Insert(count) => {
                let cursor_x = chap_tui.cursor_x;
                let cursor_y = chap_tui.cursor_y;
                td.insert_bytes(
                    cursor_y,
                    cursor_x,
                    line_meta.get(cursor_y).unwrap(),
                    &vec![0u8; count],
                    false,
                )?;
                td.get_one_page_from_state(&chap_tui.start_line_state)?;
            }
            Command::Unknown(cmd) => {}
        }

        Ok(())
    }

    fn handle_shift_up<'a>(
        &self,
        chap_tui: &mut ChapTui,
        mut line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        if line_meta.is_empty() {
            return Ok(());
        }
        if chap_tui.cursor_y == 0 {
            //滚动上一行
            td.scroll_pre_one_line(line_meta.get(0).unwrap())?;
            line_meta = td.get_current_line_meta()?;
        }
        chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
        if chap_tui.cursor_x
            >= line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_txt_len()
                .saturating_sub(1)
        {
            chap_tui.cursor_x = line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_txt_len()
                .saturating_sub(1);
        };

        // chap_tui.txt_sel.set_pos(
        //     line_meta
        //         .get(chap_tui.cursor_y)
        //         .unwrap()
        //         .get_line_file_start()
        //         + chap_tui.cursor_x,
        // );

        let pos = line_meta
            .get(chap_tui.cursor_y)
            .unwrap()
            .get_line_file_start()
            + chap_tui.cursor_x;
        if pos < chap_tui.txt_sel.get_start() {
            chap_tui.txt_sel.set_start(pos);
        } else {
            chap_tui.txt_sel.set_end(pos);
        }
        Ok(())
    }

    fn handle_shift_down<'a>(
        &self,
        chap_tui: &mut ChapTui,
        mut line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        if line_meta.is_empty() {
            return Ok(());
        }
        if chap_tui.cursor_y < line_meta.len().saturating_sub(1) {
            chap_tui.cursor_y += 1;
        } else {
            //滚动下一行
            td.scroll_next_one_line(line_meta.last().unwrap())?;
            line_meta = td.get_current_line_meta()?;
        }
        if chap_tui.cursor_x
            >= line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_txt_len()
                .saturating_sub(1)
        {
            chap_tui.cursor_x = line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_txt_len()
                .saturating_sub(1);
        };

        let pos = line_meta
            .get(chap_tui.cursor_y)
            .unwrap()
            .get_line_file_start()
            + chap_tui.cursor_x;
        if pos > chap_tui.txt_sel.get_end() {
            chap_tui.txt_sel.set_end(pos);
        } else {
            chap_tui.txt_sel.set_start(pos);
        }
        Ok(())
    }

    fn handle_shift_right(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<LineState>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        if line_meta.is_empty() {
            return Ok(());
        }
        if chap_tui.cursor_x
            < line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_txt_len()
                .saturating_sub(1)
        {
            chap_tui.cursor_x += 1;
        } else {
            chap_tui.cursor_x = 0;
            if chap_tui.cursor_y < line_meta.len().saturating_sub(1) {
                chap_tui.cursor_y += 1;
            }
        }

        let pos = line_meta
            .get(chap_tui.cursor_y)
            .unwrap()
            .get_line_file_start()
            + chap_tui.cursor_x;
        if pos > chap_tui.txt_sel.get_end() {
            chap_tui.txt_sel.set_end(pos);
        } else {
            chap_tui.txt_sel.set_start(pos);
        }
        Ok(())
    }

    fn handle_shift_left(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<LineState>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        if line_meta.is_empty() {
            return Ok(());
        }
        if chap_tui.cursor_x == 0 {
            // 这个判断说明当前行已经读完了
            if line_meta
                .get(chap_tui.cursor_y)
                .unwrap()
                .get_line_file_start()
                == 0
            {
                //无需操作
                return Ok(());
            } else {
                chap_tui.cursor_x = line_meta
                    .get(chap_tui.cursor_y - 1)
                    .unwrap()
                    .get_txt_len()
                    .saturating_sub(1);
                chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
            }
        } else {
            chap_tui.cursor_x = chap_tui.cursor_x.saturating_sub(1);
        }

        let pos = line_meta
            .get(chap_tui.cursor_y)
            .unwrap()
            .get_line_file_start()
            + chap_tui.cursor_x;
        if pos < chap_tui.txt_sel.get_start() {
            chap_tui.txt_sel.set_start(pos);
        } else {
            chap_tui.txt_sel.set_end(pos);
        }

        Ok(())
    }

    fn handle_backspace<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        chap_tui.elem.cmd_inp.pop();
        Ok(())
    }

    fn handle_char<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
        c: char,
    ) -> ChapResult<()> {
        if chap_tui.elem.cmd_inp.len() >= 50 {
            return Ok(()); // 限制输入长度为16
        }
        chap_tui.elem.cmd_inp.push(c);
        Ok(())
    }

    fn handle_paste<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
        pasted_string: &str,
    ) -> ChapResult<()> {
        todo!("Handle paste in hex mode");
    }
}

// impl Handle for HandleHex {
//     fn handle_esc(
//         &self,
//         chap_tui: &mut ChapTui,
//         line_meta: &RingVec<LineState>,
//         td: &TextDisplay,
//     ) {
//         // Implement text mode ESC handling
//     }

//     fn handle_ctrl_c(
//         &self,
//         chap_tui: &mut ChapTui,
//         line_meta: &RingVec<LineState>,
//         td: &TextDisplay,
//     ) {
//         // Implement text mode Ctrl+C handling
//     }
// }

//struct HandleEdit;
