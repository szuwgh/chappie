use crate::chap;
use crate::common::ring_vec::RingVec;
use crate::textwarp::CacheStr;
use crate::textwarp::LineParts;
use crate::textwarp::LineState;
use crate::tui::build_cursor_line;
use crate::tui::build_nav_text;
use crate::tui::char_range_to_visible;
use crate::tui::BuildContent;
use crate::tui::ChapTui;
use crate::tui::EditContext;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use utf8_iter::Utf8CharsEx;

pub(crate) struct TextBuildContent;

impl BuildContent for TextBuildContent {
    fn build_content<'a>(
        txts: &'a RingVec<CacheStr>,
        with: usize,
        line_meta: &'a RingVec<LineState>,
        cur_line: usize,
        select_line: &Option<(usize, usize)>,
        ed_ctx: &EditContext,
    ) -> super::Content<'a> {
        let mut lines = Vec::with_capacity(line_meta.len());
        let height = ed_ctx.height;
        let column_offset = ed_ctx.column_offset;
        let cursor_y = ed_ctx.cursor_y;
        let cursor_x = ed_ctx.cursor_x;
        let is_txt_model = ed_ctx.is_txt_model;
        for (i, txt) in txts.iter().enumerate() {
            let full = txt.text(0..);
            let visible =
                char_range_to_visible(full.as_parts(), column_offset, column_offset + with);
            let parts: &[&[u8]] = &visible[..];
            if cursor_y == i && is_txt_model {
                let (spans, _, _) = build_cursor_line(parts, cursor_x, &[], 0);
                lines.push(Line::from(spans));
            } else {
                let mut spans = Vec::with_capacity(parts.len());
                for v in parts {
                    spans.push(Span::raw(str::from_utf8(v).unwrap_or("☻")));
                }
                lines.push(Line::from(spans));
            }
        }
        let text = Text::from(lines);
        let nav_text = build_nav_text(line_meta, height);
        return super::Content {
            navi: nav_text,
            visible_content: text,
            byte_cursor: 0,
            last_char_bytes_size: 0,
        };
    }
}
