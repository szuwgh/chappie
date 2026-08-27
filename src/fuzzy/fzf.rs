use crate::fuzzy::smart_case;
use crate::fuzzy::Match;
use crate::searcher::memchr::memchr;

const SCORE_MATCH: i16 = 16;
const SCORE_GAP_START: i16 = -3;
const SCORE_GAP_EXT: i16 = -1;
const BONUS_BOUNDARY: i16 = SCORE_MATCH / 2;
const BONUS_NON_WORD: i16 = SCORE_MATCH / 2;
const BONUS_CAMEL123: i16 = BONUS_BOUNDARY + SCORE_GAP_EXT;
const BONUS_BOUNDARY_WHITE: i16 = BONUS_BOUNDARY + 2;
const BONUS_BOUNDARY_DELIMITER: i16 = BONUS_BOUNDARY + 1;
const BONUS_CONSECUTIVE: i16 = -(SCORE_GAP_START + SCORE_GAP_EXT);
const BONUS_FIRST_CHAR_MULTIPLIER: i16 = 2;
const MAX_MATCH_SPAN_PER_PATTERN_BYTE: usize = 4;

pub(crate) struct FzfMatcher {
    pattern: Vec<u8>,
    positions: Vec<usize>,
}

impl FzfMatcher {
    pub(crate) fn new() -> Self {
        Self {
            pattern: Vec::new(),
            positions: Vec::new(),
        }
    }

    pub(crate) fn find_parts(&mut self, pattern: &[u8], parts: &[&[u8]]) -> Vec<Match> {
        self.find_best(pattern, parts).into_iter().collect()
    }

    pub(crate) fn find_parts_smart_case(&mut self, pattern: &[u8], parts: &[&[u8]]) -> Vec<Match> {
        self.find_best_smart_case(pattern, parts)
            .into_iter()
            .collect()
    }

    pub(crate) fn find_best(&mut self, pattern: &[u8], parts: &[&[u8]]) -> Option<Match> {
        self.find_best_impl(pattern, parts, false)
    }

    pub(crate) fn find_best_smart_case(
        &mut self,
        pattern: &[u8],
        parts: &[&[u8]],
    ) -> Option<Match> {
        self.find_best_impl(pattern, parts, smart_case(pattern))
    }

    pub(crate) fn find_best_case_sensitive(
        &mut self,
        pattern: &[u8],
        parts: &[&[u8]],
    ) -> Option<Match> {
        self.find_best_impl(pattern, parts, true)
    }

    fn find_best_impl(
        &mut self,
        pattern: &[u8],
        parts: &[&[u8]],
        case_sensitive: bool,
    ) -> Option<Match> {
        if pattern.is_empty() {
            return None;
        }

        let text_len = parts_len(parts);
        if text_len == 0 || pattern.len() > text_len {
            return None;
        }

        self.pattern.clear();
        if case_sensitive {
            self.pattern.extend_from_slice(pattern);
        } else {
            self.pattern.extend(pattern.iter().map(|&b| ascii_lower(b)));
        }

        let end = self.forward_scan(parts, case_sensitive)?;
        let start = self.backward_shrink(parts, end, case_sensitive);
        if is_overly_sparse_match(self.pattern.len(), start, end) {
            return None;
        }
        let (score, positions) =
            calculate_score_window(parts, &self.pattern, start, end, case_sensitive);

        Some(Match {
            score,
            start,
            end,
            positions,
        })
    }

    fn forward_scan(&self, parts: &[&[u8]], case_sensitive: bool) -> Option<usize> {
        let mut start = 0usize;
        let mut end = 0usize;

        for &byte in &self.pattern {
            let pos = find_next(parts, start, byte, case_sensitive)?;
            end = pos + 1;
            start = end;
        }

        Some(end)
    }

