use crate::fuzzy::fzf::FzfMatcher;
use crate::fuzzy::{CompiledPattern, Match, MatchBounds};
use crate::searcher::memchr::memchr;

const SCORE_MATCH: i16 = 16;
const SCORE_GAP_START: i16 = -3;
const SCORE_GAP_EXT: i16 = -1;
const BONUS_BOUNDARY: i16 = SCORE_MATCH / 2;
const BONUS_NON_WORD: i16 = SCORE_MATCH / 2;
const BONUS_CAMEL123: i16 = BONUS_BOUNDARY + SCORE_GAP_EXT;
const BONUS_CONSECUTIVE: i16 = -(SCORE_GAP_START + SCORE_GAP_EXT);
const BONUS_FIRST_CHAR_MULTIPLIER: i16 = 2;
const BONUS_BOUNDARY_WHITE: i16 = BONUS_BOUNDARY + 2;
const BONUS_BOUNDARY_DELIMITER: i16 = BONUS_BOUNDARY + 1;

const V2_I16_SLAB_LIMIT: usize = 100 * 1024;
const V2_PATTERN_LIMIT: usize = 1000;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CharClass {
    White = 0,
    NonWord = 1,
    Delimiter = 2,
    Lower = 3,
    Upper = 4,
    Letter = 5,
    Number = 6,
}

pub(super) struct FzfV2Matcher {
    pattern: Vec<u8>,
    text: Vec<u8>,
    h0: Vec<i16>,
    c0: Vec<i16>,
    bonus: Vec<i16>,
    first: Vec<usize>,
    h: Vec<i16>,
    c: Vec<i16>,
    positions: Vec<usize>,
    fallback: FzfMatcher,
    with_positions: bool,
}

impl FzfV2Matcher {
    pub(super) fn new() -> Self {
        Self {
            pattern: Vec::new(),
            text: Vec::new(),
            h0: Vec::new(),
            c0: Vec::new(),
            bonus: Vec::new(),
            first: Vec::new(),
            h: Vec::new(),
            c: Vec::new(),
            positions: Vec::new(),
            fallback: FzfMatcher::new(),
            with_positions: true,
        }
    }

    pub(super) fn prepare(&mut self, pattern: &CompiledPattern) {
        self.pattern.clear();
        self.pattern.extend_from_slice(&pattern.bytes);
    }

    pub(super) fn find_best_bounds_prepared(
        &mut self,
        parts: &[&[u8]],
        case_sensitive: bool,
    ) -> Option<MatchBounds> {
        self.with_positions = false;
        self.find_best_prepared(parts, case_sensitive)
            .map(|matched| MatchBounds {
                score: matched.score,
                start: matched.start,
                end: matched.end,
            })
    }

    pub(super) fn find_best_with_positions_prepared(
        &mut self,
        parts: &[&[u8]],
        case_sensitive: bool,
    ) -> Option<Match> {
        self.with_positions = true;
        self.find_best_prepared(parts, case_sensitive)
    }

    #[cfg(test)]
    fn find_best(&mut self, pattern: &[u8], parts: &[&[u8]]) -> Option<Match> {
        let compiled = CompiledPattern {
            bytes: pattern.iter().map(|&byte| ascii_lower(byte)).collect(),
            case_sensitive: false,
        };
        self.prepare(&compiled);
        self.find_best_with_positions_prepared(parts, false)
    }

    #[cfg(test)]
    fn find_parts(&mut self, pattern: &[u8], parts: &[&[u8]]) -> Vec<Match> {
        self.find_best(pattern, parts).into_iter().collect()
    }

    #[cfg(test)]
    fn find_best_case_sensitive(&mut self, pattern: &[u8], parts: &[&[u8]]) -> Option<Match> {
        let compiled = CompiledPattern {
            bytes: pattern.to_vec(),
            case_sensitive: true,
        };
        self.prepare(&compiled);
        self.find_best_with_positions_prepared(parts, true)
    }

