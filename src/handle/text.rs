use crate::handle::edit::HandleCmdInpEdit;
use crate::handle::ChapResult;
use crate::handle::Handle;
use crate::handle::HandleBase;
use crate::handle::RingVec;
use crate::textwarp::LineState;
use crate::textwarp::TextOper;
use crate::tui::ViewMode;
pub struct HandleText {
    txt_base: HandleBase,
    cmd_inp: HandleCmdInpEdit,
}

impl HandleText {
    pub(crate) fn new() -> Self {
        HandleText {
            txt_base: HandleBase {},
            cmd_inp: HandleCmdInpEdit {},
        }
    }
}

impl Handle for HandleText {
    fn handle_backspace<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        chap_tui.elem.cmd_inp.pop();
        Ok(())
    }
    fn handle_char<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a crate::textwarp::TextDisplay,
        c: char,
    ) -> ChapResult<()> {
        self.cmd_inp.handle_char(chap_tui, line_meta, td, c)?;
        Ok(())
    }

    fn handle_ctrl_r<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_down<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        self.txt_base.handle_down(chap_tui, line_meta, td)?;
        Ok(())
    }

    fn handle_left<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        self.txt_base.handle_left(chap_tui, line_meta, td)?;
        Ok(())
    }

    fn handle_right<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        self.txt_base.handle_right(chap_tui, line_meta, td)?;
        Ok(())
    }

    fn handle_up<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        self.txt_base.handle_up(chap_tui, line_meta, td)?;
        Ok(())
    }

    fn handle_enter<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        if matches!(chap_tui.view_mode, ViewMode::SearchResult) {
            let Some(store) = &chap_tui.search_result else {
                return Ok(());
            };
            let result_row = chap_tui.cursor_y;
            let Some(result_meta) = line_meta.get(result_row) else {
                return Ok(());
            };
            let entry_index = result_meta.line_index;
            let Some(entry) = store.entries.get(entry_index) else {
                return Ok(());
            };

            chap_tui.view_mode = ViewMode::Normal;
            td.get_one_page_from_state(&entry.source_state)?;
            chap_tui.start_line_state = entry.source_state.clone();
            chap_tui.cursor_y = 0;
            chap_tui.cursor_x = 0;
            return Ok(());
        }
        if let Some(cur_meta) = line_meta.get(chap_tui.cursor_y) {
            self.cmd_inp.handle_cmd_command(chap_tui, cur_meta, td)?;
        };
        Ok(())
    }

    fn handle_shift_down<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        self.cmd_inp.handle_down(chap_tui, td)?;
        Ok(())
    }

    fn handle_shift_left<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_shift_right<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_shift_up<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_ctrl_s<P: AsRef<std::path::Path>>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        p: P,
        td: &mut crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_paste<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a RingVec<LineState>,
        td: &'a crate::textwarp::TextDisplay,
        pasted_string: &str,
    ) -> ChapResult<()> {
        Ok(())
    }
}
