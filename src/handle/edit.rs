use crate::common::error::ChapResult;
use crate::common::ring_vec::RingVec;
use crate::handle::Handle;
use crate::textwarp::LineState;
use crate::textwarp::TextDisplay;
use crate::textwarp::TextOper;
use crate::textwarp::TextWarpType;
use crate::ChapTui;
use std::path::Path;
use unicode_width::UnicodeWidthChar;

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
        chap_tui.elem.cmd_inp.clear();
        //保存
        if let Ok(_) = td.save(&p) {
            chap_tui.elem.cmd_inp.push_str("saved");
        } else {
            chap_tui.elem.cmd_inp.push_str("save fail");
        }
        Ok(())
    }

    fn handle_up<'a>(
        &self,
        chap_tui: &mut ChapTui,
        mut line_meta: &'a RingVec<LineState>,
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
                if chap_tui.column_offset >= meta.get_char_len() {
                    chap_tui.column_offset = meta.get_char_len();
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
                if chap_tui.cursor_x
                    >= line_meta
                        .get(chap_tui.cursor_y)
                        .unwrap()
                        .get_char_len()
                        .saturating_sub(1)
                {
                    chap_tui.cursor_x = line_meta
                        .get(chap_tui.cursor_y)
                        .unwrap()
                        .get_char_len()
                        .saturating_sub(1);
                }

                chap_tui.is_last_line = false;
            }
        }
        Ok(())
    }

    fn handle_down<'a>(
        &self,
        chap_tui: &mut ChapTui,
        mut line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match chap_tui.warp_type {
            TextWarpType::NoWrap => {
                if chap_tui.cursor_y < chap_tui.elem.tv.get_height() - 1 {
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
                        .get_char_len()
                        .saturating_sub(1)
                {
                    chap_tui.cursor_x = line_meta
                        .get(chap_tui.cursor_y)
                        .unwrap()
                        .get_char_len()
                        .saturating_sub(1);
                }
                let meta = line_meta.get(chap_tui.cursor_y).unwrap();
                if chap_tui.column_offset >= meta.get_char_len() {
                    chap_tui.column_offset = meta.get_char_len();
                }
                chap_tui.is_last_line = false;
            }
            TextWarpType::SoftWrap => {
                if chap_tui.cursor_y < chap_tui.elem.tv.get_height().saturating_sub(1) {
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
                        .get_char_len()
                        .saturating_sub(1)
                {
                    chap_tui.cursor_x = line_meta
                        .get(chap_tui.cursor_y)
                        .unwrap()
                        .get_char_len()
                        .saturating_sub(1);
                }
                chap_tui.is_last_line = false;
            }
        }
        Ok(())
    }

    fn handle_left<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match chap_tui.warp_type {
            TextWarpType::NoWrap => {
                chap_tui.cursor_x = chap_tui.cursor_x.saturating_sub(1);
                chap_tui.column_offset = chap_tui.column_offset.saturating_sub(1);
            }
            TextWarpType::SoftWrap => {
                if chap_tui.cursor_x == 0 {
                    // 这个判断说明当前行已经读完了
                    if line_meta.get(chap_tui.cursor_y).unwrap().get_line_offset() == 0 {
                        //无需操作
                    } else {
                        chap_tui.cursor_x = line_meta
                            .get(chap_tui.cursor_y - 1)
                            .unwrap()
                            .get_char_len()
                            .saturating_sub(1);
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
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match chap_tui.warp_type {
            TextWarpType::NoWrap => {
                let meta = line_meta.get(chap_tui.cursor_y).unwrap();
                if chap_tui.cursor_x < meta.get_char_len()
                    && chap_tui.cursor_x < chap_tui.elem.tv.get_width()
                {
                    chap_tui.cursor_x += 1;
                }
                {
                    chap_tui.cursor_x += 1;
                }
                if chap_tui.column_offset <= meta.get_char_len() {
                    chap_tui.column_offset += 1;
                }
            }
            TextWarpType::SoftWrap => {
                if chap_tui.cursor_x
                    < line_meta
                        .get(chap_tui.cursor_y)
                        .unwrap()
                        .get_char_len()
                        .saturating_sub(1)
                {
                    chap_tui.cursor_x += 1;

                    if chap_tui.cursor_x >= line_meta.get(chap_tui.cursor_y).unwrap().get_char_len()
                        && chap_tui.cursor_y < chap_tui.elem.tv.get_height()
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
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        chap_tui.elem.cmd_inp.clear();
        td.insert_newline(
            chap_tui.cursor_y,
            chap_tui.bytes_cursor,
            line_meta.get(chap_tui.cursor_y).unwrap(),
        )?;
        if chap_tui.cursor_y < chap_tui.elem.tv.get_height() - 1 {
            chap_tui.cursor_y += 1;
        }
        chap_tui.cursor_x = 0;
        td.get_one_page(chap_tui.start_line_num)?;
        Ok(())
    }

    fn handle_backspace<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        chap_tui.elem.cmd_inp.clear();
        if chap_tui.cursor_y == 0 && chap_tui.cursor_x == 0 {
            return Ok(());
        }
        let prev_line_char_len = if chap_tui.cursor_y == 0 {
            0
        } else {
            line_meta
                .get(chap_tui.cursor_y - 1)
                .unwrap()
                .get_char_len()
                .saturating_sub(1)
        };
        td.backspace(
            chap_tui.cursor_y,
            chap_tui.bytes_cursor,
            chap_tui.bytes_cursor_size,
            line_meta.get(chap_tui.cursor_y).unwrap(),
        )?;
        td.get_one_page(chap_tui.start_line_num)?;
        if chap_tui.cursor_x == 0 {
            let cursor_y = chap_tui.cursor_y.saturating_sub(1);
            if prev_line_char_len > 0 {
                chap_tui.cursor_x = prev_line_char_len;
            } else {
                chap_tui.cursor_x = 0;
            }
            chap_tui.cursor_y = cursor_y;
        } else {
            chap_tui.cursor_x = chap_tui.cursor_x.saturating_sub(1);
        }

        Ok(())
    }

    fn handle_shift_down<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_shift_up<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_shift_right(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<LineState>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_shift_left(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<LineState>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_char<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
        c: char,
    ) -> ChapResult<()> {
        chap_tui.elem.cmd_inp.clear();
        if chap_tui.cursor_x == 0 && chap_tui.is_last_line {
            td.insert_char(
                chap_tui.cursor_y - 1,
                chap_tui.elem.tv.get_width(),
                line_meta.get(chap_tui.cursor_y - 1).unwrap(),
                c,
            )?;
            chap_tui.is_last_line = false;
        } else {
            td.insert_char(
                chap_tui.cursor_y,
                chap_tui.bytes_cursor,
                line_meta.get(chap_tui.cursor_y).unwrap(),
                c,
            )?;
        }
        if chap_tui.cursor_x < chap_tui.elem.tv.get_width() {
            chap_tui.cursor_x += 1;
            if chap_tui.cursor_x >= chap_tui.elem.tv.get_width()
                && chap_tui.cursor_y < chap_tui.elem.tv.get_height()
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

    fn handle_paste<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
        pasted_string: &str,
    ) -> ChapResult<()> {
        chap_tui.elem.cmd_inp.clear();
        let mut char_with = 0;
        if chap_tui.cursor_x == 0 && chap_tui.is_last_line {
            let line_state = line_meta.get(chap_tui.cursor_y - 1).unwrap();
            char_with = line_state.char_with;
            td.insert_bytes(
                chap_tui.cursor_y - 1,
                chap_tui.elem.tv.get_width(),
                line_state,
                pasted_string.as_bytes(),
            )?;
            chap_tui.is_last_line = false;
        } else {
            let line_state = line_meta.get(chap_tui.cursor_y).unwrap();
            char_with = line_state.char_with;
            td.insert_bytes(
                chap_tui.cursor_y,
                chap_tui.bytes_cursor,
                line_state,
                pasted_string.as_bytes(),
            )?;
        }
        log::debug!(
            "cursor_x before paste: {}, cursor_y: {}, tv width: {},chap_tui.bytes_cursor:{}",
            chap_tui.cursor_x,
            chap_tui.cursor_y,
            chap_tui.elem.tv.get_width(),
            chap_tui.bytes_cursor
        );

        // 更新光标位置
        for x in pasted_string.chars() {
            let x_width = x.width().unwrap_or(0);
            if x == '\n' {
                chap_tui.cursor_x = 0;
                chap_tui.cursor_y += 1;
            } else {
                char_with += x_width;
                if char_with > chap_tui.elem.tv.get_width() {
                    if chap_tui.cursor_y < chap_tui.elem.tv.get_height() {
                        //不断添加字符 还是续接上一行
                        chap_tui.cursor_x = 1;
                        char_with = x_width;
                        chap_tui.cursor_y += 1;
                    }
                } else {
                    chap_tui.cursor_x += 1;
                }
            }
        }
        log::debug!(
            "After paste, cursor_x: {}, cursor_y: {}",
            chap_tui.cursor_x,
            chap_tui.cursor_y,
        );
        td.get_one_page(chap_tui.start_line_num)?;
        Ok(())
    }
}
