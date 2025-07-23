use crate::command::Command;
use crate::editor::EditLineMeta;
use crate::editor::RingVec;
use crate::editor::TextDisplay;
use crate::editor::TextOper;
use crate::editor::TextWarpType;
use crate::editor::HEX_WITH;
use crate::error::ChapResult;
use crate::ChapTui;
use crossterm::cursor;
use crossterm::execute;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::ExecutableCommand;
use std::io;
use std::path::Path;
use std::process::exit;
pub(crate) enum HandleImpl {
    Edit(HandleEdit),
    Hex(HandleHex),
}

impl Handle for HandleImpl {
    fn handle_ctrl_s<P: AsRef<Path>>(
        &self,
        chap_tui: &mut ChapTui,
        p: P,
        td: &mut TextDisplay,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_ctrl_s(chap_tui, p, td),
            HandleImpl::Hex(h) => h.handle_ctrl_s(chap_tui, p, td),
        }
    }

    fn handle_up(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_up(chap_tui, line_meta, td),
            HandleImpl::Hex(h) => h.handle_up(chap_tui, line_meta, td),
        }
    }

    fn handle_shift_right(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_shift_right(chap_tui, line_meta, td),
            HandleImpl::Hex(h) => h.handle_shift_right(chap_tui, line_meta, td),
        }
    }

    fn handle_shift_left(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_shift_left(chap_tui, line_meta, td),
            HandleImpl::Hex(h) => h.handle_shift_left(chap_tui, line_meta, td),
        }
    }

    fn handle_down<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_down(chap_tui, line_meta, td),
            HandleImpl::Hex(h) => h.handle_down(chap_tui, line_meta, td),
        }
    }

    fn handle_left<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_left(chap_tui, line_meta, td),
            HandleImpl::Hex(h) => h.handle_left(chap_tui, line_meta, td),
        }
    }

    fn handle_right<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_right(chap_tui, line_meta, td),
            HandleImpl::Hex(h) => h.handle_right(chap_tui, line_meta, td),
        }
    }

    fn handle_enter<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_enter(chap_tui, line_meta, td),
            HandleImpl::Hex(h) => h.handle_enter(chap_tui, line_meta, td),
        }
    }

    fn handle_backspace<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_backspace(chap_tui, line_meta, td),
            HandleImpl::Hex(h) => h.handle_backspace(chap_tui, line_meta, td),
        }
    }

    fn handle_char<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
        c: char,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_char(chap_tui, line_meta, td, c),
            HandleImpl::Hex(h) => h.handle_char(chap_tui, line_meta, td, c),
        }
    }
}

pub(crate) trait Handle {
    fn handle_esc(&self, chap_tui: &mut ChapTui) -> ChapResult<()> {
        chap_tui.cmd_inp.clear();
        chap_tui.assist_inp.clear();
        chap_tui.navi.clear();
        chap_tui.txt_sel.reset_to_start();
        Ok(())
    }

    fn handle_ctrl_c(&self, chap_tui: &mut ChapTui) -> ChapResult<()> {
        crossterm::terminal::disable_raw_mode()?;
        execute!(
            chap_tui.terminal.backend_mut(),
            LeaveAlternateScreen // 离开备用屏幕
        )?;
        io::stdout().execute(cursor::Show)?;
        exit(0);
    }

    fn handle_ctrl_s<P: AsRef<Path>>(
        &self,
        chap_tui: &mut ChapTui,
        p: P,
        td: &mut TextDisplay,
    ) -> ChapResult<()>;

    fn handle_up<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_shift_right<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_shift_left(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()>;

    fn handle_down<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_left<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_right<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_enter<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_backspace<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_char<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
        c: char,
    ) -> ChapResult<()>;
}

pub(crate) struct HandleEdit;

impl HandleEdit {
    pub(crate) fn new() -> Self {
        HandleEdit {}
    }
}

impl Handle for HandleEdit {
    fn handle_ctrl_s<P: AsRef<Path>>(
        &self,
        chap_tui: &mut ChapTui,
        p: P,
        td: &mut TextDisplay,
    ) -> ChapResult<()> {
        chap_tui.cmd_inp.clear();
        //保存
        if let Ok(_) = td.save(&p) {
            chap_tui.cmd_inp.push_str("saved");
        } else {
            chap_tui.cmd_inp.push_str("save fail");
        }
        Ok(())
    }