    fn find_best_prepared(&mut self, parts: &[&[u8]], case_sensitive: bool) -> Option<Match> {
        let pattern_len = self.pattern.len();
        let text_len = parts_len(parts);
        if pattern_len == 0 || pattern_len > text_len {
            return None;
        }

        if pattern_len > V2_PATTERN_LIMIT
            || pattern_len.saturating_mul(text_len) > V2_I16_SLAB_LIMIT
        {
            if case_sensitive {
                return self.greedy_fallback(parts, true);
            }
            return if self.with_positions {
                self.fallback
                    .find_best_with_positions_prepared(parts, false)
            } else {
                self.fallback
                    .find_best_bounds_prepared(parts, false)
                    .map(|bounds| Match {
                        score: bounds.score,
                        start: bounds.start,
                        end: bounds.end,
                        positions: Vec::new(),
                    })
            };
        }

        if pattern_len == 1 {
            return self.find_single(parts, case_sensitive);
        }

        let (min_idx, max_idx) = ascii_fuzzy_index(parts, &self.pattern, case_sensitive)?;
        if pattern_len == 2 {
            return self.find_two(parts, min_idx, max_idx, case_sensitive);
        }
        self.find_general(parts, min_idx, max_idx, case_sensitive)
    }

    fn find_single(&mut self, parts: &[&[u8]], case_sensitive: bool) -> Option<Match> {
        let pattern = self.pattern[0];
        let mut idx = 0usize;
        let mut max_score = 0i16;
        let mut max_score_pos = None;

        while let Some(pos) = find_next(parts, idx, pattern, case_sensitive) {
            let class = char_class(byte_at(parts, pos));
            let prev_class = if pos == 0 {
                CharClass::White
            } else {
                char_class(byte_at(parts, pos - 1))
            };
            let bonus = bonus_for(prev_class, class);
            let score = SCORE_MATCH + bonus * BONUS_FIRST_CHAR_MULTIPLIER;
            if score > max_score {
                max_score = score;
                max_score_pos = Some(pos);
                if bonus >= BONUS_BOUNDARY {
                    break;
                }
            }
            idx = pos + 1;
        }

        let pos = max_score_pos?;
        Some(Match {
            score: max_score,
            start: pos,
            end: pos + 1,
            positions: if self.with_positions {
                vec![pos]
            } else {
                Vec::new()
            },
        })
    }

