pub(crate) mod edit;
pub(crate) mod hex;
#[cfg(test)]
mod large_file_tests;
pub(crate) mod text;
use crate::chap;
use crate::command::Command;
use crate::command::FindValue;
use crate::common::error::{ChapError, ChapResult};
use crate::common::ring_vec::RingVec;
use crate::execute;
use crate::handle::text::HandleText;
use crate::lua::LuaPlugin;
use crate::textwarp::LineState;
use crate::textwarp::TextDisplay;
use crate::textwarp::TextOper;
use crate::textwarp::TextWarpType;
use crate::undo::undo::OpType;
use crate::ChapTui;
use crossterm::cursor::Show;
use ratatui::restore;
use std::io::stdout;
use std::path::Path;
use std::process::exit;

pub(crate) use edit::HandleEdit;
pub(crate) use hex::HandleHex;

pub(crate) struct HandleBase;

fn line_visual_start(state: &LineState) -> usize {
    state.get_line_file_start() + state.get_line_offset()
}

fn line_state_at_byte_offset(
    td: &TextDisplay,
    byte_offset: usize,
    prefer_next_at_boundary: bool,
) -> ChapResult<(LineState, usize)> {
    if let TextDisplay::EditBlock(v) = td {
        if let Some((block_id, block_offset)) =
            v.resolve_block_for_file_offset_with_boundary(byte_offset, prefer_next_at_boundary)
        {
            let block_line_index = v
                .find_block_line_for_offset(block_id, block_offset)
                .unwrap_or(0);
            let state = LineState::builder()
                .block_num(block_id)
                .block_line_index(block_line_index)
                .block_offset(block_offset)
                .line_offset(0)
                .line_file_start(byte_offset)
                .line_file_end(byte_offset)
                .build();
            return Ok((state, 0));
        }
    }

    let mut boundary_match = None;
    {
        for state in td.get_current_line_meta()?.iter() {
            let start = line_visual_start(state);
            let end = start + state.get_txt_len();
            if byte_offset < start {
                break;
            }
            if byte_offset == start {
                return Ok((state.clone(), 0));
            }
            if byte_offset < end || (!prefer_next_at_boundary && byte_offset == end) {
                return Ok((state.clone(), byte_offset - start));
            }
            if byte_offset == end {
                boundary_match = Some((state.clone(), byte_offset - start));
            }
        }
    }

    if let Some(matched) = boundary_match {
        return Ok(matched);
    }

    Err(ChapError::Unexpected(format!(
        "undo byte offset {} not found in current page metadata",
        byte_offset
    )))
}