    fn handle_up<'a>(
        &self,
        chap_tui: &mut ChapTui,
        mut line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match chap_tui.warp_type {
            TextWarpType::NoWrap => {
                if chap_tui.cursor_y == 0 {
                    //滚动上一行
                    td.scroll_pre_one_line(line_meta.get(0).unwrap())?;
                    td.get_current_line_meta()?;
                }
                chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
                if chap_tui.cursor_x >= line_meta.get(chap_tui.cursor_y).unwrap().get_char_len() {
                    chap_tui.cursor_x = line_meta.get(chap_tui.cursor_y).unwrap().get_char_len();
                }
                let meta = line_meta.get(chap_tui.cursor_y).unwrap();
                if chap_tui.offset >= meta.get_char_len() {
                    chap_tui.offset = meta.get_char_len();
                }
                chap_tui.is_last_line = false;
            }
            TextWarpType::SoftWrap => {
                if chap_tui.cursor_y == 0 {
                    //滚动上一行
                    td.scroll_pre_one_line(line_meta.get(0).unwrap())?;
                    line_meta = td.get_current_line_meta()?;
                }
                chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
                if chap_tui.cursor_x >= line_meta.get(chap_tui.cursor_y).unwrap().get_char_len() {
                    chap_tui.cursor_x = line_meta.get(chap_tui.cursor_y).unwrap().get_char_len();
                }

                chap_tui.is_last_line = false;
            }
        }
        Ok(())
    }

    fn handle_down<'a>(
        &self,
        chap_tui: &mut ChapTui,
        mut line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match chap_tui.warp_type {
            TextWarpType::NoWrap => {
                if chap_tui.cursor_y < chap_tui.tv.get_height() - 1 {
                    chap_tui.cursor_y += 1;
                } else {
                    //滚动下一行
                    td.scroll_next_one_line(line_meta.last().unwrap())?;
                    line_meta = td.get_current_line_meta()?;
                }
                if chap_tui.cursor_x >= line_meta.get(chap_tui.cursor_y).unwrap().get_char_len() {
                    chap_tui.cursor_x = line_meta.get(chap_tui.cursor_y).unwrap().get_char_len();
                }
                let meta = line_meta.get(chap_tui.cursor_y).unwrap();
                if chap_tui.offset >= meta.get_char_len() {
                    chap_tui.offset = meta.get_char_len();
                }
                chap_tui.is_last_line = false;
            }
            TextWarpType::SoftWrap => {
                if chap_tui.cursor_y < chap_tui.tv.get_height() - 1 {
                    chap_tui.cursor_y += 1;
                } else {
                    //滚动下一行
                    td.scroll_next_one_line(line_meta.last().unwrap())?;
                    line_meta = td.get_current_line_meta()?;
                }
                if chap_tui.cursor_x >= line_meta.get(chap_tui.cursor_y).unwrap().get_char_len() {
                    chap_tui.cursor_x = line_meta.get(chap_tui.cursor_y).unwrap().get_char_len();
                }
                chap_tui.is_last_line = false;
            }
        }
        Ok(())
    }

    fn handle_left<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match chap_tui.warp_type {
            TextWarpType::NoWrap => {
                chap_tui.cursor_x = chap_tui.cursor_x.saturating_sub(1);
                chap_tui.offset = chap_tui.offset.saturating_sub(1);
            }
            TextWarpType::SoftWrap => {
                if chap_tui.cursor_x == 0 {
                    // 这个判断说明当前行已经读完了
                    if line_meta.get(chap_tui.cursor_y).unwrap().get_line_offset() == 0 {
                        //无需操作
                    } else {
                        chap_tui.cursor_x =
                            line_meta.get(chap_tui.cursor_y - 1).unwrap().get_char_len() - 1;
                        chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
                    }
                } else {
                    chap_tui.cursor_x = chap_tui.cursor_x.saturating_sub(1);
                }
                chap_tui.is_last_line = false;
            }
        }
        Ok(())
    }

    fn handle_right<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match chap_tui.warp_type {
            TextWarpType::NoWrap => {
                let meta = line_meta.get(chap_tui.cursor_y).unwrap();
                if chap_tui.cursor_x < meta.get_char_len()
                    && chap_tui.cursor_x < chap_tui.tv.get_width()
                {
                    chap_tui.cursor_x += 1;
                }
                {
                    chap_tui.cursor_x += 1;
                }
                if chap_tui.offset <= meta.get_char_len() {
                    chap_tui.offset += 1;
                }
            }
            TextWarpType::SoftWrap => {
                if chap_tui.cursor_x < line_meta.get(chap_tui.cursor_y).unwrap().get_char_len() {
                    chap_tui.cursor_x += 1;

                    if chap_tui.cursor_x >= line_meta.get(chap_tui.cursor_y).unwrap().get_char_len()
                        && chap_tui.cursor_y < chap_tui.tv.get_height()
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
        Ok(())
    }

    fn handle_enter<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        chap_tui.cmd_inp.clear();
        td.insert_newline(
            chap_tui.cursor_y,
            chap_tui.cursor_x,
            line_meta.get(chap_tui.cursor_y).unwrap(),
        )?;
        if chap_tui.cursor_y < chap_tui.tv.get_height() - 1 {
            chap_tui.cursor_y += 1;
        }
        chap_tui.cursor_x = 0;
        td.get_one_page(chap_tui.start_line_num)?;
        Ok(())
    }

    fn handle_backspace<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        chap_tui.cmd_inp.clear();
        if chap_tui.cursor_y == 0 && chap_tui.cursor_x == 0 {
            return Ok(());
        }
        td.backspace(
            chap_tui.cursor_y,
            chap_tui.cursor_x,
            line_meta.get(chap_tui.cursor_y).unwrap(),
        )?;
        if chap_tui.cursor_x == 0 {
            chap_tui.cursor_x = line_meta.get(chap_tui.cursor_y - 1).unwrap().get_txt_len();
            chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
        } else {
            chap_tui.cursor_x = chap_tui.cursor_x.saturating_sub(1);
        }
        td.get_one_page(chap_tui.start_line_num)?;
        Ok(())
    }

    fn handle_char<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
        c: char,
    ) -> ChapResult<()> {
        chap_tui.cmd_inp.clear();
        if chap_tui.cursor_x == 0 && chap_tui.is_last_line {
            td.insert(
                chap_tui.cursor_y - 1,
                chap_tui.tv.get_width(),
                line_meta.get(chap_tui.cursor_y - 1).unwrap(),
                c,
            )?;
            chap_tui.is_last_line = false;
        } else {
            td.insert(
                chap_tui.cursor_y,
                chap_tui.cursor_x,
                line_meta.get(chap_tui.cursor_y).unwrap(),
                c,
            )?;
        }
        if chap_tui.cursor_x < chap_tui.tv.get_width() {
            chap_tui.cursor_x += 1;
            if chap_tui.cursor_x >= chap_tui.tv.get_width()
                && chap_tui.cursor_y < chap_tui.tv.get_height()
            {
                //不断添加字符 还是续接上一行
                chap_tui.is_last_line = true;
                chap_tui.cursor_x = 0;
                chap_tui.cursor_y += 1;
            }
        }
        td.get_one_page(chap_tui.start_line_num)?;
        Ok(())
    }

    fn handle_shift_right(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_shift_left(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }
}