    fn find_two(
        &mut self,
        parts: &[&[u8]],
        min_idx: usize,
        max_idx: usize,
        case_sensitive: bool,
    ) -> Option<Match> {
        let n = max_idx - min_idx;
        self.h0.resize(n, 0);
        self.c0.resize(n, 0);
        self.h.resize(n, 0);
        self.c.resize(n, 0);

        let pchar0 = self.pattern[0];
        let pchar1 = self.pattern[1];
        let mut max_score = 0i16;
        let mut max_score_pos = 0usize;
        let mut prev_class = CharClass::White;
        let mut f0 = None;
        let mut f1 = None;
        let mut h0_prev = 0i16;
        let mut c0_prev = 0i16;
        let mut bonus_prev = 0i16;
        let mut in_gap0 = false;
        let mut h1_prev = 0i16;
        let mut in_gap1 = false;

        for off in 0..n {
            let pos = min_idx + off;
            let byte = byte_at(parts, pos);
            let class = char_class(byte);
            let lower = if case_sensitive {
                byte
            } else {
                ascii_lower(byte)
            };
            let bonus = bonus_for(prev_class, class);
            prev_class = class;

            if f0.is_none() {
                if lower == pchar0 {
                    f0 = Some(off);
                }
            } else if f1.is_none() && lower == pchar1 {
                f1 = Some(off);
            }

            let (h0_cur, c0_cur) = if lower == pchar0 {
                (SCORE_MATCH + bonus * BONUS_FIRST_CHAR_MULTIPLIER, 1i16)
            } else {
                let score = if in_gap0 {
                    h0_prev + SCORE_GAP_EXT
                } else {
                    h0_prev + SCORE_GAP_START
                }
                .max(0);
                in_gap0 = true;
                (score, 0)
            };
            if lower == pchar0 {
                in_gap0 = false;
            }
            self.h0[off] = h0_cur;
            self.c0[off] = c0_cur;

            if let Some(f1_pos) = f1 {
                if off >= f1_pos {
                    let hleft = if off == f1_pos { 0 } else { h1_prev };
                    let s2 = if in_gap1 {
                        hleft + SCORE_GAP_EXT
                    } else {
                        hleft + SCORE_GAP_START
                    };
                    let mut s1 = 0i16;
                    let mut consecutive = 0i16;

                    if lower == pchar1 {
                        s1 = h0_prev + SCORE_MATCH;
                        let mut b = bonus;
                        consecutive = c0_prev + 1;
                        if consecutive > 1 {
                            if b >= BONUS_BOUNDARY && b > bonus_prev {
                                consecutive = 1;
                            } else {
                                b = b.max(BONUS_CONSECUTIVE).max(bonus_prev);
                            }
                        }
                        if s1 + b < s2 {
                            s1 += bonus;
                            consecutive = 0;
                        } else {
                            s1 += b;
                        }
                    }

                    in_gap1 = s1 < s2;
                    let score = s1.max(s2).max(0);
                    if score > max_score {
                        max_score = score;
                        max_score_pos = off;
                    }
                    h1_prev = score;
                    self.h[off] = score;
                    self.c[off] = consecutive;
                }
            }

            h0_prev = h0_cur;
            c0_prev = c0_cur;
            bonus_prev = bonus;
        }

        let f0 = f0?;
        let f1 = f1?;
        self.positions.clear();
        let mut row = 1usize;
        let mut j = max_score_pos;
        let mut prefer_match = true;
        loop {
            let (score, diagonal, left, consecutive) = if row == 1 {
                (
                    self.h[j],
                    if j >= f1 { self.h0[j - 1] } else { 0 },
                    if j > f1 { self.h[j - 1] } else { 0 },
                    self.c[j],
                )
            } else {
                (
                    self.h0[j],
                    0,
                    if j > f0 { self.h0[j - 1] } else { 0 },
                    self.c0[j],
                )
            };

            let current_row = row;
            if score > diagonal && (score > left || score == left && prefer_match) {
                self.positions.push(j + min_idx);
                if row == 0 {
                    break;
                }
                row -= 1;
            }
            prefer_match = consecutive > 1
                || current_row == 0 && j + 1 < n && j + 1 >= f1 && self.c[j + 1] > 0;
            if j == 0 {
                break;
            }
            j -= 1;
        }
        self.positions.reverse();
        self.match_from_positions(max_score)
    }

