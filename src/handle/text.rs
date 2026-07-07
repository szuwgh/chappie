use crate::handle::ChapResult;
use crate::handle::Handle;
use crate::handle::HandleBase;
pub struct HandleText {
    txt_base: HandleBase,
}

impl HandleText {
    pub(crate) fn new() -> Self {
        HandleText {
            txt_base: HandleBase {},
        }
    }
}

impl Handle for HandleText {
    fn handle_backspace<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a crate::common::ring_vec::RingVec<crate::textwarp::LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }
    fn handle_char<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a crate::common::ring_vec::RingVec<crate::textwarp::LineState>,
        td: &'a crate::textwarp::TextDisplay,
        c: char,
    ) -> ChapResult<()> {
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
        line_meta: &'a crate::common::ring_vec::RingVec<crate::textwarp::LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        self.txt_base.handle_down(chap_tui, line_meta, td)?;
        Ok(())
    }

    fn handle_left<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a crate::common::ring_vec::RingVec<crate::textwarp::LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        self.txt_base.handle_left(chap_tui, line_meta, td)?;
        Ok(())
    }

    fn handle_right<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a crate::common::ring_vec::RingVec<crate::textwarp::LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        self.txt_base.handle_right(chap_tui, line_meta, td)?;
        Ok(())
    }

    fn handle_up<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a crate::common::ring_vec::RingVec<crate::textwarp::LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        self.txt_base.handle_up(chap_tui, line_meta, td)?;
        Ok(())
    }

    fn handle_enter<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a crate::common::ring_vec::RingVec<crate::textwarp::LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_shift_down<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a crate::common::ring_vec::RingVec<crate::textwarp::LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_shift_left(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &crate::common::ring_vec::RingVec<crate::textwarp::LineState>,
        td: &crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_shift_right<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a crate::common::ring_vec::RingVec<crate::textwarp::LineState>,
        td: &'a crate::textwarp::TextDisplay,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn handle_shift_up<'a>(
        &self,
        chap_tui: &mut crate::tui::ChapTui,
        line_meta: &'a crate::common::ring_vec::RingVec<crate::textwarp::LineState>,
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
        line_meta: &'a crate::common::ring_vec::RingVec<crate::textwarp::LineState>,
        td: &'a crate::textwarp::TextDisplay,
        pasted_string: &str,
    ) -> ChapResult<()> {
        Ok(())
    }
}
