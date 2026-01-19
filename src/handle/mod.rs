pub(crate) mod edit;
pub(crate) mod hex;
use crate::common::error::ChapResult;
use crate::common::ring_vec::RingVec;
use crate::execute;
use crate::lua::LuaPlugin;
use crate::textwarp::LineState;
use crate::textwarp::TextDisplay;
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
}