    fn find_general(
        &mut self,
        parts: &[&[u8]],
        min_idx: usize,
        max_idx: usize,
        case_sensitive: bool,
    ) -> Option<Match> {
        let n = max_idx - min_idx;
        let m = self.pattern.len();
        self.text.clear();
        self.h0.resize(n, 0);
        self.c0.resize(n, 0);
        self.bonus.resize(n, 0);
        self.first.resize(m, 0);

        let mut max_score = 0i16;
        let mut max_score_pos = 0usize;
        let mut pidx = 0usize;
        let mut last_idx = 0usize;
        let pchar0 = self.pattern[0];
        let mut pchar = pchar0;
        let mut prev_h0 = 0i16;
        let mut prev_class = CharClass::White;
        let mut in_gap = false;

        for off in 0..n {
            let byte = byte_at(parts, min_idx + off);
            let class = char_class(byte);
            let lower = if case_sensitive {
                byte
            } else {
                ascii_lower(byte)
            };
            self.text.push(lower);

            let bonus = bonus_for(prev_class, class);
            self.bonus[off] = bonus;
            prev_class = class;

            if lower == pchar {
                if pidx < m {
                    self.first[pidx] = off;
                    pidx += 1;
                    pchar = self.pattern[pidx.min(m - 1)];
                }
                last_idx = off;
            }

            if lower == pchar0 {
                let score = SCORE_MATCH + bonus * BONUS_FIRST_CHAR_MULTIPLIER;
                self.h0[off] = score;
                self.c0[off] = 1;
                in_gap = false;
            } else {
                self.h0[off] = if in_gap {
                    prev_h0 + SCORE_GAP_EXT
                } else {
                    prev_h0 + SCORE_GAP_START
                }
                .max(0);
                self.c0[off] = 0;
                in_gap = true;
            }
            prev_h0 = self.h0[off];
        }
        if pidx != m {
            return None;
        }

        let f0 = self.first[0];
        let width = last_idx - f0 + 1;
        let matrix_len = width * m;
        self.h.resize(matrix_len, 0);
        self.c.resize(matrix_len, 0);
        self.h[..width].copy_from_slice(&self.h0[f0..=last_idx]);
        self.c[..width].copy_from_slice(&self.c0[f0..=last_idx]);

        for pidx in 1..m {
            let f = self.first[pidx];
            let row = pidx * width;
            let mut in_gap = false;
            self.h[row + f - f0 - 1] = 0;

            for col in f..=last_idx {
                let j0 = col - f0;
                let hleft_idx = row + j0 - 1;
                let s2 = if in_gap {
                    self.h[hleft_idx] + SCORE_GAP_EXT
                } else {
                    self.h[hleft_idx] + SCORE_GAP_START
                };

                let mut s1 = 0i16;
                let mut consecutive = 0i16;
                if self.pattern[pidx] == self.text[col] {
                    let diag_idx = row - width + j0 - 1;
                    s1 = self.h[diag_idx] + SCORE_MATCH;
                    let mut b = self.bonus[col];
                    consecutive = self.c[diag_idx] + 1;
                    if consecutive > 1 {
                        let first_bonus = self.bonus[col - consecutive as usize + 1];
                        if b >= BONUS_BOUNDARY && b > first_bonus {
                            consecutive = 1;
                        } else {
                            b = b.max(BONUS_CONSECUTIVE).max(first_bonus);
                        }
                    }
                    if s1 + b < s2 {
                        s1 += self.bonus[col];
                        consecutive = 0;
                    } else {
                        s1 += b;
                    }
                }

                self.c[row + j0] = consecutive;
                in_gap = s1 < s2;
                let score = s1.max(s2).max(0);
                if pidx == m - 1 && score > max_score {
                    max_score = score;
                    max_score_pos = col;
                }
                self.h[row + j0] = score;
            }
        }

        self.backtrace(min_idx, f0, last_idx, max_score, max_score_pos, width)
    }

    fn backtrace(
        &mut self,
        min_idx: usize,
        f0: usize,
        last_idx: usize,
        max_score: i16,
        max_score_pos: usize,
        width: usize,
    ) -> Option<Match> {
        self.positions.clear();
        let mut row = self.pattern.len() - 1;
        let mut j = max_score_pos;
        let mut prefer_match = true;

        loop {
            let row_start = row * width;
            let j0 = j - f0;
            let score = self.h[row_start + j0];
            let diagonal = if row > 0 && j >= self.first[row] {
                self.h[row_start - width + j0 - 1]
            } else {
                0
            };
            let left = if j > self.first[row] {
                self.h[row_start + j0 - 1]
            } else {
                0
            };

            let current_row = row;
            if score > diagonal && (score > left || score == left && prefer_match) {
                self.positions.push(j + min_idx);
                if row == 0 {
                    break;
                }
                row -= 1;
            }

            prefer_match = self.c[row_start + j0] > 1
                || current_row + 1 < self.pattern.len()
                    && j < last_idx
                    && j + 1 >= self.first[current_row + 1]
                    && self.c[row_start + width + j0 + 1] > 0;
            if j == f0 {
                break;
            }
            j -= 1;
        }

        self.positions.reverse();
        self.match_from_positions(max_score)
    }

    fn greedy_fallback(&mut self, parts: &[&[u8]], case_sensitive: bool) -> Option<Match> {
        let end = {
            let mut start = 0usize;
            let mut end = 0usize;
            for &byte in &self.pattern {
                let pos = find_next(parts, start, byte, case_sensitive)?;
                end = pos + 1;
                start = end;
            }
            end
        };

        self.positions.clear();
        let mut pos = end;
        for &byte in self.pattern.iter().rev() {
            while pos > 0 {
                pos -= 1;
                let cur = byte_at(parts, pos);
                let cur = if case_sensitive {
                    cur
                } else {
                    ascii_lower(cur)
                };
                if cur == byte {
                    self.positions.push(pos);
                    break;
                }
            }
        }
        self.positions.reverse();
        let score = score_window(parts, &self.positions);
        self.match_from_positions(score)
    }

