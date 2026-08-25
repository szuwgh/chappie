use crate::common::ring_vec::RingVec;
use crate::fuzzy::Match;
use crate::textwarp::CacheStr;
use crate::textwarp::LineState;
use crate::tui::build_nav_text;
use crate::tui::char_range_to_visible_with_cursor_byte;
use crate::tui::BuildContent;
use crate::tui::EditContext;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

fn build_text_spans<'a>(
    parts: &[&'a [u8]],
    meta: &LineState,
    visible_byte_start: usize,
    visible_byte_end: usize,
    offsets: &[Match],
    cursor_byte: Option<usize>,
    current_physical_line: bool,
) -> Vec<Span<'a>> {
    let visible_abs_start = meta.line_offset + visible_byte_start;
    let visible_abs_end = meta.line_offset + visible_byte_end;
    debug_assert!(offsets.windows(2).all(|w| w[0].start <= w[1].start));

    let mut spans = Vec::with_capacity(offsets.len().saturating_mul(2).saturating_add(2));
    let mut pos = 0usize;
    let total_len: usize = parts.iter().map(|p| p.len()).sum();
    let mut match_idx = offsets.partition_point(|m| m.end <= visible_abs_start);

    while pos < total_len {
        let abs_pos = visible_abs_start + pos;

        while match_idx < offsets.len() && offsets[match_idx].end <= abs_pos {
            match_idx += 1;
        }

        let mut next_boundary = total_len;
        let mut highlight = false;

        if let Some(m) = offsets
            .get(match_idx)
            .filter(|m| m.start < visible_abs_end && m.end > visible_abs_start)
        {
            let start = m.start.max(visible_abs_start) - visible_abs_start;
            let end = m.end.min(visible_abs_end) - visible_abs_start;

            if pos >= start && pos < end {
                next_boundary = end;
                highlight = true;
            } else if start > pos {
                next_boundary = start;
            }
        }

        if let Some(cursor) = cursor_byte {
            if pos < cursor && cursor < next_boundary {
                next_boundary = cursor;
            } else if cursor == pos {
                next_boundary = next_utf8_boundary(parts, pos).unwrap_or(pos + 1);
            }
        }

        let cursor = cursor_byte == Some(pos);
        push_range_span(
            &mut spans,
            parts,
            pos,
            next_boundary,
            highlight,
            cursor,
            current_physical_line,
        );
        pos = next_boundary;
    }

    if cursor_byte == Some(total_len) {
        spans.push(Span::styled(
            " ",
            Style::default().bg(Color::LightRed).fg(Color::White),
        ));
    }

    spans
}

fn push_range_span<'a>(
    spans: &mut Vec<Span<'a>>,
    parts: &[&'a [u8]],
    start: usize,
    end: usize,
    highlight: bool,
    cursor: bool,
    current_physical_line: bool,
) {
    let mut base = 0usize;

    for part in parts {
        let part_end = base + part.len();

        if part_end <= start || base >= end {
            base = part_end;
            continue;
        }

        let s = start.saturating_sub(base);
        let e = (end - base).min(part.len());
        push_text_span(spans, &part[s..e], highlight, cursor, current_physical_line);
        base = part_end;
    }
}

fn push_text_span<'a>(
    spans: &mut Vec<Span<'a>>,
    bytes: &'a [u8],
    highlight: bool,
    cursor: bool,
    current_physical_line: bool,
) {
    if bytes.is_empty() {
        return;
    }

    if cursor && bytes == b"\n" {
        spans.push(Span::styled(" ", newline_style(highlight)));
        return;
    }

    let text = if bytes.last() == Some(&b'\n') {
        &bytes[..bytes.len() - 1]
    } else {
        bytes
    };

    if !text.is_empty() {
        push_plain_text_span(spans, text, highlight, cursor, current_physical_line);
    }
}

fn push_plain_text_span<'a>(
    spans: &mut Vec<Span<'a>>,
    bytes: &'a [u8],
    highlight: bool,
    cursor: bool,
    current_physical_line: bool,
) {
    spans.push(Span::styled(
        std::str::from_utf8(bytes).unwrap_or("☻"),
        text_style(highlight, cursor, current_physical_line),
    ));
}

fn newline_style(highlight: bool) -> Style {
    let mut style = Style::default().bg(Color::LightBlue);
    if highlight {
        style = style.fg(Color::Green);
    }
    style
}

fn text_style(highlight: bool, cursor: bool, current_physical_line: bool) -> Style {
    let mut style = Style::default();
    if current_physical_line {
        style = style.bg(Color::DarkGray);
    }
    if highlight {
        style = style.fg(Color::Green);
    }
    if cursor {
        style = style.bg(Color::LightRed).fg(Color::White);
    }
    style
}

fn next_utf8_boundary(parts: &[&[u8]], pos: usize) -> Option<usize> {
    let bytes = byte_at(parts, pos)?;
    let len = if bytes < 0x80 {
        1
    } else if bytes & 0b1110_0000 == 0b1100_0000 {
        2
    } else if bytes & 0b1111_0000 == 0b1110_0000 {
        3
    } else if bytes & 0b1111_1000 == 0b1111_0000 {
        4
    } else {
        1
    };

    Some(pos + len)
}

