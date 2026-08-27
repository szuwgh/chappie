use std::cmp::max;

const MATCH: i16 = 3;
const MISMATCH: i16 = -1;
const GAP: i16 = -1;
const MIN_SCORE_PERCENT: i16 = 55;
pub(crate) const MAXDIMS: usize = 9182;
const MINDIMS: usize = 512;
use crate::fuzzy::Match;
// Opt-3: Vec 缓冲区存入结构体，find() 只 clear+extend，不再重复 malloc/free
pub(crate) struct SmithWaterman {
    cache: Vec<i16>,
    pos: Vec<(usize, usize)>,
}

impl SmithWaterman {
    pub(crate) fn new() -> SmithWaterman {
        SmithWaterman {
            cache: vec![0; MAXDIMS],
            pos: Vec::new(),
        }
    }
}

struct SliceParts<'a, 'b> {
    parts: &'a [&'b [u8]],
    len: usize,
}

impl<'a, 'b> SliceParts<'a, 'b> {
    #[inline]
    fn new(parts: &'a [&'b [u8]]) -> Self {
        Self {
            len: parts.iter().map(|part| part.len()).sum(),
            parts,
        }
    }

    #[inline]
    fn len(&self) -> usize {
        self.len
    }

    #[inline]
    fn byte_at(&self, index: usize) -> u8 {
        debug_assert!(index < self.len);
        match self.parts.len() {
            1 => unsafe { *self.parts[0].get_unchecked(index) },
            2 => {
                let p0 = self.parts[0];
                if index < p0.len() {
                    unsafe { *p0.get_unchecked(index) }
                } else {
                    unsafe { *self.parts[1].get_unchecked(index - p0.len()) }
                }
            }
            3 => {
                let p0 = self.parts[0];
                let p1 = self.parts[1];
                if index < p0.len() {
                    unsafe { *p0.get_unchecked(index) }
                } else if index < p0.len() + p1.len() {
                    unsafe { *p1.get_unchecked(index - p0.len()) }
                } else {
                    unsafe { *self.parts[2].get_unchecked(index - p0.len() - p1.len()) }
                }
            }
            4 => {
                let p0 = self.parts[0];
                let p1 = self.parts[1];
                let p2 = self.parts[2];
                let end1 = p0.len();
                let end2 = end1 + p1.len();
                let end3 = end2 + p2.len();
                if index < end1 {
                    unsafe { *p0.get_unchecked(index) }
                } else if index < end2 {
                    unsafe { *p1.get_unchecked(index - end1) }
                } else if index < end3 {
                    unsafe { *p2.get_unchecked(index - end2) }
                } else {
                    unsafe { *self.parts[3].get_unchecked(index - end3) }
                }
            }
            _ => {
                let mut offset = index;
                for part in self.parts {
                    if offset < part.len() {
                        return unsafe { *part.get_unchecked(offset) };
                    }
                    offset -= part.len();
                }
                unreachable!("SliceParts::byte_at index out of bounds")
            }
        }
    }
}

impl SmithWaterman {
    pub(crate) fn find_parts(&mut self, pattern: &[u8], parts: &[&[u8]]) -> Vec<Match> {
        self.find_parts_prefiltered(pattern, parts)
    }

    fn find_parts_full(&mut self, pattern: &[u8], parts: &[&[u8]]) -> Vec<Match> {
        let text_len = SliceParts::new(parts).len();
        self.find_parts_in_range(pattern, parts, 0, text_len)
    }