pub(crate) struct HandleHex;

impl HandleHex {
    pub(crate) fn new() -> Self {
        HandleHex {}
    }
}

impl Handle for HandleHex {
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
        mut line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
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
        mut line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
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
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
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
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
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
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        //todo!("Handle enter in hex mode");
        let cmd_inp = chap_tui.cmd_inp.get_inp();
        let cmd = Command::parse(cmd_inp);
        match cmd {
            Command::Back => {
                if let Some(line_num) = chap_tui.back_linenum.pop() {
                    chap_tui.cursor_y = 0;
                    chap_tui.cursor_x = 0;
                    td.get_one_page(line_num)?;
                }
            }
            Command::SetEndian(endian) => {
                chap_tui.set_endian(endian);
            }
            Command::Jump(addr) => {
                chap_tui
                    .back_linenum
                    .push(line_meta.get(0).unwrap().get_line_num());
                let with = HEX_WITH;
                let line_num = (addr / with) + 1;
                chap_tui.cursor_x = addr % with;
                chap_tui.cursor_y = 0;
                log::debug!("Jump to line number:{}, {}", addr, line_num);
                chap_tui.txt_sel.set_pos(addr);
                td.get_one_page(line_num)?;
            }
            Command::Find(value) => {}
            Command::Unknown(cmd) => {}
        }
        Ok(())
    }

    fn handle_shift_right(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
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
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
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
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        chap_tui.cmd_inp.pop();
        Ok(())
    }

    fn handle_char<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<EditLineMeta>,
        td: &'a TextDisplay,
        c: char,
    ) -> ChapResult<()> {
        if chap_tui.cmd_inp.len() >= 50 {
            return Ok(()); // 限制输入长度为16
        }
        chap_tui.cmd_inp.push(c);
        Ok(())
    }
}

// impl Handle for HandleHex {
//     fn handle_esc(
//         &self,
//         chap_tui: &mut ChapTui,
//         line_meta: &RingVec<EditLineMeta>,
//         td: &TextDisplay,
//     ) {
//         // Implement text mode ESC handling
//     }

//     fn handle_ctrl_c(
//         &self,
//         chap_tui: &mut ChapTui,
//         line_meta: &RingVec<EditLineMeta>,
//         td: &TextDisplay,
//     ) {
//         // Implement text mode Ctrl+C handling
//     }
// }

//struct HandleEdit;

struct HandleVector;