impl HandleBase {
    pub(crate) fn handle_up<'a>(
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
                    line_meta = td.get_current_line_meta()?;
                    if let Some(first) = line_meta.get(0) {
                        chap_tui.start_line_state = first.clone();
                    }
                }
                chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
                if let Some(meta) = line_meta.get(chap_tui.cursor_y) {
                    if chap_tui.cursor_x >= meta.get_char_len() {
                        chap_tui.cursor_x = meta.get_char_len();
                    }
                    if chap_tui.column_offset >= meta.get_char_len() {
                        chap_tui.column_offset = meta.get_char_len();
                    }
                }
                chap_tui.is_last_line = false;
            }
            TextWarpType::SoftWrap => {
                if chap_tui.cursor_y == 0 {
                    //滚动上一行
                    if let Some(first_meta) = line_meta.get(0) {
                        if first_meta.has_pre_line() {
                            td.scroll_pre_one_line(first_meta)?;
                            line_meta = td.get_current_line_meta()?;
                            if let Some(first) = line_meta.get(0) {
                                chap_tui.start_line_state = first.clone();
                            }
                        }
                    }
                }
                chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
                if let Some(meta) = line_meta.get(chap_tui.cursor_y) {
                    if chap_tui.cursor_x >= meta.get_char_len().saturating_sub(1) {
                        chap_tui.cursor_x = meta.get_char_len().saturating_sub(1);
                    }
                }
                chap_tui.is_last_line = false;
            }
        }
        Ok(())
    }

    pub(crate) fn handle_down<'a>(
        &self,
        chap_tui: &mut ChapTui,
        mut line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        // if chap_tui.in_command_mode() {
        //     self.cmdinp_edit.handle_down(chap_tui, td)?;
        //     return Ok(());
        // }
        match chap_tui.warp_type {
            TextWarpType::NoWrap => {
                if chap_tui.cursor_y < chap_tui.elem.tv.get_height() - 1 {
                    chap_tui.cursor_y += 1;
                } else {
                    //滚动下一行
                    td.scroll_next_one_line(line_meta.last().unwrap())?;
                    line_meta = td.get_current_line_meta()?;
                    if let Some(first) = line_meta.get(0) {
                        chap_tui.start_line_state = first.clone();
                    }
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
                    if let Some(first) = line_meta.get(0) {
                        chap_tui.start_line_state = first.clone();
                    }
                }
                if let Some(meta) = line_meta.get(chap_tui.cursor_y) {
                    if chap_tui.cursor_x >= meta.get_char_len().saturating_sub(1) {
                        chap_tui.cursor_x = meta.get_char_len().saturating_sub(1);
                    }
                }
                chap_tui.is_last_line = false;
            }
        }
        Ok(())
    }

    pub(crate) fn handle_left<'a>(
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
                    // 越界时视为行首（line_offset=0），直接 noop
                    let line_offset = line_meta
                        .get(chap_tui.cursor_y)
                        .map_or(0, |m| m.get_line_offset());
                    // 这个判断说明当前行已经读完了
                    if line_offset == 0 {
                        //无需操作
                    } else if chap_tui.cursor_y > 0 {
                        chap_tui.cursor_x = line_meta
                            .get(chap_tui.cursor_y - 1)
                            .map_or(0, |m| m.get_char_len().saturating_sub(1));
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

    pub(crate) fn handle_right<'a>(
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
                if chap_tui.column_offset <= meta.get_char_len() {
                    chap_tui.column_offset += 1;
                }
            }
            TextWarpType::SoftWrap => {
                let Some(cur_meta) = line_meta.get(chap_tui.cursor_y) else {
                    chap_tui.is_last_line = false;
                    return Ok(());
                };
                if chap_tui.cursor_x <= cur_meta.get_char_len().saturating_sub(1) {
                    chap_tui.cursor_x += 1;

                    if chap_tui.cursor_x >= cur_meta.get_char_len()
                        && chap_tui.cursor_y < chap_tui.elem.tv.get_height()
                    {
                        //判断当前行是否读完（检查下一视觉段是否属于同一逻辑行）
                        let has_next_seg = line_meta
                            .get(chap_tui.cursor_y + 1)
                            .map_or(false, |next| next.get_line_offset() > 0);
                        if has_next_seg {
                            chap_tui.cursor_x = 0;
                            chap_tui.cursor_y += 1;
                        } else {
                            // 行末，无后续换行段，回退增量
                            chap_tui.cursor_x -= 1;
                        }
                    }
                }
                chap_tui.is_last_line = false;
            }
        }
        Ok(())
    }
}