    fn find_parts_prefiltered(&mut self, pattern: &[u8], parts: &[&[u8]]) -> Vec<Match> {
        let text_len = SliceParts::new(parts).len();
        if pattern.is_empty() || text_len == 0 {
            return Vec::new();
        }

        let max_text_len = (MAXDIMS / (pattern.len() + 1)).saturating_sub(1);
        if max_text_len == 0 {
            panic!("Cannot be larger than the maximum dimension 9182");
        }

        let radius = pattern.len().saturating_mul(2).max(pattern.len() + 8);
        let radius = radius.min((max_text_len / 2).max(1));
        let mut seen = [false; 256];
        let mut windows = Vec::new();

        for &anchor in pattern {
            let anchor_idx = anchor as usize;
            if seen[anchor_idx] {
                continue;
            }
            seen[anchor_idx] = true;

            let mut base = 0usize;
            for part in parts {
                let mut offset = 0usize;
                while offset < part.len() {
                    let Some(found) = crate::searcher::memchr::memchr(&part[offset..], anchor)
                    else {
                        break;
                    };
                    let pos = base + offset + found;
                    let mut start = pos.saturating_sub(radius);
                    let mut end = pos.saturating_add(radius).saturating_add(1).min(text_len);
                    if end - start > max_text_len {
                        start = pos.saturating_sub(max_text_len / 2);
                        end = start.saturating_add(max_text_len).min(text_len);
                        start = end.saturating_sub(max_text_len);
                    }
                    windows.push((start, end));
                    offset += found + 1;
                }
                base += part.len();
            }
        }

        if windows.is_empty() {
            return Vec::new();
        }

        windows.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(windows.len());
        for (start, end) in windows {
            if let Some(last) = merged.last_mut() {
                let merged_end = last.1.max(end);
                if start <= last.1 && merged_end - last.0 <= max_text_len {
                    last.1 = merged_end;
                    continue;
                }
            }
            merged.push((start, end));
        }

        let full_cells = (pattern.len() + 1).saturating_mul(text_len + 1);
        if full_cells <= MAXDIMS {
            let candidate_cells = merged
                .iter()
                .map(|(start, end)| (pattern.len() + 1) * (end - start + 1))
                .sum::<usize>();
            if candidate_cells >= full_cells {
                return self.find_parts_full(pattern, parts);
            }
        }

        let mut matches = Vec::new();
        for (start, end) in merged {
            matches.extend(self.find_parts_in_range(pattern, parts, start, end));
        }

        let Some(max_score) = matches.iter().map(|m| m.score).max() else {
            return Vec::new();
        };
        let max_possible_score = MATCH * pattern.len() as i16;
        let threshold = max_possible_score * MIN_SCORE_PERCENT / 100;
        if max_score < threshold {
            return Vec::new();
        }

        matches.retain(|m| m.score == max_score);
        matches.sort_by(|a, b| {
            a.start
                .cmp(&b.start)
                .then(a.end.cmp(&b.end))
                .then(b.score.cmp(&a.score))
        });
        matches.dedup_by(|a, b| a.start == b.start && a.end == b.end && a.score == b.score);
        matches
    }