    fn backward_shrink(&mut self, parts: &[&[u8]], end: usize, case_sensitive: bool) -> usize {
        self.positions.clear();

        let mut pos = end;
        for &byte in self.pattern.iter().rev() {
            while pos > 0 {
                pos -= 1;
                let current = if case_sensitive {
                    byte_at(parts, pos)
                } else {
                    ascii_lower(byte_at(parts, pos))
                };
                if current == byte {
                    self.positions.push(pos);
                    break;
                }
            }
        }

        self.positions.reverse();
        self.positions[0]
    }
}

#[inline]
fn parts_len(parts: &[&[u8]]) -> usize {
    parts.iter().map(|part| part.len()).sum()
}

#[inline]
fn ascii_lower(byte: u8) -> u8 {
    if byte.is_ascii_uppercase() {
        byte + 32
    } else {
        byte
    }
}

fn find_next(parts: &[&[u8]], start: usize, byte: u8, case_sensitive: bool) -> Option<usize> {
    if case_sensitive || !byte.is_ascii_lowercase() {
        return find_next_exact(parts, start, byte);
    }

    let upper = if byte.is_ascii_lowercase() {
        Some(byte - 32)
    } else {
        None
    };

    let mut base = 0usize;
    for part in parts {
        let end = base + part.len();
        if end <= start {
            base = end;
            continue;
        }

        let local_start = start.saturating_sub(base);
        let lower_pos = memchr(&part[local_start..], byte);
        let upper_pos = upper.and_then(|upper| memchr(&part[local_start..], upper));

        let local = match (lower_pos, upper_pos) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        if let Some(local) = local {
            return Some(base + local_start + local);
        }
        base = end;
    }

    None
}

fn find_next_exact(parts: &[&[u8]], start: usize, byte: u8) -> Option<usize> {
    let mut base = 0usize;
    for part in parts {
        let end = base + part.len();
        if end <= start {
            base = end;
            continue;
        }

        let local_start = start.saturating_sub(base);
        if let Some(local) = memchr(&part[local_start..], byte) {
            return Some(base + local_start + local);
        }
        base = end;
    }

    None
}

#[inline]
fn byte_at(parts: &[&[u8]], index: usize) -> u8 {
    match parts.len() {
        1 => unsafe { *parts[0].get_unchecked(index) },
        2 => {
            let p0 = parts[0];
            if index < p0.len() {
                unsafe { *p0.get_unchecked(index) }
            } else {
                unsafe { *parts[1].get_unchecked(index - p0.len()) }
            }
        }
        3 => {
            let p0 = parts[0];
            let p1 = parts[1];
            let end0 = p0.len();
            let end1 = end0 + p1.len();
            if index < end0 {
                unsafe { *p0.get_unchecked(index) }
            } else if index < end1 {
                unsafe { *p1.get_unchecked(index - end0) }
            } else {
                unsafe { *parts[2].get_unchecked(index - end1) }
            }
        }
        4 => {
            let p0 = parts[0];
            let p1 = parts[1];
            let p2 = parts[2];
            let end0 = p0.len();
            let end1 = end0 + p1.len();
            let end2 = end1 + p2.len();
            if index < end0 {
                unsafe { *p0.get_unchecked(index) }
            } else if index < end1 {
                unsafe { *p1.get_unchecked(index - end0) }
            } else if index < end2 {
                unsafe { *p2.get_unchecked(index - end1) }
            } else {
                unsafe { *parts[3].get_unchecked(index - end2) }
            }
        }
        _ => {
            let mut offset = index;
            for part in parts {
                if offset < part.len() {
                    return unsafe { *part.get_unchecked(offset) };
                }
                offset -= part.len();
            }
            unreachable!("byte_at index out of bounds")
        }
    }
}

