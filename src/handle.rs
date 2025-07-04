use crate::editor::EditLineMeta;
use crate::editor::RingVec;
use crate::editor::TextDisplay;
use crate::tui::ChapMod;
use crate::ChapTui;
use std::path::Path;

enum HandleImpl {
    Text(HandleText),
    Impl(HandleHex),
}

impl Handle for HandleImpl {
    fn handle_ctrl_c(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) {
        match self {
            HandleImpl::Text(handle_text) => {
                handle_text.handle_ctrl_c(chap_tui, line_meta, td);
            }
            HandleImpl::Impl(handle_hex) => {
                // handle_hex.handle_ctrl_c(chap_tui, line_meta, td);
                // Implement hex mode Ctrl+C handling
            }
        }
    }

    fn handle_ctrl_s<P: AsRef<Path>>(&mut self, p: P, td: &mut TextDisplay) {}
}

trait Handle {
    fn handle_esc(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) {
        chap_tui.fuzzy_inp.clear();
        chap_tui.assist_inp.clear();
        chap_tui.navi.clear();
        chap_tui.txt_sel.reset_to_start();
    }
    fn handle_ctrl_c(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    );

    fn handle_ctrl_s<P: AsRef<Path>>(&mut self, p: P, td: &mut TextDisplay);
}

struct HandleText;

impl Handle for HandleText {
    fn handle_ctrl_c(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) {
        // Implement text mode Ctrl+C handling
    }

    fn handle_ctrl_s<P: AsRef<Path>>(&mut self, p: P, td: &mut TextDisplay) {}
}

struct HandleHex;

impl Handle for HandleHex {
    fn handle_esc(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) {
        // Implement hex mode ESC handling
    }

    fn handle_ctrl_c(
        &self,
        chap_tui: &mut ChapTui,
        line_meta: &RingVec<EditLineMeta>,
        td: &TextDisplay,
    ) {
        // Implement hex mode Ctrl+C handling
    }

    fn handle_ctrl_s<P: AsRef<Path>>(&mut self, p: P, td: &mut TextDisplay) {}
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

struct HandleEdit;

struct HandleVector;
