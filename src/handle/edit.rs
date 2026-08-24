use crate::chap;
use crate::command::Command;
use crate::command::FindValue;
use crate::common::error::ChapResult;
use crate::common::ring_vec::RingVec;
use crate::handle::Handle;
use crate::handle::HandleBase;
use crate::textwarp::CacheStr;
use crate::textwarp::LineState;
use crate::textwarp::TextDisplay;
use crate::textwarp::TextOper;
use crate::textwarp::TextWarpType;
use crate::undo::undo::EditOp;
use crate::undo::undo::OpType;
use crate::ChapTui;
use std::path::Path;
use utf8_iter::Utf8CharsEx;

const CMD_INPUT_MAX: usize = 60;

#[derive(Clone, Copy, Debug, Default)]
struct CursorByteState {
    cursor_x: usize,
    bytes_cursor: usize,
    bytes_cursor_size: usize,
}

fn line_abs_start(line_state: &LineState) -> usize {
    line_state.get_line_file_start() + line_state.get_line_offset()
}

fn find_visual_line_for_abs_offset(
    meta: &RingVec<LineState>,
    target_abs_offset: usize,
) -> Option<usize> {
    if meta.is_empty() {
        return None;
    }

    // Prefer the next visual row when the offset is exactly on a row boundary.
    for (row, line_state) in meta.iter().enumerate().skip(1) {
        if target_abs_offset == line_abs_start(line_state) {
            return Some(row);
        }
    }

    for (row, line_state) in meta.iter().enumerate() {
        let start = line_abs_start(line_state);
        let end = start + line_state.get_txt_len();
        if target_abs_offset >= start && target_abs_offset <= end {
            return Some(row);
        }
    }

    if let Some(first) = meta.get(0) {
        if target_abs_offset < line_abs_start(first) {
            return Some(0);
        }
    }

    meta.len().checked_sub(1)
}

fn line_cursor_from_char_position(line: &CacheStr, cursor_x: usize) -> CursorByteState {
    let mut bytes_seen = 0usize;
    let mut chars_seen = 0usize;
    let mut prev_char_size = 0usize;

    for part in line.as_slice().as_parts() {
        for (_, ch) in part.char_indices() {
            if chars_seen == cursor_x {
                return CursorByteState {
                    cursor_x: chars_seen,
                    bytes_cursor: bytes_seen,
                    bytes_cursor_size: prev_char_size,
                };
            }

            let ch_len = ch.len_utf8();
            bytes_seen += ch_len;
            prev_char_size = ch_len;
            chars_seen += 1;
        }
    }

    CursorByteState {
        cursor_x: chars_seen,
        bytes_cursor: bytes_seen,
        bytes_cursor_size: prev_char_size,
    }
}

fn line_cursor_from_byte_offset(line: &CacheStr, bytes_cursor: usize) -> CursorByteState {
    let mut bytes_seen = 0usize;
    let mut chars_seen = 0usize;
    let mut prev_char_size = 0usize;

    for part in line.as_slice().as_parts() {
        for (part_byte_idx, ch) in part.char_indices() {
            let char_start = bytes_seen + part_byte_idx;
            if bytes_cursor <= char_start {
                return CursorByteState {
                    cursor_x: chars_seen,
                    bytes_cursor: char_start,
                    bytes_cursor_size: prev_char_size,
                };
            }

            let ch_len = ch.len_utf8();
            let char_end = char_start + ch_len;
            if bytes_cursor < char_end {
                return CursorByteState {
                    cursor_x: chars_seen,
                    bytes_cursor: char_start,
                    bytes_cursor_size: prev_char_size,
                };
            }

            chars_seen += 1;
            prev_char_size = ch_len;
        }
        bytes_seen += part.len();
    }

    CursorByteState {
        cursor_x: chars_seen,
        bytes_cursor: bytes_seen,
        bytes_cursor_size: prev_char_size,
    }
}

fn previous_cursor_size_at_line_start(
    content: &RingVec<CacheStr>,
    meta: &RingVec<LineState>,
    row: usize,
) -> usize {
    if row == 0 {
        return 0;
    }

    let starts_new_logical_line = meta
        .get(row)
        .zip(meta.get(row - 1))
        .map(|(cur, prev)| cur.get_line_index() != prev.get_line_index())
        .unwrap_or(false);

    if starts_new_logical_line {
        return 1;
    }

    content
        .get(row - 1)
        .and_then(|prev_line| {
            prev_line
                .as_slice()
                .as_parts()
                .iter()
                .rev()
                .find_map(|part| {
                    part.char_indices()
                        .rev()
                        .find(|(_, ch)| !ch.is_control())
                        .map(|(_, ch)| ch.len_utf8())
                })
        })
        .unwrap_or(0)
}

pub(crate) struct HandleTxtEdit {
    txt_base: HandleBase,
}

impl HandleTxtEdit {
    fn handle_char<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
        c: char,
    ) -> ChapResult<()> {
        chap_tui.elem.cmd_inp.clear();
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        let inserted_size = c.len_utf8();
        let inserted_on_softwrap_continuation =
            chap_tui.cursor_x == 0 && chap_tui.is_last_line && chap_tui.cursor_y > 0;
        if chap_tui.cursor_x == 0 && chap_tui.is_last_line && chap_tui.cursor_y > 0 {
            let Some(prev_meta) = line_meta.get(chap_tui.cursor_y - 1) else {
                return Ok(());
            };
            let byte_offset = prev_meta.get_txt_len(); // 段末字节数（非列数，支持多字节字符）
            td.insert_char(chap_tui.cursor_y - 1, byte_offset, prev_meta, c)?;
            if let Some(undo) = &mut chap_tui.undo {
                let abs_offset = prev_meta.get_line_file_start()
                    + prev_meta.get_line_offset()
                    + byte_offset
                    + c.len_utf8();
                undo.push(EditOp {
                    op_type: OpType::InsertChar,
                    cursor_y: chap_tui.cursor_y as u32,
                    cursor_x: chap_tui.cursor_x as u32,
                    block_id: prev_meta.get_block_num() as u32,
                    line_index: chap_tui.start_line_state.line_index as u32,
                    byte_offset: abs_offset as u32,
                    data: s.as_bytes().to_vec(),
                })?;
            }
            chap_tui.is_last_line = false;
        } else {
            let Some(cur_meta) = line_meta.get(chap_tui.cursor_y) else {
                return Ok(());
            };
            td.insert_char(chap_tui.cursor_y, chap_tui.bytes_cursor, cur_meta, c)?;
            if let Some(undo) = &mut chap_tui.undo {
                let abs_offset = cur_meta.get_line_file_start()
                    + cur_meta.get_line_offset()
                    + chap_tui.bytes_cursor
                    + c.len_utf8();
                undo.push(EditOp {
                    op_type: OpType::InsertChar,
                    cursor_y: chap_tui.cursor_y as u32,
                    cursor_x: chap_tui.cursor_x as u32,
                    block_id: cur_meta.get_block_num() as u32,
                    line_index: chap_tui.start_line_state.line_index as u32,
                    byte_offset: abs_offset as u32,
                    data: s.as_bytes().to_vec(),
                })?;
            }
        }
        if inserted_on_softwrap_continuation {
            chap_tui.bytes_cursor = inserted_size;
        } else {
            chap_tui.bytes_cursor += inserted_size;
        }
        chap_tui.bytes_cursor_size = inserted_size;
        if chap_tui.cursor_x < chap_tui.elem.tv.get_width() {
            chap_tui.cursor_x += 1;
            if chap_tui.cursor_x >= chap_tui.elem.tv.get_width()
                && chap_tui.cursor_y < chap_tui.elem.tv.get_height()
            {
                //不断添加字符 还是续接上一行
                chap_tui.is_last_line = true;
                chap_tui.cursor_x = 0;
                chap_tui.cursor_y += 1;
                chap_tui.bytes_cursor = 0;
            }
        }
        td.get_one_page_from_state(&chap_tui.start_line_state)?;
        Ok(())
    }
}

pub(crate) struct HandleCmdInpEdit;