    fn match_from_positions(&self, score: i16) -> Option<Match> {
        Some(Match {
            score,
            start: *self.positions.first()?,
            end: self.positions.last()? + 1,
            positions: if self.with_positions {
                self.positions.clone()
            } else {
                Vec::new()
            },
        })
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

fn ascii_fuzzy_index(
    parts: &[&[u8]],
    pattern: &[u8],
    case_sensitive: bool,
) -> Option<(usize, usize)> {
    let mut first_idx = 0usize;
    let mut idx = 0usize;
    let mut last_idx = 0usize;
    let mut last_byte = 0u8;

    for (pidx, &byte) in pattern.iter().enumerate() {
        let pos = find_next(parts, idx, byte, case_sensitive)?;
        if pidx == 0 && pos > 0 {
            first_idx = pos - 1;
        }
        last_idx = pos;
        last_byte = byte;
        idx = pos + 1;
    }

    let total_len = parts_len(parts);
    if last_idx + 1 < total_len {
        if let Some(end) = find_last(parts, last_idx + 1, total_len, last_byte, case_sensitive) {
            return Some((first_idx, end + 1));
        }
    }
    Some((first_idx, last_idx + 1))
}

fn find_next(parts: &[&[u8]], start: usize, byte: u8, case_sensitive: bool) -> Option<usize> {
    if case_sensitive || !byte.is_ascii_lowercase() {
        return find_next_exact(parts, start, byte);
    }

    let upper = byte - 32;
    let mut base = 0usize;
    for part in parts {
        let end = base + part.len();
        if end <= start {
            base = end;
            continue;
        }

        let local_start = start.saturating_sub(base);
        let lower_pos = memchr(&part[local_start..], byte);
        let upper_pos = memchr(&part[local_start..], upper);
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

fn find_last(
    parts: &[&[u8]],
    start: usize,
    end: usize,
    byte: u8,
    case_sensitive: bool,
) -> Option<usize> {
    let mut pos = end;
    while pos > start {
        pos -= 1;
        let current = byte_at(parts, pos);
        if if case_sensitive {
            current == byte
        } else {
            ascii_lower(current) == byte
        } {
            return Some(pos);
        }
    }
    None
}

#[inline]
fn byte_at(parts: &[&[u8]], index: usize) -> u8 {
    match parts.len() {
        1 => unsafe { *parts[0].get_unchecked(index) },
        2 => {
            let first = parts[0];
            if index < first.len() {
                unsafe { *first.get_unchecked(index) }
            } else {
                unsafe { *parts[1].get_unchecked(index - first.len()) }
            }
        }
        3 => {
            let first = parts[0];
            let second = parts[1];
            let end0 = first.len();
            let end1 = end0 + second.len();
            if index < end0 {
                unsafe { *first.get_unchecked(index) }
            } else if index < end1 {
                unsafe { *second.get_unchecked(index - end0) }
            } else {
                unsafe { *parts[2].get_unchecked(index - end1) }
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

#[inline]
fn char_class(byte: u8) -> CharClass {
    match byte {
        b'a'..=b'z' => CharClass::Lower,
        b'A'..=b'Z' => CharClass::Upper,
        b'0'..=b'9' => CharClass::Number,
        b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c => CharClass::White,
        b'/' | b'\\' | b':' | b';' | b',' => CharClass::Delimiter,
        0x80..=0xff => CharClass::Letter,
        _ => CharClass::NonWord,
    }
}

#[inline]
fn bonus_for(prev_class: CharClass, class: CharClass) -> i16 {
    if class >= CharClass::NonWord {
        match prev_class {
            CharClass::White => return BONUS_BOUNDARY_WHITE,
            CharClass::Delimiter => return BONUS_BOUNDARY_DELIMITER,
            CharClass::NonWord => return BONUS_BOUNDARY,
            _ => {}
        }
    }

    if prev_class == CharClass::Lower && class == CharClass::Upper
        || prev_class != CharClass::Number && class == CharClass::Number
    {
        return BONUS_CAMEL123;
    }

    match class {
        CharClass::NonWord | CharClass::Delimiter => BONUS_NON_WORD,
        CharClass::White => BONUS_BOUNDARY_WHITE,
        _ => 0,
    }
}

fn score_window(parts: &[&[u8]], positions: &[usize]) -> i16 {
    let mut score = 0i16;
    let mut in_gap = false;
    let mut consecutive = 0i16;
    let mut first_bonus = 0i16;
    let mut pidx = 0usize;
    let start = positions[0];
    let end = positions[positions.len() - 1] + 1;
    let mut pos_iter = positions.iter().copied();
    let mut next_pos = pos_iter.next();
    let mut prev_class = if start > 0 {
        char_class(byte_at(parts, start - 1))
    } else {
        CharClass::White
    };

    for idx in start..end {
        let class = char_class(byte_at(parts, idx));
        if Some(idx) == next_pos {
            score += SCORE_MATCH;
            let mut bonus = bonus_for(prev_class, class);
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
            next_pos = pos_iter.next();
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
        prev_class = class;
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_fzf_v2_ascii_scores() {
        let mut matcher = FzfV2Matcher::new();

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
    fn rejects_case_sensitive_non_match() {
        let mut matcher = FzfV2Matcher::new();
        assert!(matcher
            .find_best_case_sensitive(b"oBZ", &[b"fooBarbaz"])
            .is_none());
    }

    #[test]
    fn supports_cross_slice_input() {
        let mut matcher = FzfV2Matcher::new();
        let parts: &[&[u8]] = &[b"/.oh-", b"my-zsh", b"/cache"];
        let matched = matcher.find_best(b"zshc", parts).unwrap();

        assert_eq!((matched.start, matched.end, matched.score), (8, 13, 102));
        assert_eq!(matched.positions, [8, 9, 10, 12]);
    }

    #[test]
    fn uses_single_and_two_char_fast_paths() {
        let mut matcher = FzfV2Matcher::new();

        let single = matcher.find_best(b"b", &[b"foo bar"]).unwrap();
        assert_eq!((single.start, single.end, single.score), (4, 5, 36));

        let two = matcher.find_best(b"fb", &[b"foobar fb"]).unwrap();
        assert_eq!((two.start, two.end, two.score), (7, 9, 62));
    }

    #[test]
    #[ignore = "benchmark-style test; run with --release -- --ignored --nocapture"]
    fn bench_fzf_v1_fzf_v2_and_smithwaterman() {
        use crate::fuzzy::fzf::FzfMatcher;
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
        let total = rounds * lines.len();

        let mut fzf_v1 = FzfMatcher::new();
        let start = Instant::now();
        let mut v1_found = 0usize;
        for _ in 0..rounds {
            for line in &lines {
                if fzf_v1
                    .find_best(black_box(pattern), black_box(&[line.as_slice()]))
                    .is_some()
                {
                    v1_found += 1;
                }
            }
        }
        let v1_elapsed = start.elapsed();

        let mut fzf_v2 = FzfV2Matcher::new();
        let start = Instant::now();
        let mut v2_found = 0usize;
        for _ in 0..rounds {
            for line in &lines {
                if fzf_v2
                    .find_best(black_box(pattern), black_box(&[line.as_slice()]))
                    .is_some()
                {
                    v2_found += 1;
                }
            }
        }
        let v2_elapsed = start.elapsed();

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

        eprintln!(
            "fzf_v1_filter: rounds={rounds} lines={} found={} elapsed={:?} ns/line={}",
            lines.len(),
            v1_found,
            v1_elapsed,
            v1_elapsed.as_nanos() / total as u128
        );
        eprintln!(
            "fzf_v2_sw: rounds={rounds} lines={} found={} elapsed={:?} ns/line={}",
            lines.len(),
            v2_found,
            v2_elapsed,
            v2_elapsed.as_nanos() / total as u128
        );
        eprintln!(
            "smith_waterman: rounds={rounds} lines={} found={} elapsed={:?} ns/line={}",
            lines.len(),
            sw_found,
            sw_elapsed,
            sw_elapsed.as_nanos() / total as u128
        );

        assert_eq!(v1_found, total);
        assert_eq!(v2_found, total);
        assert_eq!(sw_found, total);
    }
}
