pub(crate) mod edit;
pub(crate) mod hex;
#[cfg(test)]
mod large_file_tests;
use crate::common::error::ChapResult;
use crate::common::ring_vec::RingVec;
use crate::execute;
use crate::lua::LuaPlugin;
use crate::textwarp::LineState;
use crate::textwarp::TextDisplay;
use crate::textwarp::TextOper;
use crate::tui::edit::get_edit_content;
use crate::undo::undo::OpType;
use crate::ChapTui;
use crossterm::cursor::Show;
use ratatui::restore;
use std::io::stdout;
use std::path::Path;
use std::process::exit;

pub(crate) use edit::HandleEdit;
pub(crate) use hex::HandleHex;

pub(crate) enum HandleImpl {
    Edit(HandleEdit),
    Hex(HandleHex<LuaPlugin>),
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
        line_meta: &RingVec<LineState>,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_up(chap_tui, line_meta, td),
            HandleImpl::Hex(h) => h.handle_up(chap_tui, line_meta, td),
        }
    }

    fn handle_shift_up<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_shift_up(chap_tui, line_meta, td),
            HandleImpl::Hex(h) => h.handle_shift_up(chap_tui, line_meta, td),
        }
    }

    fn handle_shift_down<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_shift_down(chap_tui, line_meta, td),
            HandleImpl::Hex(h) => h.handle_shift_down(chap_tui, line_meta, td),
        }
    }

    fn handle_shift_right(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<LineState>,
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
        line_meta: &RingVec<LineState>,
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
        line_meta: &RingVec<LineState>,
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
        line_meta: &'a RingVec<LineState>,
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
        line_meta: &'a RingVec<LineState>,
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
        line_meta: &'a RingVec<LineState>,
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
        line_meta: &'a RingVec<LineState>,
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
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
        c: char,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_char(chap_tui, line_meta, td, c),
            HandleImpl::Hex(h) => h.handle_char(chap_tui, line_meta, td, c),
        }
    }

    fn handle_paste<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
        pasted_string: &str,
    ) -> ChapResult<()> {
        match self {
            HandleImpl::Edit(h) => h.handle_paste(chap_tui, line_meta, td, pasted_string),
            HandleImpl::Hex(h) => h.handle_paste(chap_tui, line_meta, td, pasted_string),
        }
    }
}

pub(crate) fn tui_retore() -> ChapResult<()> {
    restore();
    execute!(
        stdout(),
        Show // 显示光标
    )?;
    Ok(())
}

pub(crate) trait Handle {
    fn handle_esc(&self, chap_tui: &mut ChapTui) -> ChapResult<()> {
        chap_tui.elem.cmd_inp.clear();
        chap_tui.elem.navi.clear();
        chap_tui.assist_tv2_data.clear();
        chap_tui.txt_sel.reset_to_start();
        Ok(())
    }

    fn handle_ctrl_c(&self, chap_tui: &mut ChapTui) -> ChapResult<()> {
        tui_retore()?;
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
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_shift_down<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_shift_up<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_shift_right<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_shift_left(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<LineState>,
        td: &TextDisplay,
    ) -> ChapResult<()>;

    fn handle_down<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_left<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_right<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_enter<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_backspace<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()>;

    fn handle_char<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
        c: char,
    ) -> ChapResult<()>;

    fn handle_paste<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
        pasted_string: &str,
    ) -> ChapResult<()>;

    fn handle_ctrl_z<'a>(&self, chap_tui: &mut ChapTui, td: &'a TextDisplay) -> ChapResult<()> {
        // 取一条 undo record
        //    push() 时已存为逆操作，undo() 直接返回可执行的逆操作
        let Some(op) = chap_tui.undo.as_mut().and_then(|u| u.undo().ok().flatten()) else {
            return Ok(()); // undo 未启用 或 栈空
        };

        // block_id 稳定，不受 split_block 影响；
        // byte_offset 是块内绝对偏移（操作后位置），block_offset/line_offset 均设为 0，
        // backspace 内部会直接用 bytes_cursor（= byte_offset）作为块内绝对位置。
        let block_id = op.block_id as usize;
        let (resolved_block_num, resolved_block_line_index) = if let TextDisplay::EditBlock(v) = td
        {
            let char_start = (op.byte_offset as usize).saturating_sub(op.data.len());
            let bli = v
                .find_block_line_for_offset(block_id, char_start)
                .unwrap_or(0);
            v.ensure_block_loaded(block_id)?;
            (block_id, bli)
        } else {
            (0, 0)
        };
        let target_line = op.line_index as usize;
        chap_tui.start_line_num = target_line;
        // ④ 按 op_type 执行逆操作（record 里存的就是逆操作类型）
        //
        //   原操作          存储的逆操作      执行动作
        //   ──────────────────────────────────────────
        //   InsertChar  →  DeleteChar    →  backspace
        //   DeleteChar  →  InsertChar    →  insert_char
        //   InsertNewline→ DeleteNewline →  backspace(1 byte)
        //   DeleteNewline→ InsertNewline →  insert_newline
        //
        // block_offset=0, line_offset=0：backspace 计算
        //   insert_offset = block_offset + line_offset + bytes_cursor = byte_offset
        // 直接得到块内绝对位置，无需额外转换。
        let meta = LineState {
            char_with: 0,
            txt_len: 0,
            char_len: 0,
            page_num: 0,
            block_num: resolved_block_num,
            block_line_index: resolved_block_line_index,
            block_offset: 0,
            line_num: target_line,
            line_index: op.line_index as usize,
            line_offset: 0,
            line_file_start: 0,
            line_file_end: 0,
            start_line_num: 0,
            start_page_num: 0,
        };
        match op.op_type {
            OpType::DeleteChar => {
                td.backspace(
                    op.cursor_y as usize,
                    op.byte_offset as usize,
                    op.data.len(), // 要删掉的字节数
                    &meta,
                )?;
            }
            OpType::InsertChar => {
                td.insert_bytes(
                    op.cursor_y as usize,
                    op.byte_offset as usize,
                    &meta,
                    &op.data,
                    false,
                )?;
            }
            OpType::DeleteNewline => {
                td.delete_newline(op.cursor_y as usize, op.byte_offset as usize, &meta)?;
            }
            OpType::InsertNewline => {
                td.insert_newline(op.cursor_y as usize, op.byte_offset as usize, &meta)?;
            }
        }

        // ⑤ 恢复光标到操作发生时的位置
        chap_tui.cursor_y = op.cursor_y as usize;
        chap_tui.cursor_x = op.cursor_x as usize;
        chap_tui.is_last_line = false;

        // ⑥ 刷新页面
        td.get_one_page(chap_tui.start_line_num)?;
        // let (content, meta) = td.get_current_page()?;
        // let (_, _, byte_cursor, last_char_bytes_size) = get_edit_content(
        //     content,
        //     chap_tui.elem.tv.get_width(),
        //     &meta,
        //     0,
        //     &None,
        //     chap_tui.elem.tv.get_height(),
        //     chap_tui
        //         .column_offset
        //         .saturating_sub(chap_tui.elem.tv.get_width()),
        //     chap_tui.cursor_y,
        //     chap_tui.cursor_x,
        // );
        // chap_tui.bytes_cursor = byte_cursor;
        // chap_tui.bytes_cursor_size = last_char_bytes_size;
        Ok(())
    }

    fn handle_ctrl_r<'a>(&self, chap_tui: &mut ChapTui, td: &'a TextDisplay) -> ChapResult<()> {
        Ok(())
    }
}