    fn find_parts_in_range(
        &mut self,
        pattern: &[u8],
        parts: &[&[u8]],
        range_start: usize,
        range_end: usize,
    ) -> Vec<Match> {
        let text = SliceParts::new(parts);
        let len1 = pattern.len();
        let range_end = range_end.min(text.len());
        if range_start >= range_end {
            return Vec::new();
        }
        let len2 = range_end - range_start;
        if len1 == 0 || len2 == 0 {
            return Vec::new();
        }

        let m = (len1 + 1)
            .checked_mul(len2 + 1)
            .expect("SmithWaterman dimension overflow");
        if m > MAXDIMS {
            panic!("Cannot be larger than the maximum dimension 9182");
        }

        let col = len2 + 1;
        let alloc = &mut self.cache[..m];
        alloc.fill(0);

        let mut max_score = 0i16;
        self.pos.clear();
        let pattern_len = len1;

        for (i, &b1) in pattern.iter().enumerate() {
            // Opt-2: 行偏移提到外层循环，消除内层重复乘法
            let row_i = i * col;
            let row_i1 = row_i + col;
            let mut j = 0usize;
            let mut base = 0usize;

            for part in parts {
                let part_start = base;
                let part_end = base + part.len();
                base = part_end;
                if part_end <= range_start {
                    continue;
                }
                if part_start >= range_end {
                    break;
                }

                let local_start = range_start.saturating_sub(part_start);
                let local_end = (range_end - part_start).min(part.len());
                for &b2 in &part[local_start..local_end] {
                    let score = if b1 == b2 { MATCH } else { MISMATCH };
                    // Opt-1+2: get_unchecked + 预算行偏移，消除 O(m×n) bounds check 与乘法
                    // SAFETY: row_i+j < (len1+1)*(len2+1) == alloc.len() 由上方 panic 保证
                    let (a, b, c) = unsafe {
                        (
                            *alloc.get_unchecked(row_i + j),     // get(i,   j)
                            *alloc.get_unchecked(row_i + j + 1), // get(i,   j+1)
                            *alloc.get_unchecked(row_i1 + j),    // get(i+1, j)
                        )
                    };
                    let cur_score = max(0, max(a + score, max(b + GAP, c + GAP)));
                    unsafe {
                        *alloc.get_unchecked_mut(row_i1 + j + 1) = cur_score;
                    }

                    if cur_score > max_score {
                        max_score = cur_score;
                        self.pos.clear();
                        self.pos.push((i + 1, j + 1));
                    } else if cur_score == max_score && max_score > 0 {
                        // Opt-4: max_score==0 时不积累无效位置
                        self.pos.push((i + 1, j + 1));
                    }
                    j += 1;
                }
            }
            debug_assert_eq!(j, len2);
        }

        let mut matchs: Vec<Match> = Vec::new();
        if max_score >= (2 * pattern_len) as i16 {
            for &(max_i, max_j) in self.pos.iter() {
                let mut i = max_i;
                let mut j = max_j;
                while i > 0 && j > 0 {
                    // SAFETY: i <= len1, j <= len2，均在 alloc 范围内
                    let cur = unsafe { *alloc.get_unchecked(i * col + j) };
                    if cur == 0 {
                        break;
                    }
                    let diag = if pattern[i - 1] == text.byte_at(range_start + j - 1) {
                        MATCH
                    } else {
                        MISMATCH
                    };
                    let diag_val = unsafe { *alloc.get_unchecked((i - 1) * col + (j - 1)) };
                    if cur == diag_val + diag {
                        i -= 1;
                        j -= 1;
                    } else {
                        let up_val = unsafe { *alloc.get_unchecked((i - 1) * col + j) };
                        if cur == up_val + GAP {
                            i -= 1;
                        } else {
                            j -= 1;
                        }
                    }
                }

                let start = range_start + j;
                let end = range_start + max_j;
                matchs.push(Match {
                    score: max_score,
                    start,
                    end,
                    positions: (start..end).collect(),
                });
            }
        }
        matchs.sort_by(|a, b| a.start.cmp(&b.start));
        matchs
    }

    pub(crate) fn find(&mut self, pattern: &[u8], text: &[u8]) -> Vec<Match> {
        self.find_parts(pattern, &[text])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::str;

    fn match_tuples(matches: &[Match]) -> Vec<(i16, usize, usize)> {
        matches.iter().map(|m| (m.score, m.start, m.end)).collect()
    }

    // ── ASCII exact ──────────────────────────────────────────────────────────

    #[test]
    fn test_exact_ascii_middle() {
        //let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "say hello world";
        let matches = sw.find(b"hello", text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "hello");
        assert_eq!(matches[0].score, MATCH * 5);
    }

    #[test]
    fn test_exact_ascii_at_start() {
        // let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "hello world";
        let matches = sw.find(b"hello", text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 0);
        assert_eq!(&text[matches[0].start..matches[0].end], "hello");
    }

    #[test]
    fn test_exact_ascii_at_end() {
        //let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "say hello";
        let matches = sw.find("hello".as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].end, text.len());
        assert_eq!(&text[matches[0].start..matches[0].end], "hello");
    }

