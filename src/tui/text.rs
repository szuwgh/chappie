use crate::chap;
use crate::common::ring_vec::RingVec;
use crate::textwarp::CacheStr;
use crate::textwarp::LineParts;
use crate::textwarp::LineState;
use crate::tui::build_cursor_line;
use crate::tui::build_highlight_spans;
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

fn build_text_spans<'a>(
    parts: &[&'a [u8]],
    meta: &LineState,
    visible_start: usize,
    visible_end: usize,
    offsets: &[usize],
    highlight_len: usize,
    cursor_x: Option<usize>,
) -> Vec<Span<'a>> {
    let visible_abs_start = meta.line_offset + visible_start;
    let visible_abs_end = meta.line_offset + visible_end;

    let mut ranges: Vec<(usize, usize)> = offsets
        .iter()
        .filter_map(|offset| {
            let start = *offset;
            let end = start + highlight_len;
            let start = start.max(visible_abs_start);
            let end = end.min(visible_abs_end);

            if start < end {
                Some((start - visible_abs_start, end - visible_abs_start))
            } else {
                None
            }
        })
        .collect();

    ranges.sort_by_key(|r| r.0);

    let mut spans = Vec::new();
    let mut pos = 0usize;
    let total_len: usize = parts.iter().map(|p| p.len()).sum();

    while pos < total_len {
        let next_highlight = ranges
            .iter()
            .find(|(start, end)| pos >= *start && pos < *end)
            .copied();

        let mut next_boundary = total_len;
        let mut highlight = false;

        if let Some((_, end)) = next_highlight {
            next_boundary = end;
            highlight = true;
        } else if let Some((start, _)) = ranges.iter().find(|(start, _)| *start > pos) {
            next_boundary = *start;
        }

        if let Some(cursor) = cursor_x {
            if pos < cursor && cursor < next_boundary {
                next_boundary = cursor;
            } else if cursor == pos {
                next_boundary = next_utf8_boundary(parts, pos).unwrap_or(pos + 1);
            }
        }

        let cursor = cursor_x == Some(pos);
        push_range_span(&mut spans, parts, pos, next_boundary, highlight, cursor);
        pos = next_boundary;
    }

    if cursor_x == Some(total_len) {
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
        let text = std::str::from_utf8(&part[s..e]).unwrap_or("☻");

        let style = if cursor {
            Style::default().bg(Color::LightRed).fg(Color::White)
        } else if highlight {
            Style::default().bg(Color::Green)
        } else {
            Style::default()
        };

        spans.push(Span::styled(text, style));
        base = part_end;
    }
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

        // let mut find_highlight_offset = ed_ctx.find_highlight_offset;
        // let find_line_index = ed_ctx.find_line_index;
        let highlight_len = ed_ctx.highlight_len;

        for (i, txt) in txts.iter().enumerate() {
            let full = txt.text(0..);
            let visible =
                char_range_to_visible(full.as_parts(), column_offset, column_offset + with);
            let parts: &[&[u8]] = visible.as_parts();

            // if let Some(target_line_index) = find_line_index {
            //     let meta = line_meta.get(i).unwrap();
            //     if let Some(spans) = build_highlight_spans(
            //         parts,
            //         meta,
            //         target_line_index,
            //         column_offset,
            //         column_offset + with,
            //         &mut find_highlight_offset,
            //         &mut highlight_len,
            //     ) {
            //         lines.push(Line::from(spans));
            //         continue;
            //     }
            // }
            let meta = line_meta.get(i).unwrap();
            // if let Some(highlights) = &ed_ctx.highlights {
            //     if let Some((_, offsets)) = highlights
            //         .iter()
            //         .find(|(line_index, _)| *line_index == meta.line_index)
            //     {
            //         let spans = build_multi_highlight_spans(
            //             parts,
            //             meta,
            //             column_offset,
            //             column_offset + with,
            //             offsets,
            //             highlight_len,
            //         );

            //         if !spans.is_empty() {
            //             lines.push(Line::from(spans));
            //             continue;
            //         }
            //     }
            // }

            // if cursor_y == i && is_txt_model {
            //     let (spans, _, _) = build_cursor_line(parts, cursor_x, &[], 0);
            //     lines.push(Line::from(spans));
            // } else {
            //     let mut spans = Vec::with_capacity(parts.len());
            //     for v in parts {
            //         spans.push(Span::raw(str::from_utf8(v).unwrap_or("☻")));
            //     }
            //     lines.push(Line::from(spans));
            // }
            let offsets: &[usize] = ed_ctx
                .highlights
                .as_ref()
                .and_then(|highlights| {
                    highlights
                        .iter()
                        .find(|(line_index, _)| *line_index == meta.line_index)
                        .map(|(_, offsets)| offsets.as_slice())
                })
                .unwrap_or(&[]);

            let cursor = if cursor_y == i && is_txt_model {
                Some(cursor_x)
            } else {
                None
            };

            let spans = build_text_spans(
                parts,
                meta,
                column_offset,
                column_offset + with,
                offsets,
                highlight_len,
                cursor,
            );

            lines.push(Line::from(spans));
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

    fn command_focus() -> bool {
        true
    }
}