fn byte_at(parts: &[&[u8]], pos: usize) -> Option<u8> {
    let mut base = 0usize;

    for part in parts {
        let end = base + part.len();
        if pos < end {
            return Some(part[pos - base]);
        }
        base = end;
    }

    None
}

pub(crate) struct TextBuildContent;

impl BuildContent for TextBuildContent {
    fn build_content<'a>(
        lines: &mut Vec<Line<'a>>,
        txts: &'a RingVec<CacheStr>,
        with: usize,
        line_meta: &'a RingVec<LineState>,
        cur_line: usize,
        select_line: &Option<(usize, usize)>,
        ed_ctx: &EditContext<'_>,
    ) -> super::Content<'a> {
        lines.clear();
        let height = ed_ctx.height;
        let column_offset = ed_ctx.column_offset;
        let cursor_y = ed_ctx.cursor_y;
        let cursor_x = ed_ctx.cursor_x;
        let is_txt_model = ed_ctx.is_txt_model;
        //判断
        let cursor_line_index = if is_txt_model {
            line_meta.get(cursor_y).map(|m| m.line_index)
        } else {
            None
        };

        // let mut find_highlight_offset = ed_ctx.find_highlight_offset;
        // let find_line_index = ed_ctx.find_line_index;
        for (i, txt) in txts.iter().enumerate() {
            let full = txt.text(0..);
            let cursor = if cursor_y == i && is_txt_model {
                Some(cursor_x)
            } else {
                None
            };
            let (visible, visible_byte_start, visible_byte_end, cursor_byte) =
                char_range_to_visible_with_cursor_byte(
                    full.as_parts(),
                    column_offset,
                    column_offset + with,
                    cursor,
                );
            let parts: &[&[u8]] = visible.as_parts();
            let meta = line_meta.get(i).unwrap();
            let offsets = ed_ctx.highlights.for_line(meta.line_index);

            let spans = build_text_spans(
                parts,
                meta,
                visible_byte_start,
                visible_byte_end,
                offsets,
                cursor_byte,
                cursor_line_index == Some(meta.line_index),
            );

            lines.push(Line::from(spans));
        }
        let nav_text = build_nav_text(line_meta, height);
        return super::Content {
            navi: nav_text,
            byte_cursor: 0,
            last_char_bytes_size: 0,
        };
    }

    fn command_focus() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_spans_highlight_uses_match_byte_range_after_multibyte_prefix() {
        let line = "中文abcdef".as_bytes();
        let (visible, visible_byte_start, visible_byte_end) =
            crate::tui::char_range_to_visible_with_byte_range(&[line], 2, 5);
        let meta = LineState::builder().line_offset(0).build();
        let offsets = vec![Match {
            score: 0,
            start: "中文".len(),
            end: "中文abc".len(),
        }];

        let spans = build_text_spans(
            &visible.as_parts(),
            &meta,
            visible_byte_start,
            visible_byte_end,
            &offsets,
            None,
            false,
        );

        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "abc");
        assert_eq!(spans[0].style.fg, Some(Color::Green));
        assert_eq!(spans[0].style.bg, None);
    }

    #[test]
    fn text_spans_composes_search_foreground_with_current_physical_line_background() {
        let line = b"abcDEFghi";
        let meta = LineState::builder().line_offset(0).build();
        let offsets = vec![Match {
            score: 0,
            start: 3,
            end: 6,
        }];

        let parts: &[&[u8]] = &[line.as_slice()];
        let spans = build_text_spans(parts, &meta, 0, line.len(), &offsets, None, true);

        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content.as_ref(), "abc");
        assert_eq!(spans[0].style.bg, Some(Color::DarkGray));
        assert_eq!(spans[0].style.fg, None);
        assert_eq!(spans[1].content.as_ref(), "DEF");
        assert_eq!(spans[1].style.bg, Some(Color::DarkGray));
        assert_eq!(spans[1].style.fg, Some(Color::Green));
        assert_eq!(spans[2].content.as_ref(), "ghi");
        assert_eq!(spans[2].style.bg, Some(Color::DarkGray));
        assert_eq!(spans[2].style.fg, None);
    }

    #[test]
    fn text_spans_hides_newline_without_cursor() {
        let line = b"abc\n";
        let meta = LineState::builder().line_offset(0).build();

        let spans = build_text_spans(&[line.as_slice()], &meta, 0, line.len(), &[], None, false);

        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "abc");
    }

    #[test]
    fn text_spans_renders_newline_as_blue_marker_when_cursor_hits_newline() {
        let line = b"abc\n";
        let meta = LineState::builder().line_offset(0).build();

        let spans = build_text_spans(
            &[line.as_slice()],
            &meta,
            0,
            line.len(),
            &[],
            Some(3),
            false,
        );

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content.as_ref(), "abc");
        assert_eq!(spans[1].content.as_ref(), " ");
        assert_eq!(spans[1].style.bg, Some(Color::LightBlue));
    }

    #[test]
    fn text_spans_renders_newline_cursor_after_multibyte_chars() {
        let line = "中文\n".as_bytes();
        let meta = LineState::builder().line_offset(0).build();

        let spans = build_text_spans(
            &[line],
            &meta,
            0,
            line.len(),
            &[],
            Some("中文".len()),
            false,
        );

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content.as_ref(), "中文");
        assert_eq!(spans[1].content.as_ref(), " ");
        assert_eq!(spans[1].style.bg, Some(Color::LightBlue));
    }
}