    #[test]
    fn test_exact_ascii_full_text() {
        //let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "hello";
        let matches = sw.find("hello".as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].end, text.len());
        assert_eq!(matches[0].score, MATCH * 5);
    }

    #[test]
    fn test_find_parts_exact_match_across_slice_boundary() {
        //let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let parts: &[&[u8]] = &[b"say he", b"llo world"];

        let matches = sw.find_parts(b"hello", parts);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 4);
        assert_eq!(matches[0].end, 9);
        assert_eq!(matches[0].score, MATCH * 5);
    }

    #[test]
    fn test_find_parts_fuzzy_match_across_slice_boundary() {
        //let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let parts: &[&[u8]] = &[b"abc h", b"xllo xyz"];

        let matches = sw.find_parts(b"hello", parts);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 4);
        assert_eq!(matches[0].end, 9);
        assert_eq!(matches[0].score, 4 * MATCH + MISMATCH);
    }

    #[test]
    fn test_find_parts_keeps_byte_offsets_after_multibyte_parts() {
        //let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let parts: &[&[u8]] = &["中".as_bytes(), "文he".as_bytes(), b"llo"];

        let matches = sw.find_parts(b"hello", parts);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 6);
        assert_eq!(matches[0].end, 11);
    }

    #[test]
    fn test_find_parts_matches_joined_find_for_many_splits() {
        let text = b"prefix hello middle hxllo suffix";
        let pattern = b"hello";

        for split1 in 0..text.len() {
            for split2 in split1..text.len() {
                //let mut joined_cache = vec![0i16; MAXDIMS];
                let mut joined_sw = SmithWaterman::new();
                let joined = joined_sw.find(pattern, text);

                // let mut parts_cache = vec![0i16; MAXDIMS];
                let mut parts_sw = SmithWaterman::new();
                let parts = [&text[..split1], &text[split1..split2], &text[split2..]];
                let split = parts_sw.find_parts(pattern, &parts);

                assert_eq!(match_tuples(&split), match_tuples(&joined));
            }
        }
    }

    #[test]
    fn test_find_parts_defaults_to_prefilter_and_matches_full_on_sparse_text() {
        let pattern = b"abcdefghijklmnop";
        let mut text = vec![b'x'; 512];
        text[460..476].copy_from_slice(b"abcdxfghijklmnop");

        // let mut full_cache = vec![0i16; MAXDIMS];
        let mut full_sw = SmithWaterman::new();
        let full = full_sw.find_parts_full(pattern, &[&text]);

        // let mut prefiltered_cache = vec![0i16; MAXDIMS];
        let mut prefiltered_sw = SmithWaterman::new();
        let prefiltered = prefiltered_sw.find_parts(pattern, &[&text]);

        assert_eq!(match_tuples(&prefiltered), match_tuples(&full));
    }

    #[test]
    fn test_find_parts_prefiltered_default_matches_across_slice_boundary() {
        let pattern = b"hello";
        let parts: &[&[u8]] = &[b"abc h", b"xllo xyz"];

        //let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let matches = sw.find_parts(pattern, parts);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 4);
        assert_eq!(matches[0].end, 9);
        assert_eq!(matches[0].score, 4 * MATCH + MISMATCH);
    }

    #[test]
    fn test_find_parts_prefiltered_default_handles_large_sparse_text() {
        let pattern = b"abcdefghijklmnop";
        let mut text = vec![b'x'; 4096];
        text[3500..3516].copy_from_slice(b"abcdxfghijklmnop");
        let parts = [&text[..1200], &text[1200..3000], &text[3000..]];

        //let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let matches = sw.find_parts(pattern, &parts);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 3500);
        assert_eq!(matches[0].end, 3516);
        assert_eq!(matches[0].score, 15 * MATCH + MISMATCH);
    }

    #[test]
    fn test_find_parts_prefiltered_default_clamps_candidate_window_to_cache() {
        let pattern: Vec<u8> = (0..80).collect();
        let mut text = vec![b'x'; 4096];
        text[2048..2048 + pattern.len()].copy_from_slice(&pattern);

        let mut sw = SmithWaterman::new();
        let matches = sw.find_parts(&pattern, &[&text]);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 2048);
        assert_eq!(matches[0].end, 2048 + pattern.len());
        assert_eq!(matches[0].score, MATCH * pattern.len() as i16);
    }

    // ── score verification ───────────────────────────────────────────────────

    #[test]
    fn test_score_one_substitution() {
        //let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        // "hxllo" vs "hello": 4 MATCH + 1 MISMATCH = 4*3 + 1*(-2) = 10
        let matches = sw.find("hello".as_bytes(), "hxllo".as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].score, 4 * MATCH + MISMATCH); // 10
        assert_eq!(&"hxllo"[matches[0].start..matches[0].end], "hxllo");
    }

    #[test]
    fn test_no_match_two_substitutions() {
        //let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        // "hxxlo" vs "hello": 3*3 + 2*(-2) = 5, threshold = 10 → no match
        let matches = sw.find("hello".as_bytes(), "hxxlo".as_bytes());
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_no_match_completely_different() {
        // let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let matches = sw.find("zzzzz".as_bytes(), "abcdefgh".as_bytes());
        assert_eq!(matches.len(), 0);
    }

    // ── byte offset correctness ──────────────────────────────────────────────

    #[test]
    fn test_byte_offset_ascii_middle() {
        //let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "abc hello xyz";
        let matches = sw.find("hello".as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 4); // "abc " = 4 bytes
        assert_eq!(matches[0].end, 9); // "abc hello" = 9 bytes
        assert_eq!(&text[matches[0].start..matches[0].end], "hello");
    }

    #[test]
    fn test_byte_offset_after_multibyte_chars() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        // "中" = 3 bytes, "文" = 3 bytes → "中文" = 6 bytes before "hello"
        let text = "中文hello";
        let matches = sw.find("hello".as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 6);
        assert_eq!(&text[matches[0].start..matches[0].end], "hello");
    }

    #[test]
    fn test_byte_offset_mixed_cjk_ascii() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "abc端口def";
        let matches = sw.find("端口".as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 3); // "abc" = 3 bytes
        assert_eq!(&text[matches[0].start..matches[0].end], "端口");
    }

    // ── Unicode exact matches ────────────────────────────────────────────────

    #[test]
    fn test_chinese_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "检查端口是否启动的函数";
        let matches = sw.find("端口".as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "端口");
    }

    #[test]
    fn test_japanese_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "こんにちは世界";
        let matches = sw.find("にちは".as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "にちは");
    }

    #[test]
    fn test_korean_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "안녕하세요 세계";
        let matches = sw.find("하세요".as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "하세요");
    }

    #[test]
    fn test_arabic_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "السلام عليكم";
        // "لسلام" is a substring starting at char index 1
        let matches = sw.find("لسلام".as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "لسلام");
    }

    #[test]
    fn test_chinese_unicode_score() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "端口";
        let pattern = "端口";
        let matches = sw.find(pattern.as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].score, MATCH * pattern.len() as i16); // byte-length scoring
    }

    // ── numbers and special characters ──────────────────────────────────────

    #[test]
    fn test_numbers_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "abc12345def";
        let matches = sw.find("12345".as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "12345");
    }

    #[test]
    fn test_numbers_fuzzy() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        // "12x45" vs "12345": 4 match + 1 mismatch = 10, threshold = 10
        let text = "abc12345def";
        let matches = sw.find("12x45".as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "12345");
    }

    // ── case sensitivity ─────────────────────────────────────────────────────

    #[test]
    fn test_case_sensitive_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        // uppercase "HELLO" should not match lowercase "hello" pattern
        // All 5 chars mismatch: score = 5*(-2) = -10, clamped to 0
        let matches = sw.find("hello".as_bytes(), "HELLO WORLD".as_bytes());
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_case_sensitive_partial() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        // Pattern "hello" vs text containing "Hello": H≠h is clamped to 0 in local alignment,
        // so the alignment starts from 'e'. Score = 4*3 = 12, threshold = 10. Match is "ello".
        let text = "say Hello";
        let matches = sw.find("hello".as_bytes(), text.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "ello");
    }

    // ── sorted output ────────────────────────────────────────────────────────

    #[test]
    fn test_results_sorted_by_start() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "hello world hello";
        let matches = sw.find("hello".as_bytes(), text.as_bytes());
        assert!(!matches.is_empty());
        for i in 1..matches.len() {
            assert!(matches[i].start >= matches[i - 1].start);
        }
    }

    // ── cache reuse across calls ─────────────────────────────────────────────

    #[test]
    fn test_reuse_across_calls() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();

        let text = "say hello world";
        let m1 = sw.find("hello".as_bytes(), text.as_bytes());
        assert_eq!(&text[m1[0].start..m1[0].end], "hello");

        let m2 = sw.find("world".as_bytes(), text.as_bytes());
        assert_eq!(&text[m2[0].start..m2[0].end], "world");

        let m3 = sw.find("say".as_bytes(), text.as_bytes());
        assert_eq!(&text[m3[0].start..m3[0].end], "say");
    }

    // ── valid UTF-8 slices for fuzzy Unicode ─────────────────────────────────

    #[test]
    fn test_fuzzy_chinese_valid_utf8_slice() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "如果你想从一个字符串中跳过前几个字，并从之后的位跳当前几个字";
        let pattern = "跳去前几行字";
        let matches = sw.find(pattern.as_bytes(), text.as_bytes());
        // Verify all returned slices are valid UTF-8 (no panic on indexing)
        for m in &matches {
            let slice = &text[m.start..m.end];
            assert!(std::str::from_utf8(slice.as_bytes()).is_ok());
        }
    }

    #[test]
    fn test_mixed_unicode_valid_slices() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "12396874,这是中文文本，包含一些特殊字符：@#%&*()，以及英文文字: Hello World! <>/。阿拉伯文: السلام عليكم。韩文: 안녕하세요。日文: こんにちは。#RustExample";
        let patterns = [
            "Hxllo",
            "لسلام عليك",
            "하세요",
            "にちは",
            "96974",
            "RustUxample",
        ];
        for pattern in patterns {
            let matches = sw.find(pattern.as_bytes(), text.as_bytes());
            for m in &matches {
                let slice = &text[m.start..m.end];
                assert!(
                    std::str::from_utf8(slice.as_bytes()).is_ok(),
                    "pattern={pattern} produced invalid UTF-8 slice [{}, {}]",
                    m.start,
                    m.end
                );
            }
        }
    }

    // ── long text ────────────────────────────────────────────────────────────

    #[test]
    fn test_long_text_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "Elasticsearch is a distributed search and analytics engine, scalable data store and vector database optimized for speed and relevance on production-scale workloads.";
        let pattern = "applications"; // not present → should be empty or fuzzy
        let matches = sw.find(pattern.as_bytes(), text.as_bytes());
        // Just verify no panic; slices must be valid if any
        for m in &matches {
            let _ = &text[m.start..m.end];
        }
    }

    #[test]
    fn test_long_text_fuzzy() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let text = "Elasticsearch is a distributed search and analytics engine, scalable data store and vector database optimized for speed and relevance on production-scale workloads. Elasticsearch is the foundation of Elastics open Stack platform. Search in near real-time over massive datasets, perform vector searches, integrate with generative AI applications, and much more.";
        let pattern = "apelicetions";
        let matches = sw.find(pattern.as_bytes(), text.as_bytes());
        for m in &matches {
            let slice = &text[m.start..m.end];
            assert!(std::str::from_utf8(slice.as_bytes()).is_ok());
        }
    }

    // ── keep original regression tests ───────────────────────────────────────

    #[test]
    fn test_smith_waterman() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();

        let text = "Elasticsearch is a distributed search and analytics engine, scalable data store and vector database optimized for speed and relevance on production-scale workloads. Elasticsearch is the foundation of Elastics open Stack platform. Search in near real-time over massive datasets, perform vector searches, integrate with generative AI applications, and much more.";
        let pattern = "applications";

        let m = sw.find(pattern.as_bytes(), text.as_bytes());
        for v in m.iter() {
            println!("{:?}", &text[v.start..v.end]);
        }

        let text = "Elasticsearch is a distributed search and analytics engine, scalable data store and vector database optimized for speed and relevance on production-scale workloads. Elasticsearch is the foundation of Elastics open Stack platform. Search in near real-time over massive datasets, perform vector searches, integrate with generative AI applications, and much more.";
        let pattern = "apelicetions";

        let m = sw.find(pattern.as_bytes(), text.as_bytes());
        for v in m.iter() {
            println!(
                "{:?}",
                String::from_utf8_lossy(&text.as_bytes()[v.start..v.end])
            );
        }

        let text = "如果你想从一个字符串中跳过前几个字，并从之后的位跳当前几个字";
        let pattern = "跳去前几行字";
        let m = sw.find(pattern.as_bytes(), text.as_bytes());
        for v in m.iter() {
            println!(
                "{:?}",
                String::from_utf8_lossy(&text.as_bytes()[v.start..v.end])
            );
        }

        let text = "12396874,这是中文文本，包含一些特殊字符：@#%&*()，以及英文文字: Hello World! <>/。阿拉伯文: السلام عليكم。韩文: 안녕하세요。日文: こんにちは。#RustExample";

        let patterns = vec![
            "ec",
            "Hxllo",
            "لسلام عليك",
            "하세요",
            "にちは",
            "[@#%&*()]+",
            "96974",
            "@#?&*",
            "RustUxample",
        ];

        for pattern in patterns {
            let m = sw.find(pattern.as_bytes(), text.as_bytes());
            for v in m.iter() {
                println!(
                    "pattern:{},get:{:?}",
                    pattern,
                    String::from_utf8_lossy(&text.as_bytes()[v.start..v.end])
                );
            }
        }

        let text = "hxllo，abc，htllo";
        let pattern = "hello";

        let m = sw.find(pattern.as_bytes(), text.as_bytes());
        for v in m.iter() {
            println!(
                "{:?}",
                String::from_utf8_lossy(&text.as_bytes()[v.start..v.end])
            );
        }
    }

    #[test]
    fn test_smith_waterman3() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();

        let text = "hello abc";
        let pattern = "hxlloo";

        let m = sw.find(pattern.as_bytes(), text.as_bytes());
        for v in m.iter() {
            println!(
                "pattern:{},get:{:?}",
                pattern,
                str::from_utf8(&text.as_bytes()[v.start..v.end]).unwrap()
            );
        }
    }

    #[test]
    fn test_smith_waterman2() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();

        let text = "检查端口是否启动的函数";
        let pattern = "端口";

        let m = sw.find(pattern.as_bytes(), text.as_bytes());
        for v in m.iter() {
            println!(
                "pattern:{},get:{:?}",
                pattern,
                str::from_utf8(&text.as_bytes()[v.start..v.end]).unwrap()
            );
        }
    }

    #[test]
    #[ignore = "benchmark-style perf test; run with `cargo test --release bench_smith_waterman_current_performance -- --ignored --nocapture`"]
    fn bench_smith_waterman_current_performance() {
        use std::hint::black_box;
        use std::time::Instant;

        fn report(name: &str, rounds: usize, cells_per_round: usize, elapsed: std::time::Duration) {
            let ns_per_op = elapsed.as_nanos() as f64 / rounds as f64;
            let cells_per_sec = cells_per_round as f64 * rounds as f64 / elapsed.as_secs_f64();
            eprintln!(
                "{name}: rounds={rounds}, cells/op={cells_per_round}, elapsed={elapsed:?}, ns/op={ns_per_op:.1}, Mcells/s={:.2}",
                cells_per_sec / 1_000_000.0
            );
        }

        let rounds = if cfg!(debug_assertions) {
            2_000
        } else {
            80_000
        };
        let pattern = b"abcdefghijklmnop";
        let mut text = vec![b'x'; 512];
        text[460..476].copy_from_slice(b"abcdxfghijklmnop");
        let cells_per_round = (pattern.len() + 1) * (text.len() + 1);

        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let mut found = 0usize;
        let start = Instant::now();
        for _ in 0..rounds {
            let matches = sw.find_parts_full(black_box(pattern), black_box(&[&text]));
            found = found.wrapping_add(matches.len());
            black_box(&matches);
        }
        let elapsed = start.elapsed();
        assert!(found > 0);
        report(
            "smith_waterman_find_single_slice",
            rounds,
            cells_per_round,
            elapsed,
        );

        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let mut found = 0usize;
        let start = Instant::now();
        for _ in 0..rounds {
            let matches = sw.find(black_box(pattern), black_box(&text));
            found = found.wrapping_add(matches.len());
            black_box(&matches);
        }
        let elapsed = start.elapsed();
        assert!(found > 0);
        report(
            "smith_waterman_find_default_single_slice",
            rounds,
            cells_per_round,
            elapsed,
        );

        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let parts = [&text[..173], &text[173..349], &text[349..]];
        let mut found = 0usize;
        let start = Instant::now();
        for _ in 0..rounds {
            let matches = sw.find_parts_full(black_box(pattern), black_box(&parts));
            found = found.wrapping_add(matches.len());
            black_box(&matches);
        }
        let elapsed = start.elapsed();
        assert!(found > 0);
        report(
            "smith_waterman_find_parts_3_slices",
            rounds,
            cells_per_round,
            elapsed,
        );

        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new();
        let mut found = 0usize;
        let start = Instant::now();
        for _ in 0..rounds {
            let matches = sw.find_parts(black_box(pattern), black_box(&parts));
            found = found.wrapping_add(matches.len());
            black_box(&matches);
        }
        let elapsed = start.elapsed();
        assert!(found > 0);
        report(
            "smith_waterman_find_parts_default_3_slices",
            rounds,
            cells_per_round,
            elapsed,
        );
    }
}