fn calculate_score_window(
    parts: &[&[u8]],
    pattern: &[u8],
    start: usize,
    end: usize,
    case_sensitive: bool,
) -> (i16, Vec<usize>) {
    let mut score = 0i16;
    let mut in_gap = false;
    let mut consecutive = 0i16;
    let mut first_bonus = 0i16;
    let mut pidx = 0usize;
    let mut positions = Vec::with_capacity(pattern.len());
    let mut previous = if start > 0 {
        Some(byte_at(parts, start - 1))
    } else {
        None
    };

    for idx in start..end {
        let current = byte_at(parts, idx);
        let match_byte = if case_sensitive {
            current
        } else {
            ascii_lower(current)
        };

        if match_byte == pattern[pidx] {
            positions.push(idx);
            score += SCORE_MATCH;
            let mut bonus = bonus_for(previous, current);
            if consecutive == 0 {
                first_bonus = bonus;
            } else {
                if bonus >= BONUS_BOUNDARY && bonus > first_bonus {
                    first_bonus = bonus;
                }
                bonus = bonus.max(first_bonus).max(BONUS_CONSECUTIVE);
            }
            if pidx == 0 {
                score += bonus * BONUS_FIRST_CHAR_MULTIPLIER;
            } else {
                score += bonus;
            }
            in_gap = false;
            consecutive += 1;
            pidx += 1;
            if pidx == pattern.len() {
                break;
            }
        } else {
            if in_gap {
                score += SCORE_GAP_EXT;
            } else {
                score += SCORE_GAP_START;
            }
            in_gap = true;
            consecutive = 0;
            first_bonus = 0;
        }
        previous = Some(current);
    }

    (score, positions)
}

#[inline]
fn is_overly_sparse_match(pattern_len: usize, start: usize, end: usize) -> bool {
    end.saturating_sub(start) > pattern_len.saturating_mul(MAX_MATCH_SPAN_PER_PATTERN_BYTE)
}