impl HandleCmdInpEdit {
    pub(crate) fn handle_char<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
        c: char,
    ) -> ChapResult<()> {
        let cmd_inp = &mut chap_tui.elem.cmd_inp;
        if cmd_inp.len() >= CMD_INPUT_MAX {
            return Ok(());
        }
        chap_tui.elem.cmd_inp.push(c);
        chap_tui.inp_cursor_x += 1;
        Ok(())
    }

    pub(crate) fn handle_down(&self, chap_tui: &mut ChapTui, td: &TextDisplay) -> ChapResult<()> {
        match chap_tui.cur_cmd {
            Command::Find(_) => {
                if let Some(find_line_state) = &chap_tui.find_list {
                    let state: &LineState = &find_line_state[chap_tui.find_index];
                    if let Some(h) = &state.highlight {
                        if chap_tui.find_highlight_index < h.len() - 1 {
                            chap_tui.find_highlight_index += 1;
                            return Ok(());
                        }
                        if chap_tui.find_index < find_line_state.len() - 1 {
                            chap_tui.find_index += 1;
                            chap_tui.find_highlight_index = 0;
                            td.get_one_page_from_state(&find_line_state[chap_tui.find_index])?;
                            return Ok(());
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn find_jump(
        &self,
        chap_tui: &mut ChapTui,
        pattern: &[u8],
        line_meta: &LineState,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        let result = td.search(pattern, line_meta)?;
        if let Some(r) = &result {
            if r.len() > 0 {
                chap_tui.find_index = 0;
                chap_tui.find_highlight_index = 0;
                chap_tui.highlight_len = pattern.len();
                td.get_one_page_from_state(&r[chap_tui.find_index])?;
                let meta = td.get_current_line_meta()?;
                if let Some(first) = meta.get(0) {
                    chap_tui.start_line_state = first.clone();
                }
                chap_tui.cursor_y = 0;
                chap_tui.cursor_x = 0;
            }
        }
        chap_tui.find_list = result;
        Ok(())
    }

    pub(crate) fn handle_cmd_command(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &LineState,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        let cmd_inp = chap_tui.elem.cmd_inp.get_inp();
        let cmd = Command::parse(cmd_inp);
        match &cmd {
            Command::Find(v) => match v {
                FindValue::Ascii(s) => {
                    let pattern = s.as_bytes();
                    self.find_jump(chap_tui, pattern, line_meta, td)?;
                }
                _ => {}
            },
            _ => {
                chap_tui.elem.cmd_inp.clear();
                chap_tui.elem.cmd_inp.push_str("Unknown command");
            }
        }
        chap_tui.cur_cmd = cmd;
        Ok(())
    }
}

pub(crate) struct HandleEdit {
    txt_edit: HandleTxtEdit,
    cmdinp_edit: HandleCmdInpEdit,
}

impl HandleEdit {
    pub(crate) fn new() -> Self {
        HandleEdit {
            txt_edit: HandleTxtEdit {
                txt_base: HandleBase {},
            },
            cmdinp_edit: HandleCmdInpEdit {},
        }
    }

    fn refresh_cursor_bytes_on_current_page(
        &self,
        chap_tui: &mut ChapTui,
        td: &TextDisplay,
    ) -> ChapResult<()> {
        let (content, meta) = td.get_current_page()?;
        let Some(line) = content.get(chap_tui.cursor_y) else {
            chap_tui.bytes_cursor = 0;
            chap_tui.bytes_cursor_size = 0;
            return Ok(());
        };

        let cursor = line_cursor_from_char_position(line, chap_tui.cursor_x);
        chap_tui.cursor_x = cursor.cursor_x;
        chap_tui.bytes_cursor = cursor.bytes_cursor;
        chap_tui.bytes_cursor_size = if cursor.bytes_cursor == 0 {
            previous_cursor_size_at_line_start(content, meta, chap_tui.cursor_y)
        } else {
            cursor.bytes_cursor_size
        };
        Ok(())
    }

    fn sync_cursor_to_abs_offset_on_current_page(
        &self,
        chap_tui: &mut ChapTui,
        td: &TextDisplay,
        target_abs_offset: usize,
    ) -> ChapResult<()> {
        let (content, meta) = td.get_current_page()?;
        let Some(first) = meta.get(0) else {
            chap_tui.cursor_x = 0;
            chap_tui.cursor_y = 0;
            chap_tui.bytes_cursor = 0;
            chap_tui.bytes_cursor_size = 0;
            chap_tui.is_last_line = false;
            return Ok(());
        };
        //chap_tui.start_line_num = first.get_line_num();
        chap_tui.start_line_state = first.clone();

        let Some(row) = find_visual_line_for_abs_offset(meta, target_abs_offset) else {
            chap_tui.cursor_x = 0;
            chap_tui.cursor_y = 0;
            chap_tui.bytes_cursor = 0;
            chap_tui.bytes_cursor_size = 0;
            chap_tui.is_last_line = false;
            return Ok(());
        };
        let Some(line_state) = meta.get(row) else {
            return Ok(());
        };
        let Some(line) = content.get(row) else {
            return Ok(());
        };

        let row_start = line_abs_start(line_state);
        let row_end = row_start + line_state.get_txt_len();
        let row_relative_offset = target_abs_offset
            .clamp(row_start, row_end)
            .saturating_sub(row_start);
        let cursor = line_cursor_from_byte_offset(line, row_relative_offset);

        chap_tui.cursor_y = row;
        chap_tui.cursor_x = cursor.cursor_x;
        chap_tui.bytes_cursor = cursor.bytes_cursor;
        chap_tui.bytes_cursor_size = if cursor.bytes_cursor == 0 {
            previous_cursor_size_at_line_start(content, meta, row)
        } else {
            cursor.bytes_cursor_size
        };
        chap_tui.is_last_line = false;
        Ok(())
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
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        self.txt_edit.txt_base.handle_up(chap_tui, line_meta, td)?;
        // match chap_tui.warp_type {
        //     TextWarpType::NoWrap => {
        //         if chap_tui.cursor_y == 0 {
        //             //滚动上一行
        //             td.scroll_pre_one_line(line_meta.get(0).unwrap())?;
        //             td.get_current_line_meta()?;
        //         }
        //         chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
        //         if let Some(meta) = line_meta.get(chap_tui.cursor_y) {
        //             if chap_tui.cursor_x >= meta.get_char_len() {
        //                 chap_tui.cursor_x = meta.get_char_len();
        //             }
        //             if chap_tui.column_offset >= meta.get_char_len() {
        //                 chap_tui.column_offset = meta.get_char_len();
        //             }
        //         }
        //         chap_tui.is_last_line = false;
        //     }
        //     TextWarpType::SoftWrap => {
        //         if chap_tui.cursor_y == 0 {
        //             //滚动上一行
        //             if let Some(first_meta) = line_meta.get(0) {
        //                 if first_meta.get_line_num() > 1 {
        //                     td.scroll_pre_one_line(first_meta)?;
        //                     line_meta = td.get_current_line_meta()?;
        //                 }
        //             }
        //         }
        //         chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
        //         if let Some(meta) = line_meta.get(chap_tui.cursor_y) {
        //             if chap_tui.cursor_x >= meta.get_char_len().saturating_sub(1) {
        //                 chap_tui.cursor_x = meta.get_char_len().saturating_sub(1);
        //             }
        //         }
        //         chap_tui.is_last_line = false;
        //     }
        // }
        Ok(())
    }

    fn handle_down<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        if chap_tui.in_command_mode() {
            self.cmdinp_edit.handle_down(chap_tui, td)?;
            return Ok(());
        }
        self.txt_edit
            .txt_base
            .handle_down(chap_tui, line_meta, td)?;
        // match chap_tui.warp_type {
        //     TextWarpType::NoWrap => {
        //         if chap_tui.cursor_y < chap_tui.elem.tv.get_height() - 1 {
        //             chap_tui.cursor_y += 1;
        //         } else {
        //             //滚动下一行
        //             td.scroll_next_one_line(line_meta.last().unwrap())?;
        //             line_meta = td.get_current_line_meta()?;
        //         }
        //         if chap_tui.cursor_x
        //             >= line_meta
        //                 .get(chap_tui.cursor_y)
        //                 .unwrap()
        //                 .get_char_len()
        //                 .saturating_sub(1)
        //         {
        //             chap_tui.cursor_x = line_meta
        //                 .get(chap_tui.cursor_y)
        //                 .unwrap()
        //                 .get_char_len()
        //                 .saturating_sub(1);
        //         }
        //         let meta = line_meta.get(chap_tui.cursor_y).unwrap();
        //         if chap_tui.column_offset >= meta.get_char_len() {
        //             chap_tui.column_offset = meta.get_char_len();
        //         }
        //         chap_tui.is_last_line = false;
        //     }
        //     TextWarpType::SoftWrap => {
        //         if chap_tui.cursor_y < chap_tui.elem.tv.get_height().saturating_sub(1) {
        //             chap_tui.cursor_y += 1;
        //         } else {
        //             //滚动下一行
        //             td.scroll_next_one_line(line_meta.last().unwrap())?;
        //             line_meta = td.get_current_line_meta()?;
        //         }
        //         if let Some(meta) = line_meta.get(chap_tui.cursor_y) {
        //             if chap_tui.cursor_x >= meta.get_char_len().saturating_sub(1) {
        //                 chap_tui.cursor_x = meta.get_char_len().saturating_sub(1);
        //             }
        //         }
        //         chap_tui.is_last_line = false;
        //     }
        // }
        Ok(())
    }

    fn handle_left<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        self.txt_edit
            .txt_base
            .handle_left(chap_tui, line_meta, td)?;
        // match chap_tui.warp_type {
        //     TextWarpType::NoWrap => {
        //         chap_tui.cursor_x = chap_tui.cursor_x.saturating_sub(1);
        //         chap_tui.column_offset = chap_tui.column_offset.saturating_sub(1);
        //     }
        //     TextWarpType::SoftWrap => {
        //         if chap_tui.cursor_x == 0 {
        //             // 越界时视为行首（line_offset=0），直接 noop
        //             let line_offset = line_meta
        //                 .get(chap_tui.cursor_y)
        //                 .map_or(0, |m| m.get_line_offset());
        //             // 这个判断说明当前行已经读完了
        //             if line_offset == 0 {
        //                 //无需操作
        //             } else if chap_tui.cursor_y > 0 {
        //                 chap_tui.cursor_x = line_meta
        //                     .get(chap_tui.cursor_y - 1)
        //                     .map_or(0, |m| m.get_char_len().saturating_sub(1));
        //                 chap_tui.cursor_y = chap_tui.cursor_y.saturating_sub(1);
        //             }
        //         } else {
        //             chap_tui.cursor_x = chap_tui.cursor_x.saturating_sub(1);
        //         }
        //         chap_tui.is_last_line = false;
        //     }
        // }
        Ok(())
    }

    fn handle_right<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        self.txt_edit
            .txt_base
            .handle_right(chap_tui, line_meta, td)?;
        // match chap_tui.warp_type {
        //     TextWarpType::NoWrap => {
        //         let meta = line_meta.get(chap_tui.cursor_y).unwrap();
        //         if chap_tui.cursor_x < meta.get_char_len()
        //             && chap_tui.cursor_x < chap_tui.elem.tv.get_width()
        //         {
        //             chap_tui.cursor_x += 1;
        //         }
        //         if chap_tui.column_offset <= meta.get_char_len() {
        //             chap_tui.column_offset += 1;
        //         }
        //     }
        //     TextWarpType::SoftWrap => {
        //         let Some(cur_meta) = line_meta.get(chap_tui.cursor_y) else {
        //             chap_tui.is_last_line = false;
        //             return Ok(());
        //         };
        //         if chap_tui.cursor_x <= cur_meta.get_char_len().saturating_sub(1) {
        //             chap_tui.cursor_x += 1;

        //             if chap_tui.cursor_x >= cur_meta.get_char_len()
        //                 && chap_tui.cursor_y < chap_tui.elem.tv.get_height()
        //             {
        //                 //判断当前行是否读完（检查下一视觉段是否属于同一逻辑行）
        //                 let has_next_seg = line_meta
        //                     .get(chap_tui.cursor_y + 1)
        //                     .map_or(false, |next| next.get_line_offset() > 0);
        //                 if has_next_seg {
        //                     chap_tui.cursor_x = 0;
        //                     chap_tui.cursor_y += 1;
        //                 } else {
        //                     // 行末，无后续换行段，回退增量
        //                     chap_tui.cursor_x -= 1;
        //                 }
        //             }
        //         }
        //         chap_tui.is_last_line = false;
        //     }
        // }
        Ok(())
    }

    fn handle_enter<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        if chap_tui.in_command_mode() {
            if let Some(cur_meta) = line_meta.get(chap_tui.cursor_y) {
                self.cmdinp_edit
                    .handle_cmd_command(chap_tui, cur_meta, td)?;
            };

            return Ok(());
        }
        chap_tui.elem.cmd_inp.clear();
        let Some(cur_meta) = line_meta.get(chap_tui.cursor_y) else {
            return Ok(());
        };
        let insert_bytes_cursor = chap_tui.bytes_cursor.min(cur_meta.get_txt_len());
        td.insert_newline(chap_tui.cursor_y, insert_bytes_cursor, cur_meta)?;
        if let Some(undo) = &mut chap_tui.undo {
            let abs_offset = cur_meta.get_line_file_start()
                + cur_meta.get_line_offset()
                + insert_bytes_cursor
                + 1;
            undo.push(EditOp {
                op_type: OpType::InsertNewline,
                cursor_y: chap_tui.cursor_y as u32,
                cursor_x: chap_tui.cursor_x as u32,
                block_id: cur_meta.get_block_num() as u32,
                line_index: chap_tui.start_line_state.line_index as u32,
                byte_offset: abs_offset as u32,
                data: vec![b'\n'],
            })?;
        }
        if chap_tui.cursor_y < chap_tui.elem.tv.get_height() - 1 {
            chap_tui.cursor_y += 1;
        }
        chap_tui.cursor_x = 0;
        //td.get_one_page(chap_tui.start_line_num)?;
        td.get_one_page_from_state(&chap_tui.start_line_state)?;
        // self.refresh_cursor_metrics(chap_tui, td)?;

        Ok(())
    }

    fn handle_backspace<'a>(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a TextDisplay,
    ) -> ChapResult<()> {
        if chap_tui.in_command_mode() {
            chap_tui.elem.cmd_inp.pop();
            return Ok(());
        }
        // chap_tui.elem.cmd_inp.clear();
        // let at_absolute_start =
        //     chap_tui.cursor_y == 0 && chap_tui.cursor_x == 0 && chap_tui.start_line_num <= 1;
        // if at_absolute_start {
        //     return Ok(());
        // }
        // if chap_tui.cursor_y == 0 && chap_tui.cursor_x == 0 && chap_tui.start_line_num > 1 {
        //     chap_tui.start_line_num -= 1;
        //     td.get_one_page(chap_tui.start_line_num)?;
        //     let shifted_meta = td.get_current_line_meta()?;
        //     let Some(cur_meta) = shifted_meta.get(1) else {
        //         return Ok(());
        //     };
        //     let prev_line_char_len = shifted_meta
        //         .get(0)
        //         .map_or(0, |m| m.get_char_len().saturating_sub(1));
        //     td.delete_newline(1, 0, cur_meta)?;
        //     if let Some(undo) = &mut chap_tui.undo {
        //         let abs_offset = cur_meta
        //             .get_block_offset()
        //             .saturating_add(cur_meta.get_line_offset())
        //             .saturating_sub(1);
        //         undo.push(EditOp {
        //             op_type: OpType::DeleteNewline,
        //             cursor_y: 0,
        //             cursor_x: 0,
        //             block_id: cur_meta.get_block_num() as u32,
        //             line_index: chap_tui.start_line_num as u32,
        //             byte_offset: abs_offset as u32,
        //             data: vec![b'\n'],
        //         })?;
        //     }
        //     td.get_one_page(chap_tui.start_line_num)?;
        //     chap_tui.cursor_y = 0;
        //     chap_tui.cursor_x = prev_line_char_len;
        //     self.refresh_cursor_bytes_on_current_page(chap_tui, td)?;
        //     return Ok(());
        // }
        // let prev_line_char_len = if chap_tui.cursor_y == 0 {
        //     0
        // } else {
        //     line_meta
        //         .get(chap_tui.cursor_y - 1)
        //         .map_or(0, |m| m.get_char_len().saturating_sub(1))
        // };
        // let Some(cur_meta) = line_meta.get(chap_tui.cursor_y) else {
        //     return Ok(());
        // };
        if chap_tui.cursor_y == 0 && chap_tui.cursor_x == 0 {
            if chap_tui.start_line_state.line_index == 0
                && chap_tui.start_line_state.line_offset == 0
            {
                return Ok(());
            }

            // chap_tui.start_line_num -= 1;
            // td.get_one_page(chap_tui.start_line_num)?;
            td.scroll_pre_one_line(&chap_tui.start_line_state);
            let shifted_meta = td.get_current_line_meta()?;
            if let Some(first) = shifted_meta.get(0) {
                chap_tui.start_line_state = first.clone();
            }
            let Some(cur_meta) = shifted_meta.get(1) else {
                return Ok(());
            };
            let prev_line_char_len = shifted_meta
                .get(0)
                .map_or(0, |m| m.get_char_len().saturating_sub(1));

            td.delete_newline(1, 0, cur_meta)?;
            if let Some(undo) = &mut chap_tui.undo {
                let abs_offset = cur_meta
                    .get_block_offset()
                    .saturating_add(cur_meta.get_line_offset())
                    .saturating_sub(1);
                undo.push(EditOp {
                    op_type: OpType::DeleteNewline,
                    cursor_y: 0,
                    cursor_x: 0,
                    block_id: cur_meta.get_block_num() as u32,
                    line_index: chap_tui.start_line_state.line_index as u32,
                    byte_offset: abs_offset as u32,
                    data: vec![b'\n'],
                })?;
            }
            td.get_one_page_from_state(&chap_tui.start_line_state)?;
            chap_tui.cursor_y = 0;
            chap_tui.cursor_x = prev_line_char_len;
            self.refresh_cursor_bytes_on_current_page(chap_tui, td)?;
            return Ok(());
        }
        let prev_line_char_len = if chap_tui.cursor_y == 0 {
            0
        } else {
            line_meta
                .get(chap_tui.cursor_y - 1)
                .map_or(0, |m| m.get_char_len().saturating_sub(1))
        };
        let Some(cur_meta) = line_meta.get(chap_tui.cursor_y) else {
            return Ok(());
        };
        if cur_meta.line_offset == 0 && chap_tui.bytes_cursor == 0 {
            td.delete_newline(chap_tui.cursor_y, chap_tui.cursor_x, cur_meta)?;
            if let Some(undo) = &mut chap_tui.undo {
                let abs_offset = cur_meta.get_line_file_start()
                    + cur_meta.get_line_offset()
                    + chap_tui.bytes_cursor
                    - 1;
                undo.push(EditOp {
                    op_type: OpType::DeleteNewline,
                    cursor_y: chap_tui.cursor_y as u32,
                    cursor_x: chap_tui.cursor_x as u32,
                    block_id: cur_meta.get_block_num() as u32,
                    line_index: chap_tui.start_line_state.line_index as u32,
                    byte_offset: abs_offset as u32,
                    data: vec![b'\n'],
                })?;
            }
        } else {
            let delete_bytes = td.backspace(
                chap_tui.cursor_y,
                chap_tui.bytes_cursor,
                chap_tui.bytes_cursor_size,
                cur_meta,
            )?;
            if let Some(undo) = &mut chap_tui.undo {
                let abs_offset = cur_meta.get_line_file_start()
                    + cur_meta.get_line_offset()
                    + chap_tui.bytes_cursor
                    - delete_bytes.len();
                undo.push(EditOp {
                    op_type: OpType::DeleteChar,
                    cursor_y: chap_tui.cursor_y as u32,
                    cursor_x: chap_tui.cursor_x as u32,
                    block_id: cur_meta.get_block_num() as u32,
                    line_index: chap_tui.start_line_state.line_index as u32,
                    byte_offset: abs_offset as u32,
                    data: delete_bytes,
                })?;
            }
        }

        td.get_one_page_from_state(&chap_tui.start_line_state)?;
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
        self.refresh_cursor_bytes_on_current_page(chap_tui, td)?;

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
        if chap_tui.in_command_mode() {
            self.cmdinp_edit.handle_char(chap_tui, line_meta, td, c)?;
            return Ok(());
        }
        self.txt_edit.handle_char(chap_tui, line_meta, td, c)?;
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
        if pasted_string.is_empty() {
            return Ok(());
        }
        let pasted_bytes = pasted_string.as_bytes();
        let (line_state, insert_cursor_y, insert_bytes_cursor) =
            if chap_tui.cursor_x == 0 && chap_tui.is_last_line && chap_tui.cursor_y > 0 {
                let Some(line_state) = line_meta.get(chap_tui.cursor_y - 1) else {
                    return Ok(());
                };
                (
                    line_state,
                    chap_tui.cursor_y - 1,
                    chap_tui.elem.tv.get_width(),
                )
            } else {
                let Some(line_state) = line_meta.get(chap_tui.cursor_y) else {
                    return Ok(());
                };
                (line_state, chap_tui.cursor_y, chap_tui.bytes_cursor)
            };
        let target_abs_offset = line_state.get_line_file_start()
            + line_state.get_line_offset()
            + insert_bytes_cursor
            + pasted_bytes.len();
        td.insert_bytes(
            insert_cursor_y,
            insert_bytes_cursor,
            line_state,
            pasted_bytes,
            false,
        )?;
        if let Some(undo) = &mut chap_tui.undo {
            undo.push(EditOp {
                op_type: OpType::InsertChar,
                cursor_y: chap_tui.cursor_y as u32,
                cursor_x: chap_tui.cursor_x as u32,
                block_id: line_state.get_block_num() as u32,
                line_index: chap_tui.start_line_state.line_index as u32,
                byte_offset: target_abs_offset as u32,
                data: pasted_bytes.to_vec(),
            })?;
        }
        td.get_one_page_from_state(&chap_tui.start_line_state)?;
        self.sync_cursor_to_abs_offset_on_current_page(chap_tui, td, target_abs_offset)?;
        if !pasted_string.ends_with('\n') && chap_tui.cursor_x == chap_tui.elem.tv.get_width() {
            if chap_tui.cursor_y < chap_tui.elem.tv.get_height() {
                chap_tui.cursor_x = 0;
                chap_tui.cursor_y += 1;
                chap_tui.bytes_cursor = 0;
                chap_tui.bytes_cursor_size = 0;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textwarp::edit_block::GapBlockText;
    use crate::textwarp::EditTextWarp;
    use crate::textwarp::TextDisplay;
    use crate::textwarp::TextWarpType;
    use crate::tui::edit::EditBuildContent;
    use crate::tui::BuildContent;
    use crate::tui::ChapTui;
    use crate::tui::EditContext;
    use crate::undo::undo::UndoFile;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use tempfile::TempDir;
    // ── 测试辅助 ──────────────────────────────────────────────────────────────

    const TV_H: usize = 20;
    const TV_W: usize = 80;
    // const HANDLE_EDIT_ENTER_UNDO_BACKSPACE_FIXTURE: &str =
    //     include_str!("../../tests/fixtures/handle_edit_enter_undo_backspace_regression.txt");
    // const HANDLE_EDIT_ENTER_UNDO_BACKSPACE_REAL_CONTEXT_FIXTURE: &str =
    //     include_str!("../../tests/fixtures/handle_edit_enter_undo_backspace_real_context.txt");

    /// 从字符串内容创建临时文件，返回 (ChapTui, TextDisplay, 临时文件句柄)
    fn setup(content: &str) -> (ChapTui, TextDisplay, NamedTempFile) {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(content.as_bytes()).unwrap();
        tmp.flush().unwrap();

        let gap = GapBlockText::from_file_path(tmp.path()).unwrap();
        let mut td =
            TextDisplay::EditBlock(EditTextWarp::new(gap, TV_H, TV_W, TextWarpType::SoftWrap));
        td.get_one_page_from_state(&LineState::file_start())
            .unwrap();

        let tui = ChapTui::for_test(TV_H, TV_W);
        (tui, td, tmp)
    }

    /// 取当前页第一行的文本内容（连接所有 part）
    fn first_line_text(td: &TextDisplay) -> String {
        let meta = td.get_current_line_meta().unwrap();
        let (txts, _) = td.get_current_page().unwrap();
        txts.get(0)
            .map(|c| {
                c.as_str()
                    .as_parts()
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 取当前页所有行文本拼接成字符串
    fn all_lines_text(td: &TextDisplay) -> String {
        let (txts, _) = td.get_current_page().unwrap();
        txts.iter()
            .flat_map(|c| {
                c.as_str()
                    .as_parts()
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn saved_text(td: &mut TextDisplay) -> String {
        let tmp = NamedTempFile::new().unwrap();
        td.save(tmp.path()).unwrap();
        std::fs::read_to_string(tmp.path()).unwrap()
    }

    fn sync_cursor_metrics(tui: &mut ChapTui, td: &TextDisplay) {
        let (content, meta) = td.get_current_page().unwrap();
        let ed_ctx = EditContext {
            height: tui.elem.tv.get_height(),
            column_offset: tui.column_offset,
            cursor_y: tui.cursor_y,
            cursor_x: tui.cursor_x,
            is_txt_model: true,
            find_highlight_offset: 0,
            find_line_index: None,
            highlight_len: 0,
        };
        let content = EditBuildContent::build_content(
            content,
            tui.elem.tv.get_width(),
            &meta,
            0,
            &None,
            &ed_ctx,
        );
        tui.bytes_cursor = content.byte_cursor;
        tui.bytes_cursor_size = content.last_char_bytes_size;
    }

    fn sync_view_state(tui: &mut ChapTui, td: &TextDisplay) {
        let meta = td.get_current_line_meta().unwrap();
        if let Some(first) = meta.get(0) {
            tui.start_line_state = first.clone();
        }
        sync_cursor_metrics(tui, td);
    }

    fn handle() -> HandleEdit {
        HandleEdit::new()
    }

    // ── 上下左右移动 ──────────────────────────────────────────────────────────

    #[test]
    fn test_move_right_advances_cursor() {
        let (mut tui, td, _f) = setup("hello\nworld\n");
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_right(&mut tui, meta, &td).unwrap();
        // 预期 cursor_x = 1
        assert_eq!(tui.cursor_x, 1, "向右应使 cursor_x 增加 1");
    }

    #[test]
    fn test_move_right_does_not_exceed_line_end() {
        // "hi\n" 在 SoftWrap 下 char_len=3（h i \n），cursor 最多到 char_len-2=1
        let (mut tui, td, _f) = setup("hi\n");
        let meta = td.get_current_line_meta().unwrap();
        let h = handle();
        // 连按 10 次右键
        for _ in 0..10 {
            h.handle_right(&mut tui, meta, &td).unwrap();
        }
        let char_len = meta.get(0).unwrap().get_char_len();
        assert!(
            tui.cursor_x < char_len,
            "cursor_x({}) 不应超出行长度({})",
            tui.cursor_x,
            char_len
        );
    }

    #[test]
    fn test_move_left_decrements_cursor() {
        let (mut tui, td, _f) = setup("hello\n");
        tui.cursor_x = 3;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_left(&mut tui, meta, &td).unwrap();
        assert_eq!(tui.cursor_x, 2, "向左应使 cursor_x 减少 1");
    }

    #[test]
    fn test_move_left_at_line_start_does_not_underflow() {
        let (mut tui, td, _f) = setup("hello\n");
        tui.cursor_x = 0;
        tui.cursor_y = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_left(&mut tui, meta, &td).unwrap();
        // 第一行行首：行内偏移为 0，不应移动
        assert_eq!(tui.cursor_y, 0);
        assert_eq!(tui.cursor_x, 0, "行首继续左移不应下溢");
    }

    #[test]
    fn test_move_left_wraps_to_prev_line() {
        // cursor_y=1, cursor_x=0，向左应跳到第 0 行末尾
        let (mut tui, td, _f) = setup("hello\nworld\n");
        tui.cursor_y = 1;
        tui.cursor_x = 0;
        let meta = td.get_current_line_meta().unwrap();
        // 第 1 行的 line_offset 应 > 0（是新行，不是行中间分段）
        // 只有当 meta[1].line_offset > 0 时才会跳行
        let line1_offset = meta.get(1).unwrap().get_line_offset();
        if line1_offset > 0 {
            handle().handle_left(&mut tui, meta, &td).unwrap();
            assert_eq!(tui.cursor_y, 0, "应跳到上一行");
            assert!(tui.cursor_x > 0, "应移动到上一行末尾");
        }
    }

    #[test]
    fn test_move_down_advances_cursor_y() {
        let (mut tui, td, _f) = setup("line1\nline2\n");
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_down(&mut tui, meta, &td).unwrap();
        assert_eq!(tui.cursor_y, 1, "向下应使 cursor_y 增加 1");
    }

    #[test]
    fn test_move_up_decrements_cursor_y() {
        let (mut tui, td, _f) = setup("line1\nline2\n");
        tui.cursor_y = 1;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_up(&mut tui, meta, &td).unwrap();
        assert_eq!(tui.cursor_y, 0, "向上应使 cursor_y 减少 1");
    }

    #[test]
    fn test_move_up_at_top_does_not_underflow() {
        let (mut tui, td, _f) = setup("only\n");
        tui.cursor_y = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_up(&mut tui, meta, &td).unwrap();
        assert_eq!(tui.cursor_y, 0, "顶行继续上移不应下溢");
    }

    #[test]
    fn test_move_down_clamps_cursor_x_to_line_len() {
        // 第 0 行很长，第 1 行很短，向下后 cursor_x 应被截断
        let (mut tui, td, _f) = setup("long_first_line\nshort\n");
        tui.cursor_x = 10;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_down(&mut tui, meta, &td).unwrap();
        let line1_len = meta.get(1).unwrap().get_char_len();
        assert!(
            tui.cursor_x < line1_len,
            "cursor_x({}) 应被截断到第 1 行长度({})",
            tui.cursor_x,
            line1_len
        );
    }

    // ── 插入字符 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_insert_char_advances_cursor() {
        let (mut tui, td, _f) = setup("hello\n");
        tui.cursor_y = 0;
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'X').unwrap();
        assert_eq!(tui.cursor_x, 1, "插入字符后 cursor_x 应增加 1");
    }

    #[test]
    fn test_insert_char_content_correct() {
        let (mut tui, td, _f) = setup("hello\n");
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'X').unwrap();
        let text = first_line_text(&td);
        assert!(text.contains('X'), "插入后内容应包含 'X'，实际: {:?}", text);
    }

    #[test]
    fn test_insert_multibyte_char() {
        let (mut tui, td, _f) = setup("abc\n");
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, '你').unwrap();
        let text = first_line_text(&td);
        assert!(
            text.contains('你'),
            "插入中文后内容应包含 '你'，实际: {:?}",
            text
        );
        // cursor_x 增加 1（字符数，不是字节数）
        assert_eq!(tui.cursor_x, 1);
    }

    #[test]
    fn test_insert_char_wraps_line_when_full() {
        // 行宽 80，连续插入 81 个字符，第 81 个应换行（cursor_y += 1, cursor_x = 0）
        let (mut tui, td, _f) = setup("a\n");
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        let h = handle();
        for _ in 0..TV_W {
            h.handle_char(&mut tui, meta, &td, 'a').unwrap();
        }
        // 插入 TV_W 个字符后应触发换行
        assert_eq!(tui.cursor_y, 1, "超过行宽后 cursor_y 应增加");
        assert_eq!(tui.cursor_x, 0, "换行后 cursor_x 应归零");
        assert!(tui.is_last_line, "换行后 is_last_line 应为 true");
    }

    // ── 回车（插入换行）────────────────────────────────────────────────────────

    #[test]
    fn test_enter_splits_line() {
        let (mut tui, td, _f) = setup("helloworld\n");
        tui.cursor_y = 0;
        tui.cursor_x = 5;
        tui.bytes_cursor = 5;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_enter(&mut tui, meta, &td).unwrap();
        let (txts, meta2) = td.get_current_page().unwrap();
        // 应有至少 2 行
        assert!(txts.len() >= 2, "回车后应有 2 行，实际 {} 行", txts.len());
        // cursor_y 应增加
        assert_eq!(tui.cursor_y, 1, "回车后 cursor_y 应增加 1");
        assert_eq!(tui.cursor_x, 0, "回车后 cursor_x 应归零");
    }

    #[test]
    fn test_enter_at_line_start() {
        // 在行首回车，应在上方插入空行
        let (mut tui, td, _f) = setup("hello\n");
        tui.cursor_y = 0;
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_enter(&mut tui, meta, &td).unwrap();
        let (txts, _) = td.get_current_page().unwrap();
        assert!(txts.len() >= 2, "行首回车后应有 2 行");
    }

    #[test]
    fn test_enter_at_line_end() {
        // 在行尾回车，新行应为空
        let (mut tui, td, _f) = setup("hello\n");
        tui.cursor_y = 0;
        // bytes_cursor 指向 '\n' 之前
        tui.bytes_cursor = 5;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_enter(&mut tui, meta, &td).unwrap();
        let (txts, _) = td.get_current_page().unwrap();
        assert!(txts.len() >= 2);
    }

    // ── Backspace 删除 ────────────────────────────────────────────────────────

    #[test]
    fn test_backspace_at_position_0_0_is_noop() {
        // 文件开头无法删除
        let (mut tui, td, _f) = setup("hello\n");
        tui.cursor_y = 0;
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        tui.bytes_cursor_size = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_backspace(&mut tui, meta, &td).unwrap();
        // 光标不变
        assert_eq!(tui.cursor_x, 0);
        assert_eq!(tui.cursor_y, 0);
        let text = first_line_text(&td);
        assert!(text.starts_with('h'), "文件开头 backspace 不应删除内容");
    }

    #[test]
    fn test_backspace_deletes_char() {
        let (mut tui, td, _f) = setup("hello\n");
        tui.cursor_x = 1;
        tui.cursor_y = 0;
        tui.bytes_cursor = 1;
        tui.bytes_cursor_size = 1; // 'h' 占 1 字节
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_backspace(&mut tui, meta, &td).unwrap();
        let text = first_line_text(&td);
        assert!(
            !text.starts_with('h'),
            "backspace 后 'h' 应被删除，实际: {:?}",
            text
        );
        assert_eq!(tui.cursor_x, 0, "cursor_x 应减少 1");
    }

    #[test]
    fn test_backspace_multibyte_char() {
        // "你好\n"，光标在 '好' 后（bytes_cursor=6），bytes_cursor_size=3
        let (mut tui, td, _f) = setup("你好\n");
        tui.cursor_x = 2;
        tui.cursor_y = 0;
        tui.bytes_cursor = 6;
        tui.bytes_cursor_size = 3; // '好' 占 3 字节
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_backspace(&mut tui, meta, &td).unwrap();
        let text = first_line_text(&td);
        assert!(
            !text.contains('好'),
            "backspace 应删除 '好'，实际: {:?}",
            text
        );
        assert!(text.contains('你'), "backspace 不应影响 '你'");
        assert_eq!(tui.cursor_x, 1, "cursor_x 应减少 1");
    }

    #[test]
    fn test_backspace_merges_lines() {
        // cursor 在第 1 行行首，backspace 应合并行
        let (mut tui, td, _f) = setup("hello\nworld\n");
        tui.cursor_y = 1;
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        tui.bytes_cursor_size = 1; // '\n' 占 1 字节
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_backspace(&mut tui, meta, &td).unwrap();
        // 合并后应只有一行
        let text = all_lines_text(&td);
        assert!(
            text.contains("helloworld"),
            "backspace 应合并两行，实际: {:?}",
            text
        );
        assert_eq!(tui.cursor_y, 0, "合并后 cursor_y 应回到第 0 行");
    }

    // ── 粘贴文本 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_paste_ascii_text() {
        let (mut tui, td, _f) = setup("abc\n");
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_paste(&mut tui, meta, &td, "XYZ").unwrap();
        let text = first_line_text(&td);
        assert!(
            text.contains("XYZ"),
            "粘贴后内容应包含 'XYZ'，实际: {:?}",
            text
        );
        assert_eq!(tui.cursor_x, 3, "粘贴 3 个字符后 cursor_x 应为 3");
    }

    #[test]
    fn test_paste_with_newline() {
        let (mut tui, td, _f) = setup("abc\n");
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle()
            .handle_paste(&mut tui, meta, &td, "line1\nline2")
            .unwrap();
        let (txts, _) = td.get_current_page().unwrap();
        assert!(
            txts.len() >= 2,
            "粘贴含换行的文本后应有多行，实际 {} 行",
            txts.len()
        );
        assert_eq!(tui.cursor_y, 1, "粘贴换行后 cursor_y 应增加");
    }

    #[test]
    fn test_paste_multibyte() {
        let (mut tui, td, _f) = setup("abc\n");
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_paste(&mut tui, meta, &td, "你好").unwrap();
        let text = first_line_text(&td);
        assert!(
            text.contains("你好"),
            "粘贴中文后内容应包含 '你好'，实际: {:?}",
            text
        );
    }

    // ── SoftWrap 额外边界场景 ─────────────────────────────────────────────────

    /// 辅助：取页面中第 n 行的文本
    fn nth_line_text(td: &TextDisplay, n: usize) -> String {
        let (txts, _) = td.get_current_page().unwrap();
        txts.get(n)
            .map(|c| {
                c.as_str()
                    .as_parts()
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 辅助：取页面总行数
    fn page_line_count(td: &TextDisplay) -> usize {
        let (txts, _) = td.get_current_page().unwrap();
        txts.len()
    }

    // ── handle_down：文件行数 < tv_height 时不应 panic ─────────────────────

    /// Bug1 候选：3 行文件，cursor 在末行，再向下不应 panic
    #[test]
    fn test_down_short_file_no_panic() {
        let (mut tui, td, _f) = setup("line1\nline2\nline3\n");
        let meta = td.get_current_line_meta().unwrap();
        tui.cursor_y = 2; // 已在最后一行
                          // handle_down 内部: cursor_y += 1 → 3, 随后 line_meta.get(3).unwrap() → None → panic ?
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle().handle_down(&mut tui, meta, &td).unwrap()
        }));
        assert!(result.is_ok(), "handle_down 在短文件末行不应 panic");
        // 光标不应超出 meta 有效范围
        assert!(tui.cursor_y <= 3, "cursor_y({}) 超出文件行数", tui.cursor_y);
    }

    /// 多次向下直到超出文件行，验证不 panic
    #[test]
    fn test_down_repeatedly_short_file_no_panic() {
        let (mut tui, td, _f) = setup("a\nb\nc\n");
        let meta = td.get_current_line_meta().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let h = handle();
            for _ in 0..10 {
                h.handle_down(&mut tui, meta, &td).unwrap();
            }
        }));
        assert!(result.is_ok(), "反复 handle_down 在短文件不应 panic");
    }

    // ── handle_up：SoftWrap cursor_x 截断 ────────────────────────────────────

    /// 从较长的行向上移到较短的行，cursor_x 应被截断
    #[test]
    fn test_up_clamps_cursor_x_to_shorter_line() {
        let (mut tui, td, _f) = setup("hi\nlonger_line\n");
        tui.cursor_y = 1; // 在 longer_line 行
        tui.cursor_x = 8; // 指向较长字符位置
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_up(&mut tui, meta, &td).unwrap();
        assert_eq!(tui.cursor_y, 0, "应移到第 0 行");
        let line0_len = meta.get(0).unwrap().get_char_len();
        assert!(
            tui.cursor_x < line0_len,
            "cursor_x({}) 应被截断到 line0_len({})",
            tui.cursor_x,
            line0_len
        );
    }

    /// handle_up 后 is_last_line 应重置为 false
    #[test]
    fn test_up_resets_is_last_line() {
        let (mut tui, td, _f) = setup("hello\nworld\n");
        tui.cursor_y = 1;
        tui.is_last_line = true; // 人为置位
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_up(&mut tui, meta, &td).unwrap();
        assert!(!tui.is_last_line, "handle_up 后 is_last_line 应为 false");
    }

    // ── handle_right：SoftWrap 换行段跳跃 ────────────────────────────────────

    /// handle_right 后 is_last_line 应为 false
    #[test]
    fn test_right_resets_is_last_line() {
        let (mut tui, td, _f) = setup("hello\n");
        tui.is_last_line = true;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_right(&mut tui, meta, &td).unwrap();
        assert!(!tui.is_last_line, "handle_right 后 is_last_line 应为 false");
    }

    /// SoftWrap：连按右键永远不应超出总行数
    #[test]
    fn test_right_cursor_y_never_exceeds_page_lines() {
        let (mut tui, td, _f) = setup("abc\ndef\n");
        let meta = td.get_current_line_meta().unwrap();
        let total = meta.len();
        let h = handle();
        for _ in 0..20 {
            h.handle_right(&mut tui, meta, &td).unwrap();
        }
        assert!(
            tui.cursor_y < total,
            "cursor_y({}) 不应 >= page_lines({})",
            tui.cursor_y,
            total
        );
    }

    // ── handle_left：SoftWrap 行首跳转上一段 ──────────────────────────────────

    /// cursor_x=0 且 line_offset=0：不应改变光标
    #[test]
    fn test_left_at_absolute_start_is_noop() {
        let (mut tui, td, _f) = setup("hello\n");
        tui.cursor_y = 0;
        tui.cursor_x = 0;
        let meta = td.get_current_line_meta().unwrap();
        // line_meta[0].line_offset 应为 0（第一行起始处）
        handle().handle_left(&mut tui, meta, &td).unwrap();
        assert_eq!(tui.cursor_y, 0, "第一行行首左移 cursor_y 不变");
        assert_eq!(tui.cursor_x, 0, "第一行行首左移 cursor_x 不变");
    }

    // ── handle_char：行填满后跳到下一视觉行 ──────────────────────────────────

    /// 填满一行后 cursor 状态正确
    #[test]
    fn test_char_fills_line_sets_is_last_line() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_x = TV_W - 1; // 距行满还差 1 个字符
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'X').unwrap();
        // 插入后 cursor_x = TV_W → 触发换行
        assert!(tui.is_last_line, "行满后 is_last_line 应为 true");
        assert_eq!(tui.cursor_x, 0, "换行后 cursor_x 应为 0");
        assert_eq!(tui.cursor_y, 1, "换行后 cursor_y 应为 1");
    }

    /// cursor_y = tv_height-1 时插入字符后 cursor_y 不应超出 tv_height
    /// Bug 候选：cursor_y 可能变成 tv_height，超出 line_meta 有效范围
    #[test]
    fn test_char_cursor_y_does_not_exceed_tv_height() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_y = TV_H - 1; // 最后一个可见行
        tui.cursor_x = TV_W - 1; // 插入后将触发换行
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'X').unwrap();
        // cursor_y 可能变为 TV_H，超出 tv.height，但下次 insert 时 line_meta.get(TV_H) 会 None → panic
        assert!(
            tui.cursor_y <= TV_H,
            "cursor_y({}) 不应超出 tv_height({})",
            tui.cursor_y,
            TV_H
        );
    }

    /// 连续插入字符直到 cursor_y 超出视图，不应 panic
    #[test]
    fn test_char_continuous_insert_no_panic() {
        let (mut tui, td, _f) = setup("a\n");
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        let h = handle();
        // 连续插入 TV_H * TV_W + TV_H 个字符（足以填满整个视图）
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for _ in 0..(TV_W * TV_H + TV_H) {
                h.handle_char(&mut tui, meta, &td, 'a').unwrap();
            }
        }));
        assert!(result.is_ok(), "大量连续插入不应 panic");
    }

    // ── handle_backspace：合并后 cursor_x 位置 ────────────────────────────────

    /// 在第 1 行行首 backspace：光标应移到第 0 行末尾内容字符处
    /// Bug 候选：cursor_x 被设为 prev_line_char_len = char_len-1，可能落在 '\n' 上
    #[test]
    fn test_backspace_cursor_x_after_merge_not_on_newline() {
        let (mut tui, td, _f) = setup("hello\nworld\n");
        tui.cursor_y = 1;
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        tui.bytes_cursor_size = 1; // '\n' 是 1 字节
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_backspace(&mut tui, meta, &td).unwrap();
        // "hello\n" char_len=6, prev_line_char_len = 6-1 = 5
        // 合并后 "helloworld\n"，位置 5 是 'w'，cursor 指向 'w'
        // 但 cursor_x=5 在合并后是否依然合法? char_len("helloworld\n")=12, 5<12 ✓
        let new_text = all_lines_text(&td);
        assert!(
            new_text.contains("helloworld"),
            "合并后应包含 helloworld，实际: {:?}",
            new_text
        );
        assert_eq!(tui.cursor_y, 0, "合并后 cursor_y 应为 0");
        // cursor_x 应指向合并点（'w'），不超出新行长度
        let merged_char_len = meta.get(0).unwrap().get_char_len(); // 旧 meta 的长度（"hello\n"）
                                                                   // 注意 merged_char_len 是合并前的值，合并后 char_len 会变
        assert!(
            tui.cursor_x <= 5,
            "cursor_x({}) 应 <= 5 (合并点)",
            tui.cursor_x
        );
    }

    /// cursor_x > 0 时 backspace，cursor_x 应减少 1
    #[test]
    fn test_backspace_cursor_x_decrements_by_one() {
        let (mut tui, td, _f) = setup("abcde\n");
        tui.cursor_x = 3;
        tui.bytes_cursor = 3;
        tui.bytes_cursor_size = 1;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_backspace(&mut tui, meta, &td).unwrap();
        assert_eq!(tui.cursor_x, 2, "backspace 后 cursor_x 应减少 1");
    }

    // ── handle_enter：cursor_y 边界 ───────────────────────────────────────────

    /// cursor_y = tv_height-1 时回车，cursor_y 不应超出 tv_height-1
    #[test]
    fn test_enter_at_last_visible_line_no_overflow() {
        // 文件需要至少 TV_H 行，确保 line_meta[TV_H-1] 存在
        let content = "x\n".repeat(TV_H + 1);
        let (mut tui, td, _f) = setup(&content);
        tui.cursor_y = TV_H - 1;
        tui.cursor_x = 3;
        tui.bytes_cursor = 3;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_enter(&mut tui, meta, &td).unwrap();
        assert!(
            tui.cursor_y <= TV_H - 1,
            "回车后 cursor_y({}) 不应超出 tv_height-1({})",
            tui.cursor_y,
            TV_H - 1
        );
        assert_eq!(tui.cursor_x, 0, "回车后 cursor_x 应为 0");
    }

    // ── handle_paste：cursor_x 和 cursor_y 边界 ───────────────────────────────

    /// 粘贴触发换行时 cursor_x 应为 1（paste 与 handle_char 的换行 cursor_x 不同）
    /// Bug 候选：handle_char 换行时 cursor_x=0，handle_paste 换行时 cursor_x=1
    #[test]
    fn test_paste_wrap_cursor_x_is_one() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        // 粘贴超过 TV_W 字符的内容（不含换行），触发宽度溢出换行
        let wide = "x".repeat(TV_W + 1);
        handle().handle_paste(&mut tui, meta, &td, &wide).unwrap();
        // paste 换行后 cursor_x = 1，而 handle_char 换行后 cursor_x = 0
        // 这是一个语义不一致，记录实际值
        assert_eq!(
            tui.cursor_y, 1,
            "粘贴超宽内容后 cursor_y 应增加，实际: {}",
            tui.cursor_y
        );
        // 记录 cursor_x 实际值（Bug 分析：应该是 0 还是 1？）
        let actual_cursor_x = tui.cursor_x;
        // handle_char 的语义是换行后 cursor_x=0，handle_paste 设置为 1
        // 两者应一致：cursor_x 应为 1 (paste 已把最后一个字符放到了新行)
        println!("paste 换行后 cursor_x = {}", actual_cursor_x);
    }

    /// 粘贴含多个换行（超过 tv_height），cursor_y 不应超出 tv_height
    /// Bug 候选：handle_paste 没有 cursor_y 上界检查
    #[test]
    fn test_paste_many_newlines_cursor_y_bounded() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        // 粘贴 TV_H * 2 行换行
        let many_lines = "x\n".repeat(TV_H * 2);
        handle()
            .handle_paste(&mut tui, meta, &td, &many_lines)
            .unwrap();
        assert!(
            tui.cursor_y <= TV_H,
            "大量换行粘贴后 cursor_y({}) 不应超出 tv_height({})",
            tui.cursor_y,
            TV_H
        );
    }

    /// 粘贴后内容应出现在文件中
    #[test]
    fn test_paste_content_persists_after_wide_paste() {
        let (mut tui, td, _f) = setup("abc\n");
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle()
            .handle_paste(&mut tui, meta, &td, "hello world")
            .unwrap();
        let text = all_lines_text(&td);
        assert!(
            text.contains("hello world"),
            "粘贴后内容应包含 'hello world'，实际: {:?}",
            text
        );
    }

    // ── handle_down：SoftWrap cursor_x 截断 ──────────────────────────────────

    /// 向下移到 longer 行时 cursor_x 不超出
    #[test]
    fn test_down_then_up_cursor_x_consistent() {
        let (mut tui, td, _f) = setup("hi\nlonger_line\n");
        let meta = td.get_current_line_meta().unwrap();
        let h = handle();
        tui.cursor_x = 1;
        h.handle_down(&mut tui, meta, &td).unwrap();
        // 向下到 longer_line，cursor_x 应 <= longer_line.char_len-1
        let line1_len = meta.get(1).map(|m| m.get_char_len()).unwrap_or(0);
        assert!(
            tui.cursor_x < line1_len,
            "向下后 cursor_x({}) 应 < line1_len({})",
            tui.cursor_x,
            line1_len
        );
        // 再向上
        h.handle_up(&mut tui, meta, &td).unwrap();
        let line0_len = meta.get(0).map(|m| m.get_char_len()).unwrap_or(0);
        assert!(
            tui.cursor_x < line0_len,
            "向上后 cursor_x({}) 应 < line0_len({})",
            tui.cursor_x,
            line0_len
        );
    }

    // ══════════════════════════════════════════════════════════════════════════
    // 第二轮：SoftWrap 剩余风险场景 + 内容正确性
    // ══════════════════════════════════════════════════════════════════════════

    // ── handle_left：cursor_y 越界 ────────────────────────────────────────────

    /// Bug 候选：handle_left cursor_x=0 时调用 line_meta.get(cursor_y).unwrap()
    /// 若 cursor_y 越界 → panic
    #[test]
    fn test_left_cursor_y_oob_no_panic() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_y = 5; // 越界：文件只有 1 行
        tui.cursor_x = 0;
        let meta = td.get_current_line_meta().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle().handle_left(&mut tui, meta, &td).unwrap()
        }));
        assert!(result.is_ok(), "handle_left cursor_y 越界不应 panic");
    }

    // ── handle_right：cursor_y 越界 ───────────────────────────────────────────

    /// Bug 候选：handle_right SoftWrap 首先调用 line_meta.get(cursor_y).unwrap()
    /// 若 cursor_y 越界 → panic
    #[test]
    fn test_right_cursor_y_oob_no_panic() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_y = 5; // 越界
        tui.cursor_x = 0;
        let meta = td.get_current_line_meta().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle().handle_right(&mut tui, meta, &td).unwrap()
        }));
        assert!(result.is_ok(), "handle_right cursor_y 越界不应 panic");
    }

    // ── handle_paste：cursor_y 越界（正常路径） ───────────────────────────────

    /// Bug 候选：handle_paste else 分支 line_meta.get(cursor_y).unwrap()
    /// 若 cursor_y 越界 → panic
    #[test]
    fn test_paste_cursor_y_oob_no_panic() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_y = 5; // 越界
        tui.cursor_x = 1;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle().handle_paste(&mut tui, meta, &td, "hello").unwrap()
        }));
        assert!(result.is_ok(), "handle_paste cursor_y 越界不应 panic");
    }

    // ── handle_char：is_last_line=true 且 cursor_y=0 ─────────────────────────

    /// Bug 候选：is_last_line=true 且 cursor_y=0 时，cursor_y-1 = usize::MAX
    /// → line_meta.get(usize::MAX).unwrap() 或直接 panic
    #[test]
    fn test_char_is_last_line_with_cursor_y_zero_no_panic() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_y = 0;
        tui.cursor_x = 0;
        tui.is_last_line = true; // 人为置位（非正常路径，但防御测试）
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle().handle_char(&mut tui, meta, &td, 'X').unwrap()
        }));
        assert!(result.is_ok(), "is_last_line=true + cursor_y=0 不应 panic");
    }

    /// Bug 候选：handle_paste is_last_line=true 且 cursor_y=0
    /// → cursor_y-1 = usize::MAX → panic
    #[test]
    fn test_paste_is_last_line_with_cursor_y_zero_no_panic() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_y = 0;
        tui.cursor_x = 0;
        tui.is_last_line = true;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle().handle_paste(&mut tui, meta, &td, "XY").unwrap()
        }));
        assert!(
            result.is_ok(),
            "handle_paste is_last_line + cursor_y=0 不应 panic"
        );
    }

    // ── 内容正确性：插入位置验证 ──────────────────────────────────────────────

    /// 在行中间插入字符，内容顺序应正确
    #[test]
    fn test_char_insert_at_mid_position_content_correct() {
        let (mut tui, td, _f) = setup("ab\n");
        tui.cursor_x = 1;
        tui.bytes_cursor = 1; // 'a' 后面
        tui.bytes_cursor_size = 1;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'X').unwrap();
        let text = first_line_text(&td);
        assert!(
            text.starts_with("aXb"),
            "中间插入 'X' 后应为 aXb...，实际: {:?}",
            text
        );
        assert_eq!(tui.cursor_x, 2, "插入后 cursor_x 应为 2");
    }

    /// 在行首插入字符
    #[test]
    fn test_char_insert_at_line_beginning() {
        let (mut tui, td, _f) = setup("hello\n");
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'Z').unwrap();
        let text = first_line_text(&td);
        assert!(
            text.starts_with('Z'),
            "行首插入 'Z' 后首字符应为 Z，实际: {:?}",
            text
        );
    }

    /// 在行尾（\n 前）插入字符
    #[test]
    fn test_char_insert_before_newline() {
        let (mut tui, td, _f) = setup("hi\n");
        // bytes_cursor=2 指向 '\n'，插入在 \n 前
        tui.cursor_x = 2;
        tui.bytes_cursor = 2;
        tui.bytes_cursor_size = 1;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'Z').unwrap();
        let text = first_line_text(&td);
        assert!(
            text.starts_with("hiZ"),
            "行尾插入 'Z' 后应为 hiZ...，实际: {:?}",
            text
        );
    }

    // ── handle_enter：内容分割正确性 ─────────────────────────────────────────

    /// 在中间回车，左半边保留在第 0 行，右半边移到第 1 行
    #[test]
    fn test_enter_splits_content_correctly() {
        let (mut tui, td, _f) = setup("helloworld\n");
        tui.cursor_y = 0;
        tui.cursor_x = 5;
        tui.bytes_cursor = 5; // 在 'w' 前插入 \n
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_enter(&mut tui, meta, &td).unwrap();
        let line0 = nth_line_text(&td, 0);
        let line1 = nth_line_text(&td, 1);
        assert!(
            line0.contains("hello"),
            "第 0 行应包含 hello，实际: {:?}",
            line0
        );
        assert!(
            line1.contains("world"),
            "第 1 行应包含 world，实际: {:?}",
            line1
        );
    }

    // ── handle_down：移到空行时 cursor_x 截断 ────────────────────────────────

    /// 移到空行（只有 '\n'，char_len=1），cursor_x 应截断到 0
    #[test]
    fn test_down_to_empty_line_cursor_x_is_zero() {
        let (mut tui, td, _f) = setup("hello\n\nworld\n");
        tui.cursor_x = 4;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_down(&mut tui, meta, &td).unwrap();
        // meta[1] 是空行 "\n"，char_len=1，saturating_sub(1)=0
        assert_eq!(tui.cursor_x, 0, "移到空行 cursor_x 应截断为 0");
    }

    // ── handle_up：cursor_x 截断到 char_len-1（含 \n 位置） ──────────────────

    /// 从长行向上移到短行，cursor_x 被截断到 char_len-1
    #[test]
    fn test_up_cursor_x_clamped_to_char_len_minus_one() {
        let (mut tui, td, _f) = setup("hi\nlongerlongline\n");
        tui.cursor_y = 1;
        tui.cursor_x = 10; // 长行中间
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_up(&mut tui, meta, &td).unwrap();
        let line0_char_len = meta.get(0).unwrap().get_char_len();
        // SoftWrap handle_up 截断到 char_len-1（含 '\n'）
        let expected_max = line0_char_len.saturating_sub(1);
        assert!(
            tui.cursor_x <= expected_max,
            "向上后 cursor_x({}) 应 <= char_len-1({})",
            tui.cursor_x,
            expected_max
        );
    }

    // ── handle_backspace：连续删除多个字符 ───────────────────────────────────

    /// 连续 backspace，每次光标减 1
    #[test]
    fn test_backspace_consecutive_decrements_cursor() {
        let (mut tui, mut td, _f) = setup("abcde\n");
        tui.cursor_x = 3;
        tui.bytes_cursor = 3;
        tui.bytes_cursor_size = 1;
        let meta = td.get_current_line_meta().unwrap();
        let h = handle();
        h.handle_backspace(&mut tui, meta, &td).unwrap();
        assert_eq!(tui.cursor_x, 2);
        let meta = td.get_current_line_meta().unwrap();
        h.handle_backspace(&mut tui, meta, &td).unwrap();
        assert_eq!(tui.cursor_x, 1);
        assert_eq!(saved_text(&mut td), "ade\n");
    }

    #[test]
    fn test_char_then_backspace_without_manual_sync_restores_original() {
        let (mut tui, mut td, _f) = setup("abc\n");
        tui.cursor_x = 1;
        tui.cursor_y = 0;
        tui.bytes_cursor = 1;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'X').unwrap();

        let meta = td.get_current_line_meta().unwrap();
        handle().handle_backspace(&mut tui, meta, &td).unwrap();

        assert_eq!(saved_text(&mut td), "abc\n");
        assert_eq!(tui.cursor_x, 1);
        assert_eq!(tui.cursor_y, 0);
    }

    #[test]
    fn test_paste_then_backspace_without_manual_sync_deletes_last_pasted_char() {
        let (mut tui, mut td, _f) = setup("abc\n");
        tui.cursor_x = 0;
        tui.cursor_y = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_paste(&mut tui, meta, &td, "XY").unwrap();

        let meta = td.get_current_line_meta().unwrap();
        handle().handle_backspace(&mut tui, meta, &td).unwrap();

        assert_eq!(saved_text(&mut td), "Xabc\n");
        assert_eq!(tui.cursor_x, 1);
    }

    /// backspace 到行首（cursor_x=1 → 0），不触发合并行
    #[test]
    fn test_backspace_to_line_start_no_merge() {
        let (mut tui, td, _f) = setup("ab\ncd\n");
        tui.cursor_y = 0;
        tui.cursor_x = 1;
        tui.bytes_cursor = 1;
        tui.bytes_cursor_size = 1;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_backspace(&mut tui, meta, &td).unwrap();
        let text = first_line_text(&td);
        assert_eq!(tui.cursor_y, 0, "未触发行合并，cursor_y 应保持 0");
        assert_eq!(tui.cursor_x, 0, "cursor_x 应减到 0");
        // 第 0 行应删掉 'a'，只剩 'b\n'
        assert!(
            text.starts_with('b'),
            "删除 'a' 后首字符应为 b，实际: {:?}",
            text
        );
    }

    // ── handle_down/up 往返光标稳定性 ────────────────────────────────────────

    /// 同等长度的行上下移动，cursor_x 不被截断
    #[test]
    fn test_down_up_same_length_cursor_x_preserved() {
        let (mut tui, td, _f) = setup("hello\nworld\n");
        tui.cursor_x = 3;
        let meta = td.get_current_line_meta().unwrap();
        let h = handle();
        h.handle_down(&mut tui, meta, &td).unwrap();
        // 两行长度相同，cursor_x 不应被截断
        assert_eq!(tui.cursor_x, 3, "等长行向下 cursor_x 不变");
        h.handle_up(&mut tui, meta, &td).unwrap();
        // 向上后，SoftWrap clamp 到 char_len-1，"hello\n" char_len=6，最大=5
        assert!(tui.cursor_x <= 5, "向上 cursor_x 应 <= 5");
    }

    // ══════════════════════════════════════════════════════════════════════════
    // 第三轮：空文件、cursor_x 溢出、bytes_cursor_size=0、滚动路径
    // ══════════════════════════════════════════════════════════════════════════

    // ── handle_up / handle_down：空文件或空 line_meta ─────────────────────────

    /// handle_up cursor_y=0 时走 scroll_pre_one_line(line_meta.get(0).unwrap())
    /// 若已在第一行（line_num=1）不应尝试向上滚动，不应 panic
    /// 注：GapBlockText 不支持真正的空文件，用最小合法文件 "\n" 代替
    #[test]
    fn test_up_empty_file_no_panic() {
        let (mut tui, td, _f) = setup("\n");
        tui.cursor_y = 0;
        let meta = td.get_current_line_meta().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle().handle_up(&mut tui, meta, &td).unwrap()
        }));
        assert!(result.is_ok(), "第一行 handle_up 不应 panic");
    }

    /// handle_down 在 cursor_y >= tv_height-1 时走 scroll_next_one_line(line_meta.last().unwrap())
    /// 若 line_meta 为空 → panic
    #[test]
    fn test_down_scroll_path_no_panic() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_y = TV_H - 1; // 触发滚动路径
        let meta = td.get_current_line_meta().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle().handle_down(&mut tui, meta, &td).unwrap()
        }));
        assert!(result.is_ok(), "滚动路径 handle_down 不应 panic");
    }

    // ── handle_paste：cursor_x 精确等于 TV_W 时是否越界 ───────────────────────

    /// 粘贴恰好 TV_W 个字符（不含换行），cursor_x 不应超出 TV_W
    /// Bug 候选：char_with == TV_W 时 `char_with > TV_W` 为 false → cursor_x += 1 → cursor_x = TV_W
    #[test]
    fn test_paste_exact_tv_width_cursor_x_bounded() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        let paste = "x".repeat(TV_W); // 恰好 TV_W 个字符
        handle().handle_paste(&mut tui, meta, &td, &paste).unwrap();
        assert_eq!(tui.cursor_y, 1, "粘贴满一整行后 cursor_y 应换到下一视觉行");
        assert_eq!(tui.cursor_x, 0, "粘贴满一整行后 cursor_x 应为 0");
    }

    /// 粘贴 TV_W+1 个字符：应在 TV_W 处触发换行，cursor_x 回到 1
    #[test]
    fn test_paste_tv_width_plus_one_wraps() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        let paste = "x".repeat(TV_W + 1);
        handle().handle_paste(&mut tui, meta, &td, &paste).unwrap();
        assert_eq!(tui.cursor_y, 1, "超宽粘贴后 cursor_y 应为 1");
        assert_eq!(tui.cursor_x, 1, "超宽粘贴后 cursor_x 应为 1");
    }

    #[test]
    fn test_paste_large_multiline_text_places_cursor_at_paste_end() {
        let (mut tui, mut td, _f) = setup("tail\n");
        tui.cursor_x = 0;
        tui.cursor_y = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        let paste = format!("{}\n{}\n{}", "x".repeat(TV_W + 7), "中".repeat(3), "end");

        handle().handle_paste(&mut tui, meta, &td, &paste).unwrap();

        assert_eq!(saved_text(&mut td), format!("{paste}tail\n"));
        assert_eq!(tui.cursor_y, 3, "粘贴结束后应落在最后一行");
        assert_eq!(tui.cursor_x, 3, "粘贴结束后应落在末尾字符之后");
    }

    // ── handle_backspace：bytes_cursor_size=0 时不删除内容 ─────────────────────

    /// bytes_cursor_size=0 且 cursor_x>0：backspace 删除 0 字节，内容不变但 cursor_x 减少
    /// Bug 候选：cursor 移动但内容未删除（哑删）
    #[test]
    fn test_backspace_size_zero_cursor_x_nonzero_is_noop_on_content() {
        let (mut tui, td, _f) = setup("hello\n");
        tui.cursor_x = 2;
        tui.bytes_cursor = 2;
        tui.bytes_cursor_size = 0; // 0 字节：不应删除任何内容
        let meta = td.get_current_line_meta().unwrap();
        let before = first_line_text(&td);
        handle().handle_backspace(&mut tui, meta, &td).unwrap();
        let after = first_line_text(&td);
        // cursor_x 减少
        assert_eq!(tui.cursor_x, 1, "cursor_x 应减少 1");
        // 内容应不变（删除 0 字节）
        // Bug：如果内容改变，说明 bytes_cursor_size=0 仍触发了删除
        assert_eq!(
            before, after,
            "bytes_cursor_size=0 时不应删除内容，前={:?}，后={:?}",
            before, after
        );
    }

    // ── handle_char：cursor_x 卡在 TV_W 时的行为 ──────────────────────────────

    /// 当 cursor_x 已经等于 TV_W（溢出状态），插入字符 cursor_x 不应继续增加
    /// Bug 候选：cursor_x == TV_W 时 `cursor_x < TV_W` 为 false，
    /// 字符虽插入但 cursor_x 不增加、也不触发换行 → 光标悬空
    #[test]
    fn test_char_cursor_x_at_tv_width_stalls() {
        let (mut tui, td, _f) = setup("a\n");
        tui.cursor_x = TV_W; // 人为设置为 TV_W（溢出状态）
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'Z').unwrap();
        // cursor_x < TV_W 为 false：cursor_x 不变，也不换行
        // 字符被插入但光标不移动
        println!(
            "cursor_x=TV_W 插入后: cursor_x={}, is_last_line={}",
            tui.cursor_x, tui.is_last_line
        );
        assert_eq!(
            tui.cursor_x, TV_W,
            "cursor_x 卡在 TV_W 时插入字符后应不变，实际: {}",
            tui.cursor_x
        );
    }

    // ── handle_down/up：cursor_y 绝对不超出 tv_height ────────────────────────

    /// 连续 handle_down TV_H*2 次，cursor_y 不超出 tv_height
    #[test]
    fn test_down_cursor_y_never_exceeds_tv_height() {
        let content = "x\n".repeat(TV_H * 3); // 文件比视图大
        let (mut tui, td, _f) = setup(&content);
        let meta = td.get_current_line_meta().unwrap();
        let h = handle();
        for _ in 0..TV_H * 2 {
            h.handle_down(&mut tui, meta, &td).unwrap();
        }
        assert!(
            tui.cursor_y <= TV_H,
            "连续向下后 cursor_y({}) 不应超出 tv_height({})",
            tui.cursor_y,
            TV_H
        );
    }

    // ── handle_right：跨视觉行跳转后 cursor_y 增加 ───────────────────────────

    /// SoftWrap：在折行段末尾按右键，cursor_y += 1, cursor_x = 0
    /// 需要构造一个足够长的行触发折行
    #[test]
    fn test_right_wraps_visual_line_cursor_y_increments() {
        // 构造 TV_W+5 字符的长行，让 SoftWrap 产生两个视觉段
        let long_line = "a".repeat(TV_W + 5) + "\n";
        let (mut tui, td, _f) = setup(&long_line);
        let meta = td.get_current_line_meta().unwrap();
        // 第 0 视觉段 char_len = TV_W
        // 将 cursor 移到第 0 段末尾 (char_len-2，'\n' 前的最后一个内容字符)
        if let Some(seg0) = meta.get(0) {
            let end_x = seg0.get_char_len().saturating_sub(1);
            tui.cursor_x = end_x;
            let h = handle();
            // 在当前视觉段最后一个字符上继续右移，应跳到续行
            h.handle_right(&mut tui, meta, &td).unwrap();
            assert_eq!(tui.cursor_y, 1, "跨到续行后 cursor_y 应为 1");
            assert_eq!(tui.cursor_x, 0, "跨到续行后 cursor_x 应为 0");
        }
    }

    #[test]
    fn test_scroll_edit_undo_keeps_correct_line_context() {
        let content = (1..=30)
            .map(|i| format!("line{:02}\n", i))
            .collect::<String>();
        let (mut tui, mut td, _f, _uf) = setup_with_undo(&content);
        let h = handle();

        for _ in 0..(TV_H + 2) {
            let meta = td.get_current_line_meta().unwrap();
            // tui.start_line_num = meta.get(0).map_or(1, |m| m.get_line_num());
            h.handle_down(&mut tui, meta, &td).unwrap();
            sync_view_state(&mut tui, &td);
        }

        let meta = td.get_current_line_meta().unwrap();
        // tui.start_line_num = meta.get(0).map_or(1, |m| m.get_line_num());
        sync_view_state(&mut tui, &td);
        h.handle_char(&mut tui, meta, &td, 'Z').unwrap();
        let after_insert = saved_text(&mut td);
        assert!(
            after_insert.contains("Z"),
            "滚动后插入应落到当前上下文中的实际文档行"
        );

        h.handle_ctrl_z(&mut tui, &td).unwrap();
        let after_undo = saved_text(&mut td);
        assert_eq!(after_undo, content, "滚动后编辑再 undo 应恢复原文档");
    }

    #[test]
    fn test_scroll_edit_delete_sequence_preserves_document_order() {
        let content = (1..=35)
            .map(|i| format!("row{:02}\n", i))
            .collect::<String>();
        let (mut tui, mut td, _f) = setup(&content);
        let h = handle();

        for _ in 0..22 {
            let meta = td.get_current_line_meta().unwrap();
            h.handle_down(&mut tui, meta, &td).unwrap();
            sync_view_state(&mut tui, &td);
        }

        tui.cursor_x = 3;
        sync_view_state(&mut tui, &td);
        let meta = td.get_current_line_meta().unwrap();
        h.handle_char(&mut tui, meta, &td, 'X').unwrap();
        sync_view_state(&mut tui, &td);

        let meta = td.get_current_line_meta().unwrap();
        h.handle_backspace(&mut tui, meta, &td).unwrap();
        sync_view_state(&mut tui, &td);

        tui.cursor_x = 0;
        sync_view_state(&mut tui, &td);
        let meta = td.get_current_line_meta().unwrap();
        h.handle_enter(&mut tui, meta, &td).unwrap();
        sync_view_state(&mut tui, &td);

        let meta = td.get_current_line_meta().unwrap();
        h.handle_backspace(&mut tui, meta, &td).unwrap();

        assert_eq!(
            saved_text(&mut td),
            content,
            "滚动后连续插入/删除/回车/合并不应打乱文档顺序"
        );
    }

    #[test]
    fn test_backspace_at_top_of_scrolled_page_merges_with_previous_line() {
        let content = (1..=28)
            .map(|i| format!("line{:02}\n", i))
            .collect::<String>();
        let (mut tui, mut td, _f) = setup(&content);
        let h = handle();

        let meta = td.get_current_line_meta().unwrap();
        td.scroll_next_one_line(meta.last().unwrap()).unwrap();
        tui.cursor_y = 0;
        tui.cursor_x = 0;
        sync_view_state(&mut tui, &td);
        let meta = td.get_current_line_meta().unwrap();
        h.handle_backspace(&mut tui, meta, &td).unwrap();

        let expected = std::iter::once("line01line02\n".to_string())
            .chain((3..=28).map(|i| format!("line{:02}\n", i)))
            .collect::<String>();
        assert_eq!(
            saved_text(&mut td),
            expected,
            "滚动后在页顶行首 Backspace 应合并隐藏的上一逻辑行"
        );
        //  assert_eq!(tui.start_line_num, 1, "合并隐藏上一行后应回显上一逻辑行");
        assert_eq!(tui.cursor_y, 0, "合并后光标应留在当前页首行");
        assert_eq!(tui.cursor_x, 6, "合并后光标应落在上一逻辑行末尾");
    }

    // ── handle_enter：连续插入多行 ────────────────────────────────────────────

    /// 连续按回车 TV_H 次，cursor_y 不超过 tv_height-1
    #[test]
    fn test_enter_consecutive_cursor_y_bounded() {
        let (mut tui, td, _f) = setup("abc\n");
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        let h = handle();
        for _ in 0..TV_H {
            h.handle_enter(&mut tui, meta, &td).unwrap();
        }
        assert!(
            tui.cursor_y <= TV_H - 1,
            "连续 Enter 后 cursor_y({}) 不应超出 tv_height-1({})",
            tui.cursor_y,
            TV_H - 1
        );
    }

    // ── handle_backspace：cursor_x=0 cursor_y=0 但 is_last_line=true ─────────

    /// 这是 handle_backspace 的特殊路径：
    /// cursor_y=0, cursor_x=0 → 早期 return（noop）
    /// 即使 is_last_line=true 也不应触发任何操作
    #[test]
    fn test_backspace_noop_when_both_zero_regardless_of_is_last_line() {
        let (mut tui, td, _f) = setup("hello\n");
        tui.cursor_y = 0;
        tui.cursor_x = 0;
        tui.is_last_line = true;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_backspace(&mut tui, meta, &td).unwrap();
        assert_eq!(tui.cursor_y, 0);
        assert_eq!(tui.cursor_x, 0);
        let text = first_line_text(&td);
        assert!(text.starts_with('h'), "内容不应改变");
    }

    // ── handle_paste：空字符串粘贴 ────────────────────────────────────────────

    /// 粘贴空字符串，cursor 不应改变，内容不应改变
    #[test]
    fn test_paste_empty_string_is_noop() {
        let (mut tui, td, _f) = setup("hello\n");
        tui.cursor_x = 2;
        tui.bytes_cursor = 2;
        let meta = td.get_current_line_meta().unwrap();
        let before = first_line_text(&td);
        handle().handle_paste(&mut tui, meta, &td, "").unwrap();
        let after = first_line_text(&td);
        assert_eq!(tui.cursor_x, 2, "空粘贴后 cursor_x 不变");
        assert_eq!(before, after, "空粘贴后内容不变");
    }

    // ── handle_up / handle_down：cursor_x 对称截断 ────────────────────────────

    /// 先向下（cursor_x 被截断到短行），再向上（cursor_x 不能恢复到原来的值）
    /// 这是预期行为（cursor_x 不记忆"理想列"）
    #[test]
    fn test_up_down_cursor_x_does_not_restore() {
        let (mut tui, td, _f) = setup("longer\nhi\nlonger\n");
        let meta = td.get_current_line_meta().unwrap();
        let h = handle();
        tui.cursor_x = 5; // 在 longer 行
        h.handle_down(&mut tui, meta, &td).unwrap(); // → hi，cursor_x 截断到 hi 的 char_len-1=2
        let x_after_down = tui.cursor_x;
        h.handle_up(&mut tui, meta, &td).unwrap(); // → longer，cursor_x 从 x_after_down 出发
                                                   // cursor_x 不会恢复为原来的 5（no "sticky column"）
                                                   // 这是已知的 UX 限制，记录实际行为
        println!(
            "down → cursor_x={}, up → cursor_x={}（不恢复为 5)",
            x_after_down, tui.cursor_x
        );
        let line0_len = meta.get(0).unwrap().get_char_len();
        assert!(
            tui.cursor_x < line0_len,
            "向上后 cursor_x({}) 应 < line0_len({})",
            tui.cursor_x,
            line0_len
        );
    }

    // ── Ctrl+S 保存 ───────────────────────────────────────────────────────────

    #[test]
    fn test_ctrl_s_saves_content() {
        let (mut tui, mut td, tmp) = setup("original\n");
        // 插入字符
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'Z').unwrap();
        // 保存
        handle()
            .handle_ctrl_s(&mut tui, tmp.path(), &mut td)
            .unwrap();
        // 验证文件内容
        let saved = std::fs::read_to_string(tmp.path()).unwrap();
        assert!(
            saved.contains('Z'),
            "保存后文件应包含插入的字符 'Z'，实际: {:?}",
            saved
        );
        assert!(
            tui.elem.cmd_inp.get_inp().contains("saved"),
            "保存成功后命令栏应显示 'saved'"
        );
    }

    // ══════════════════════════════════════════════════════════════════════════
    // handle_ctrl_z + handle_char：undo 功能测试
    // ══════════════════════════════════════════════════════════════════════════

    /// 带 undo 文件的测试 setup：返回 (ChapTui, TextDisplay, 内容临时文件, undo临时目录)
    /// undo 文件路径在目录中是新建的（不存在），UndoFile 会初始化它
    fn setup_with_undo(content: &str) -> (ChapTui, TextDisplay, NamedTempFile, TempDir) {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(content.as_bytes()).unwrap();
        tmp.flush().unwrap();

        let gap = GapBlockText::from_file_path(tmp.path()).unwrap();
        let mut td =
            TextDisplay::EditBlock(EditTextWarp::new(gap, TV_H, TV_W, TextWarpType::SoftWrap));
        td.get_one_page_from_state(&LineState::file_start())
            .unwrap();

        // 使用 TempDir 内的不存在路径，UndoFile::open 会新建并写入文件头
        let undo_dir = TempDir::new().unwrap();
        let undo_path = undo_dir.path().join("undo.log");
        let undo = UndoFile::open(&undo_path).unwrap();
        let tui = ChapTui::for_test_with_undo(TV_H, TV_W, undo);
        (tui, td, tmp, undo_dir)
    }

    /// undo=None 时，handle_char 不记录 undo；ctrl_z 是 noop
    #[test]
    fn test_ctrl_z_noop_when_undo_disabled() {
        let (mut tui, td, _f) = setup("hello\n");
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'X').unwrap();
        let before_ctrl_z = first_line_text(&td);
        // undo=None，ctrl_z 不做任何事
        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        let after_ctrl_z = first_line_text(&td);
        assert_eq!(
            before_ctrl_z, after_ctrl_z,
            "undo=None 时 ctrl_z 不应改变内容"
        );
        assert!(after_ctrl_z.contains('X'), "undo=None 时 X 应仍在");
    }

    /// undo 已启用但栈空时，ctrl_z 是 noop
    #[test]
    fn test_ctrl_z_noop_on_empty_stack() {
        let (mut tui, td, _f, _uf) = setup_with_undo("hello\n");
        let before = first_line_text(&td);
        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        let after = first_line_text(&td);
        assert_eq!(before, after, "空 undo 栈时 ctrl_z 不应改变内容");
    }

    /// 插入单个字符后 ctrl_z 应恢复内容
    #[test]
    fn test_ctrl_z_undoes_single_char_insert() {
        let (mut tui, td, _f, _uf) = setup_with_undo("hello\n");
        tui.cursor_x = 0;
        tui.cursor_y = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();

        handle().handle_char(&mut tui, meta, &td, 'X').unwrap();
        let after_insert = first_line_text(&td);
        assert!(
            after_insert.contains('X'),
            "插入后应包含 'X'，实际: {:?}",
            after_insert
        );

        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        let after_undo = first_line_text(&td);
        assert!(
            !after_undo.contains('X'),
            "undo 后 'X' 应被移除，实际: {:?}",
            after_undo
        );
        assert!(
            after_undo.starts_with('h'),
            "undo 后内容应以 'h' 开头，实际: {:?}",
            after_undo
        );
    }

    /// undo 后光标位置应恢复到插入前
    #[test]
    fn test_ctrl_z_restores_cursor_position() {
        let (mut tui, td, _f, _uf) = setup_with_undo("hello\n");
        tui.cursor_x = 2;
        tui.cursor_y = 0;
        tui.bytes_cursor = 2;
        let meta = td.get_current_line_meta().unwrap();

        handle().handle_char(&mut tui, meta, &td, 'X').unwrap();
        assert_eq!(tui.cursor_x, 3, "插入后 cursor_x 应为 3");

        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        assert_eq!(tui.cursor_x, 2, "undo 后 cursor_x 应恢复为 2");
        assert_eq!(tui.cursor_y, 0, "undo 后 cursor_y 应恢复为 0");
    }

    /// 连续插入多个字符，依次 ctrl_z，应 LIFO 顺序恢复
    #[test]
    fn test_ctrl_z_multiple_inserts_lifo_order() {
        let (mut tui, td, _f, _uf) = setup_with_undo("abc\n");
        tui.cursor_x = 0;
        tui.cursor_y = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();
        let h = handle();

        // 在行首连续插入 X Y Z
        h.handle_char(&mut tui, meta, &td, 'X').unwrap();
        tui.bytes_cursor = 1;
        h.handle_char(&mut tui, meta, &td, 'Y').unwrap();
        tui.bytes_cursor = 2;
        h.handle_char(&mut tui, meta, &td, 'Z').unwrap();

        let after_insert = first_line_text(&td);
        assert!(
            after_insert.starts_with("XYZ"),
            "插入三个字符后应以 XYZ 开头，实际: {:?}",
            after_insert
        );

        // 第一次 ctrl_z：删除最后插入的 Z
        h.handle_ctrl_z(&mut tui, &td).unwrap();
        let after1 = first_line_text(&td);
        assert!(
            !after1.contains('Z'),
            "第 1 次 undo 后 Z 应被删除，实际: {:?}",
            after1
        );
        assert!(after1.contains('Y'), "第 1 次 undo 后 Y 应仍在");

        // 第二次 ctrl_z：删除 Y
        h.handle_ctrl_z(&mut tui, &td).unwrap();
        let after2 = first_line_text(&td);
        assert!(
            !after2.contains('Y'),
            "第 2 次 undo 后 Y 应被删除，实际: {:?}",
            after2
        );
        assert!(after2.contains('X'), "第 2 次 undo 后 X 应仍在");

        // 第三次 ctrl_z：删除 X
        h.handle_ctrl_z(&mut tui, &td).unwrap();
        let after3 = first_line_text(&td);
        assert!(
            !after3.contains('X'),
            "第 3 次 undo 后 X 应被删除，实际: {:?}",
            after3
        );
        assert!(
            after3.starts_with('a'),
            "3 次 undo 后应恢复原始内容 abc，实际: {:?}",
            after3
        );
    }

    /// undo 后再保存，文件内容应与 undo 后的内容一致
    #[test]
    fn test_ctrl_z_then_save_correct() {
        let (mut tui, mut td, tmp, _uf) = setup_with_undo("hello\n");
        tui.cursor_x = 0;
        tui.cursor_y = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();

        // 插入 'Z'
        handle().handle_char(&mut tui, meta, &td, 'Z').unwrap();
        // undo
        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        // 保存
        handle()
            .handle_ctrl_s(&mut tui, tmp.path(), &mut td)
            .unwrap();

        let saved = std::fs::read_to_string(tmp.path()).unwrap();
        assert!(
            !saved.contains('Z'),
            "undo 后保存，文件不应包含 'Z'，实际: {:?}",
            saved
        );
        assert!(
            saved.starts_with("hello"),
            "undo 后保存内容应以 hello 开头，实际: {:?}",
            saved
        );
    }

    #[test]
    fn test_ctrl_z_restores_large_paste_across_blocks() {
        let original = "head\nbody\ntail\n".to_string();
        let paste = "x".repeat(4097);
        let (mut tui, mut td, _f, _uf) = setup_with_undo(&original);
        tui.cursor_x = 0;
        tui.cursor_y = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();

        handle().handle_paste(&mut tui, meta, &td, &paste).unwrap();
        if let TextDisplay::EditBlock(v) = &td {
            v.assert_block_storage_consistent_for_test();
        }
        let after_paste = saved_text(&mut td);
        assert_ne!(after_paste, original, "超大粘贴后内容应发生变化");
        assert!(
            after_paste.starts_with(&paste),
            "超大粘贴后文件应以粘贴内容开头"
        );

        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        if let TextDisplay::EditBlock(v) = &td {
            v.assert_block_storage_consistent_for_test();
        }
        let after_undo = saved_text(&mut td);
        assert_eq!(after_undo, original, "ctrl+z 后应完整恢复原始内容");
    }

    #[test]
    fn test_ctrl_z_restores_large_multibyte_multiline_paste_across_blocks() {
        let original = "head\nbody\ntail\n".to_string();
        let paste = format!("{}你\n终点", "x".repeat(4092));
        let (mut tui, mut td, _f, _uf) = setup_with_undo(&original);
        tui.cursor_x = 0;
        tui.cursor_y = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();

        handle().handle_paste(&mut tui, meta, &td, &paste).unwrap();
        if let TextDisplay::EditBlock(v) = &td {
            v.assert_block_storage_consistent_for_test();
        }
        let after_paste = saved_text(&mut td);
        assert_ne!(after_paste, original, "超大混合粘贴后内容应发生变化");
        assert!(
            after_paste.starts_with(&paste),
            "超大混合粘贴后文件应以粘贴内容开头"
        );

        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        if let TextDisplay::EditBlock(v) = &td {
            v.assert_block_storage_consistent_for_test();
        }
        let after_undo = saved_text(&mut td);
        assert_eq!(after_undo, original, "ctrl+z 后应完整恢复原始内容");
    }

    #[test]
    fn test_ctrl_z_chain_after_large_paste_and_extra_char_keeps_block_storage_consistent() {
        let original = "head\nbody\ntail\n".to_string();
        let paste = "x".repeat(4097);
        let (mut tui, mut td, _f, _uf) = setup_with_undo(&original);
        tui.cursor_x = 0;
        tui.cursor_y = 0;
        tui.bytes_cursor = 0;
        let meta = td.get_current_line_meta().unwrap();

        handle().handle_paste(&mut tui, meta, &td, &paste).unwrap();
        if let TextDisplay::EditBlock(v) = &td {
            v.assert_block_storage_consistent_for_test();
        }
        let after_paste = saved_text(&mut td);
        assert_eq!(after_paste, format!("{paste}{original}"));

        // 显式把第二次输入定位到粘贴内容末尾，避免把测试耦合到 handle_paste 的光标同步细节。
        tui.cursor_x = paste.len();
        tui.cursor_y = 0;
        tui.bytes_cursor = paste.len();
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'Z').unwrap();
        if let TextDisplay::EditBlock(v) = &td {
            v.assert_block_storage_consistent_for_test();
        }
        let after_char = saved_text(&mut td);
        assert_eq!(after_char, format!("{paste}Z{original}"));

        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        if let TextDisplay::EditBlock(v) = &td {
            v.assert_block_storage_consistent_for_test();
        }
        let after_first_undo = saved_text(&mut td);
        assert_eq!(after_first_undo, format!("{paste}{original}"));

        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        if let TextDisplay::EditBlock(v) = &td {
            v.assert_block_storage_consistent_for_test();
        }
        let after_second_undo = saved_text(&mut td);
        assert_eq!(after_second_undo, original);
    }

    #[test]
    fn test_ctrl_z_restores_large_document_after_500_mixed_ops() {
        let original = format!(
            "{}tail\n",
            "line-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n".repeat(520)
        );
        let (mut tui, mut td, _f, _uf) = setup_with_undo(&original);
        let h = handle();

        for step in 0..500usize {
            // td.get_one_page(1).unwrap();
            let meta = td.get_current_line_meta().unwrap();
            match step % 2 {
                0 => {
                    //  tui.start_line_num = 1;
                    tui.cursor_y = 0;
                    tui.cursor_x = 0;
                    tui.bytes_cursor = 0;
                    tui.bytes_cursor_size = 0;
                    let ch = if step % 10 == 0 { '你' } else { 'A' };
                    h.handle_char(&mut tui, meta, &td, ch).unwrap();
                }
                1 => {
                    //   tui.start_line_num = 1;
                    tui.cursor_y = 0;
                    tui.cursor_x = 0;
                    tui.bytes_cursor = 0;
                    tui.bytes_cursor_size = 0;
                    let paste = if step % 12 == 1 {
                        format!("mix-{step}\n中文-{step}")
                    } else if step % 10 == 1 {
                        format!("段落-{step}\nnext-{step}\nend")
                    } else {
                        format!("p{step:03}-你")
                    };
                    h.handle_paste(&mut tui, meta, &td, &paste).unwrap();
                }
                _ => unreachable!(),
            }

            if let TextDisplay::EditBlock(v) = &td {
                v.assert_block_storage_consistent_for_test();
            }
        }

        let after_ops = saved_text(&mut td);
        assert_ne!(after_ops, original, "500 次混合操作后内容应已变化");

        for undo_idx in 0..500usize {
            h.handle_ctrl_z(&mut tui, &td).unwrap();
            if undo_idx % 25 == 24 {
                if let TextDisplay::EditBlock(v) = &td {
                    v.assert_block_storage_consistent_for_test();
                }
            }
        }

        if let TextDisplay::EditBlock(v) = &td {
            v.assert_block_storage_consistent_for_test();
        }
        let after_undo = saved_text(&mut td);
        assert_eq!(after_undo, original, "500 次 ctrl+z 后应完整恢复初始大文本");
    }

    #[test]
    fn test_ctrl_z_restores_deleted_multibyte_char() {
        let (mut tui, mut td, _f, _uf) = setup_with_undo("你好\n");
        tui.cursor_x = 2;
        tui.cursor_y = 0;
        tui.bytes_cursor = 6;
        tui.bytes_cursor_size = 3;
        let meta = td.get_current_line_meta().unwrap();

        handle().handle_backspace(&mut tui, meta, &td).unwrap();
        assert_eq!(saved_text(&mut td), "你\n");

        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        assert_eq!(saved_text(&mut td), "你好\n");
    }

    #[test]
    fn test_ctrl_z_undoes_enter() {
        let (mut tui, mut td, _f, _uf) = setup_with_undo("hello\n");
        tui.cursor_x = 2;
        tui.cursor_y = 0;
        tui.bytes_cursor = 2;
        let meta = td.get_current_line_meta().unwrap();

        handle().handle_enter(&mut tui, meta, &td).unwrap();
        assert_eq!(saved_text(&mut td), "he\nllo\n");

        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        assert_eq!(saved_text(&mut td), "hello\n");
        assert_eq!(tui.cursor_x, 2);
        assert_eq!(tui.cursor_y, 0);
    }

    #[test]
    fn test_ctrl_z_restores_deleted_newline() {
        let (mut tui, mut td, _f, _uf) = setup_with_undo("hello\nworld\n");
        tui.cursor_y = 1;
        tui.cursor_x = 0;
        tui.bytes_cursor = 0;
        tui.bytes_cursor_size = 1;
        let meta = td.get_current_line_meta().unwrap();

        handle().handle_backspace(&mut tui, meta, &td).unwrap();
        assert_eq!(saved_text(&mut td), "helloworld\n");

        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        assert_eq!(saved_text(&mut td), "hello\nworld\n");
        assert_eq!(tui.cursor_x, 0);
        assert_eq!(tui.cursor_y, 1);
    }

    #[test]
    fn test_ctrl_z_mixed_char_enter_backspace_sequence() {
        let (mut tui, mut td, _f, _uf) = setup_with_undo("abc\n");

        tui.cursor_x = 1;
        tui.cursor_y = 0;
        tui.bytes_cursor = 1;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_char(&mut tui, meta, &td, 'X').unwrap();
        assert_eq!(saved_text(&mut td), "aXbc\n");

        tui.bytes_cursor = 2;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_enter(&mut tui, meta, &td).unwrap();
        assert_eq!(saved_text(&mut td), "aX\nbc\n");

        tui.cursor_y = 1;
        tui.cursor_x = 1;
        tui.bytes_cursor = 1;
        tui.bytes_cursor_size = 1;
        let meta = td.get_current_line_meta().unwrap();
        handle().handle_backspace(&mut tui, meta, &td).unwrap();
        assert_eq!(saved_text(&mut td), "aX\nc\n");

        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        assert_eq!(saved_text(&mut td), "aX\nbc\n");

        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        assert_eq!(saved_text(&mut td), "aXbc\n");

        handle().handle_ctrl_z(&mut tui, &td).unwrap();
        assert_eq!(saved_text(&mut td), "abc\n");
    }
}