pub(crate) enum HandleImpl {
    Edit(HandleEdit),
    Hex(HandleHex<LuaPlugin>),
    Text(HandleText),
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
            HandleImpl::Text(h) => h.handle_ctrl_s(chap_tui, p, td),
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
            HandleImpl::Text(h) => h.handle_up(chap_tui, line_meta, td),
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
            HandleImpl::Text(h) => h.handle_shift_up(chap_tui, line_meta, td),
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
            HandleImpl::Text(h) => h.handle_shift_down(chap_tui, line_meta, td),
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
            HandleImpl::Text(h) => h.handle_shift_right(chap_tui, line_meta, td),
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
            HandleImpl::Text(h) => h.handle_shift_left(chap_tui, line_meta, td),
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
            HandleImpl::Text(h) => h.handle_down(chap_tui, line_meta, td),
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
            HandleImpl::Text(h) => h.handle_left(chap_tui, line_meta, td),
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
            HandleImpl::Text(h) => h.handle_right(chap_tui, line_meta, td),
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
            HandleImpl::Text(h) => h.handle_enter(chap_tui, line_meta, td),
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
            HandleImpl::Text(h) => h.handle_backspace(chap_tui, line_meta, td),
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
            HandleImpl::Text(h) => h.handle_char(chap_tui, line_meta, td, c),
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
            HandleImpl::Text(h) => h.handle_paste(chap_tui, line_meta, td, pasted_string),
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
        chap_tui.enter_command_mode();
        chap_tui.find_highlight_index = 0;
        chap_tui.highlight_len = 0;
        chap_tui.find_list = None;
        chap_tui.find_index = 0;
        Ok(())
    }

    fn handle_ctrl_c(&self, chap_tui: &mut ChapTui) -> ChapResult<()> {
        tui_retore()?;
        exit(0);
    }

    fn handle_ctrl_z<'a>(&self, chap_tui: &mut ChapTui, td: &'a TextDisplay) -> ChapResult<()> {
        // 取一条 undo record
        //    push() 时已存为逆操作，undo() 直接返回可执行的逆操作
        let Some(undo) = &mut chap_tui.undo else {
            return Ok(()); // undo 未启用
        };
        let Some(op) = undo.undo()? else {
            return Ok(()); // undo 栈空
        };

        // ④ 按 op_type 执行逆操作（record 里存的就是逆操作类型）
        //
        //   原操作          存储的逆操作      执行动作
        //   ──────────────────────────────────────────
        //   InsertChar  →  DeleteChar    →  backspace
        //   DeleteChar  →  InsertChar    →  insert_char
        //   InsertNewline→ DeleteNewline →  backspace(1 byte)
        //   DeleteNewline→ InsertNewline →  insert_newline
        //
        match op.op_type {
            OpType::DeleteChar => {
                let (meta, bytes_cursor) =
                    line_state_at_byte_offset(td, op.byte_offset as usize, false)?;
                td.backspace(
                    op.cursor_y as usize,
                    bytes_cursor,
                    op.data.len(), // 要删掉的字节数
                    &meta,
                )?;
            }
            OpType::InsertChar => {
                let (meta, bytes_cursor) =
                    line_state_at_byte_offset(td, op.byte_offset as usize, false)?;
                td.insert_bytes(op.cursor_y as usize, bytes_cursor, &meta, &op.data, false)?;
            }
            OpType::DeleteNewline => {
                let (meta, bytes_cursor) =
                    line_state_at_byte_offset(td, op.byte_offset as usize, true)?;
                td.delete_newline(op.cursor_y as usize, bytes_cursor, &meta)?;
            }
            OpType::InsertNewline => {
                let (meta, bytes_cursor) =
                    line_state_at_byte_offset(td, op.byte_offset as usize, false)?;
                td.insert_newline(op.cursor_y as usize, bytes_cursor, &meta)?;
            }
        }

        // ⑤ 恢复光标到操作发生时的位置
        chap_tui.cursor_y = op.cursor_y as usize;
        chap_tui.cursor_x = op.cursor_x as usize;
        chap_tui.is_last_line = false;
        // ⑥ 刷新页面
        td.get_one_page_from_state(&chap_tui.start_line_state)?;
        Ok(())
    }

    //-----------------以下要自定义实现----------------------//
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

    fn handle_ctrl_r<'a>(&self, chap_tui: &mut ChapTui, td: &'a TextDisplay) -> ChapResult<()> {
        Ok(())
    }
}