#[inline]
fn bonus_for(previous: Option<u8>, current: u8) -> i16 {
    match previous {
        None => BONUS_BOUNDARY_WHITE,
        Some(b' ') | Some(b'\t') => BONUS_BOUNDARY_WHITE,
        Some(b'/') | Some(b'\\') | Some(b':') | Some(b';') | Some(b',') => BONUS_BOUNDARY_DELIMITER,
        Some(b'_') | Some(b'-') | Some(b'.') => BONUS_BOUNDARY,
        Some(prev) if prev.is_ascii_lowercase() && current.is_ascii_uppercase() => BONUS_CAMEL123,
        Some(prev) if prev.is_ascii_digit() && !current.is_ascii_digit() => BONUS_CAMEL123,
        Some(prev) if !prev.is_ascii_alphanumeric() => BONUS_NON_WORD,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_ordered_path_subsequence() {
        let mut matcher = FzfMatcher::new();
        let matches = matcher.find_parts(b"fb", &[b"/home/unvdb/foo/bar.rs"]);

        assert_eq!(matches.len(), 1);
        assert_eq!(
            &b"/home/unvdb/foo/bar.rs"[matches[0].start..matches[0].end],
            b"foo/b"
        );
    }

    #[test]
    fn matches_fzf_v1_ascii_scores() {
        let mut matcher = FzfMatcher::new();

        let cases: &[(&[u8], &[u8], usize, usize, i16, &[usize])] = &[
            (b"fooBarbaz1", b"oBZ", 2, 9, 49, &[2, 3, 8]),
            (b"foo bar baz", b"fbb", 0, 9, 78, &[0, 4, 8]),
            (
                b"/AutomatorDocument.icns",
                b"rdoc",
                9,
                13,
                79,
                &[9, 10, 11, 12],
            ),
            (b"/man1/zshcompctl.1", b"zshc", 6, 10, 109, &[6, 7, 8, 9]),
            (b"/.oh-my-zsh/cache", b"zshc", 8, 13, 102, &[8, 9, 10, 12]),
        ];

        for &(text, pattern, start, end, score, positions) in cases {
            let matched = matcher.find_best(pattern, &[text]).unwrap();
            assert_eq!(
                (matched.start, matched.end, matched.score),
                (start, end, score),
                "{} / {}",
                String::from_utf8_lossy(text),
                String::from_utf8_lossy(pattern)
            );
            assert_eq!(matched.positions, positions);
        }
    }

    #[test]
    fn rejects_wrong_order() {
        let mut matcher = FzfMatcher::new();
        let matches = matcher.find_parts(b"abc", &[b"/tmp/c/b/a"]);

        assert!(matches.is_empty());
    }

    #[test]
    fn prefers_path_boundary_over_flat_gap() {
        let mut matcher = FzfMatcher::new();
        let boundary = matcher.find_parts(b"fb", &[b"/tmp/foo/bar.rs"])[0].score;
        let flat = matcher.find_parts(b"fb", &[b"/tmp/fooxxxbar.rs"])[0].score;

        assert!(boundary > flat);
    }

    #[test]
    fn matches_case_insensitive_ascii() {
        let mut matcher = FzfMatcher::new();
        let matches = matcher.find_parts(b"fb", &[b"/tmp/Foo/Bar.rs"]);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].positions, [5, 9]);
    }

    #[test]
    fn supports_cross_slice_without_copy() {
        let mut matcher = FzfMatcher::new();
        let matches = matcher.find_parts(b"abc", &[b"/a", b"/b", b"/c"]);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 1);
        assert_eq!(matches[0].end, 6);
        assert_eq!(matches[0].positions, [1, 3, 5]);
    }

    #[test]
    fn rejects_overly_sparse_path_match() {
        let mut matcher = FzfMatcher::new();
        let path = b"/home/unvdb/cproject/ak-udb-tx-h/src/test/locale/koi8-to-win1251/expected/test-koi8-varchar.sql.out";
        let matches = matcher.find_parts(b"parse", &[path]);

        assert!(matches.is_empty(), "unexpected sparse match: {:?}", matches);
    }

    #[test]
    #[ignore = "benchmark-style test; run with --release -- --ignored --nocapture"]
    fn bench_fzf_like_vs_smithwaterman() {
        use crate::fuzzy::smithwaterman::SmithWaterman;
        use std::hint::black_box;
        use std::time::Instant;

        let rounds = if cfg!(debug_assertions) { 2 } else { 20 };
        let lines: Vec<Vec<u8>> = (0..20_000)
            .map(|i| {
                format!("/home/unvdb/cproject/crates/module_{i}/src/bin/main_{i}.rs").into_bytes()
            })
            .collect();
        let pattern = b"srcmain";

        let mut fzf = FzfMatcher::new();
        let start = Instant::now();
        let mut fzf_found = 0usize;
        for _ in 0..rounds {
            for line in &lines {
                if fzf
                    .find_best(black_box(pattern), black_box(&[line.as_slice()]))
                    .is_some()
                {
                    fzf_found += 1;
                }
            }
        }
        let fzf_elapsed = start.elapsed();

        let mut sw = SmithWaterman::new();
        let start = Instant::now();
        let mut sw_found = 0usize;
        for _ in 0..rounds {
            for line in &lines {
                if !sw
                    .find_parts(black_box(pattern), black_box(&[line.as_slice()]))
                    .is_empty()
                {
                    sw_found += 1;
                }
            }
        }
        let sw_elapsed = start.elapsed();

        let total = rounds * lines.len();
        eprintln!(
            "fzf_like_filter: rounds={rounds} lines={} found={} elapsed={:?} ns/line={}",
            lines.len(),
            fzf_found,
            fzf_elapsed,
            fzf_elapsed.as_nanos() / total as u128
        );
        eprintln!(
            "smith_waterman: rounds={rounds} lines={} found={} elapsed={:?} ns/line={}",
            lines.len(),
            sw_found,
            sw_elapsed,
            sw_elapsed.as_nanos() / total as u128
        );

        assert_eq!(fzf_found, total);
    }
}
