use std::cmp::max;
use std::collections::VecDeque;
// https://go.dev/src/strings/search.go
// https://en.wikipedia.org/wiki/Boyer-Moore_string_search_algorithm

pub(crate) struct BoyerMoore<'a> {
    pattern: &'a [u8],

    bad_char_skip: [usize; 256],

    good_suffix_skip: Vec<usize>,
}

impl<'a> BoyerMoore<'a> {
    pub(crate) fn new(pattern: &'a [u8]) -> BoyerMoore<'a> {
        if pattern.is_empty() {
            panic!("Pattern must not be empty");
        }
        let pattern_bytes = pattern;
        let mut good_suffix_skip = vec![pattern_bytes.len(); pattern_bytes.len()];

        let last = pattern_bytes.len() - 1;
        // 构建坏字符表
        let mut bad_char_skip = [pattern_bytes.len(); 256];

        for i in 0..last {
            bad_char_skip[pattern_bytes[i] as usize] = last - i;
        }
        // 构建好后缀表
        // Build good suffix table.
        let mut last_prefix = last;
        for i in (0..=last).rev() {
            if has_prefix_bytes(pattern_bytes, &pattern_bytes[i + 1..]) {
                last_prefix = i + 1;
            }
            good_suffix_skip[i] = last_prefix + last - i;
        }
        // Second pass: find repeats of pattern's suffix starting from the front.
        for i in 0..last {
            let len_suffix = longest_common_suffix_bytes(pattern_bytes, &pattern_bytes[1..=i]);
            if pattern_bytes[i - len_suffix] != pattern_bytes[last - len_suffix] {
                good_suffix_skip[last - len_suffix] = len_suffix + last - i;
            }
        }
        BoyerMoore {
            pattern: pattern,
            bad_char_skip: bad_char_skip,
            good_suffix_skip: good_suffix_skip,
        }
    }

    pub(crate) fn find(&'a self, text: &'a [u8]) -> Option<usize> {
        let text_bytes = text;
        let mut i = self.pattern.len() - 1;
        while i < text_bytes.len() {
            let mut j = self.pattern.len() - 1;
            while text_bytes[i] == self.pattern[j] {
                if j == 0 {
                    let match_pos = i;
                    //i = i + self.pattern.len(); // Skip ahead by pattern length
                    return Some(match_pos);
                }
                i -= 1;
                j -= 1;
            }
            let shift = max(
                self.bad_char_skip[text_bytes[i] as usize],
                self.good_suffix_skip[j],
            );
            i += shift;
        }
        None
    }

    pub(crate) fn search(&'a self, text: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
        let text_bytes = text;
        let mut i = self.pattern.len() - 1;
        std::iter::from_fn(move || {
            while i < text_bytes.len() {
                let mut j = self.pattern.len() - 1;
                while text_bytes[i] == self.pattern[j] {
                    if j == 0 {
                        let match_pos = i;
                        i = i + self.pattern.len(); // Skip ahead by pattern length
                        return Some(match_pos);
                    }
                    i -= 1;
                    j -= 1;
                }
                let shift = max(
                    self.bad_char_skip[text_bytes[i] as usize],
                    self.good_suffix_skip[j],
                );
                i += shift;
            }
            None
        })
    }

    pub(crate) fn stream<'b, I>(&'a self, mut text: I) -> impl Iterator<Item = usize> + 'b
    where
        I: Iterator<Item = u8> + 'b,
        'a: 'b,
    {
        let pattern_bytes = self.pattern;
        let bad_char_skip = &self.bad_char_skip;
        let good_suffix_skip = &self.good_suffix_skip;
        let m = pattern_bytes.len();
        let mut window = VecDeque::with_capacity(m);
        let mut idx = 0;

        // Pre-fill initial window
        while window.len() < m {
            match text.next() {
                Some(b) => window.push_back(b),
                None => break,
            }
        }

        std::iter::from_fn(move || {
            while window.len() == m {
                // Compare from end using usize, avoid isize casts in hot path.
                let mut j = m;
                while j > 0 && window[j - 1] == pattern_bytes[j - 1] {
                    j -= 1;
                }

                if j == 0 {
                    // Match at idx
                    let match_pos = idx;
                    // Slide by pattern length
                    for _ in 0..m {
                        let _ = window.pop_front();
                        if let Some(b) = text.next() {
                            window.push_back(b);
                        }
                    }
                    idx += m;
                    return Some(match_pos);
                } else {
                    // Compute shift
                    let mismatch = j - 1;
                    let bad = bad_char_skip[window[mismatch] as usize];
                    let good = good_suffix_skip[mismatch];
                    let shift = bad.max(good);

                    // Slide window by shift
                    for _ in 0..shift {
                        let _ = window.pop_front();
                        if let Some(b) = text.next() {
                            window.push_back(b);
                            idx += 1;
                        } else {
                            // Not enough data
                            return None;
                        }
                    }
                }
            }
            None
        })
    }
}

fn longest_common_suffix_bytes(a: &[u8], b: &[u8]) -> usize {
    let mut i = 0;
    while i < a.len() && i < b.len() && a[a.len() - 1 - i] == b[b.len() - 1 - i] {
        i += 1;
    }
    i
}

fn has_prefix_bytes(s: &[u8], prefix: &[u8]) -> bool {
    s.len() >= prefix.len() && &s[0..prefix.len()] == prefix
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::hint::black_box;
    use std::time::Instant;

    fn naive_find(text: &[u8], pattern: &[u8]) -> Option<usize> {
        if pattern.is_empty() {
            return Some(0);
        }
        if pattern.len() > text.len() {
            return None;
        }
        text.windows(pattern.len())
            .position(|window| window == pattern)
    }

    fn assert_find_eq_naive(text: &[u8], pattern: &[u8]) {
        let bm = BoyerMoore::new(pattern);
        assert_eq!(
            bm.find(text),
            naive_find(text, pattern),
            "text={text:?}, pattern={pattern:?}",
        );
    }

    fn generated_bytes(len: usize, seed: u64) -> Vec<u8> {
        let mut x = seed | 1;
        let mut out = Vec::with_capacity(len);

        for _ in 0..len {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
            out.push((x >> 32) as u8);
        }

        out
    }

    #[test]
    fn find_returns_first_match_for_basic_positions() {
        assert_find_eq_naive(b"abc", b"a");
        assert_find_eq_naive(b"abc", b"abc");
        assert_find_eq_naive(b"xxabc", b"abc");
        assert_find_eq_naive(b"abcxx", b"abc");
        assert_find_eq_naive(b"xxabcxx", b"abc");
        assert_find_eq_naive(b"abcabc", b"abc");
        assert_find_eq_naive(b"aaaaa", b"aa");
    }

    #[test]
    fn find_returns_none_when_pattern_is_absent_or_too_long() {
        assert_find_eq_naive(b"", b"a");
        assert_find_eq_naive(b"abc", b"d");
        assert_find_eq_naive(b"abc", b"abcd");
        assert_find_eq_naive(b"aaaaa", b"b");
        assert_find_eq_naive(b"abababab", b"abba");
    }

    #[test]
    fn find_handles_binary_bytes() {
        let text = [
            0x00, 0xff, 0x10, 0x20, 0x00, 0xff, 0x10, 0x21, 0x80, 0x00, 0xff,
        ];

        assert_find_eq_naive(&text, &[0x00]);
        assert_find_eq_naive(&text, &[0xff, 0x10]);
        assert_find_eq_naive(&text, &[0x00, 0xff, 0x10, 0x21]);
        assert_find_eq_naive(&text, &[0x80, 0x00, 0xff]);
        assert_find_eq_naive(&text, &[0xff, 0xff]);
    }

    #[test]
    fn find_matches_naive_search_for_generated_data() {
        for text_len in 0..128 {
            for pattern_len in 1..=32 {
                for seed in 0..4u64 {
                    let text = generated_bytes(text_len, seed + 0x1234);
                    let pattern = if pattern_len <= text_len && seed % 2 == 0 {
                        let start =
                            (seed as usize * 7 + text_len / 3) % (text_len - pattern_len + 1);
                        text[start..start + pattern_len].to_vec()
                    } else {
                        generated_bytes(pattern_len, seed + 0x5678)
                    };

                    assert_find_eq_naive(&text, &pattern);
                }
            }
        }
    }

    #[test]
    #[should_panic(expected = "Pattern must not be empty")]
    fn new_rejects_empty_pattern() {
        let _ = BoyerMoore::new(b"");
    }

    #[test]
    fn test_longest_common_suffix() {
        let i = longest_common_suffix_bytes("ababc".as_bytes(), "babc".as_bytes());
        println!("{}", i);
    }

    #[test]
    fn test_boyermoore() {
        let bm = BoyerMoore::new("abc".as_bytes());
        let i: Vec<usize> = bm.search("abcadceagedcabcge".as_bytes()).collect();
        println!("{:?}", i);
    }

    #[test]
    fn test_boyermoore2() {
        let pattern = "英文";
        let bm = BoyerMoore::new(pattern.as_bytes());
        let text = "12396874,这是中文文本，包含一些特殊字符：@#%&*()，以及英文文字: Hello World! <>/。阿拉伯文: السلام عليكم。英文,韩文: 안녕하세요。日文: こんにちは。#RustExample 英文";
        for i in bm.find(text.as_bytes()) {
            println!(
                "{},{:?}",
                i,
                String::from_utf8_lossy(&text.as_bytes()[i..i + pattern.as_bytes().len()])
            );
        }
    }

    #[test]
    fn test_boyermoore_stream() {
        let pattern = "英文";
        let bm = BoyerMoore::new(pattern.as_bytes());
        let text = "12396874,这是中文文本，包含一些特殊字符：@#%&*()，以及英文文字: Hello World! <>/。阿拉伯文: السلام عليكم。英文,韩文: 안녕하세요。日文: こんにちは。#RustExample 英文";
        for i in bm.stream(text.as_bytes().iter().copied()) {
            println!(
                "{},{:?}",
                i,
                String::from_utf8_lossy(&text.as_bytes()[i..i + pattern.as_bytes().len()])
            );
        }
    }

    #[test]
    #[ignore = "benchmark-style perf test; run with --ignored --nocapture"]
    fn test_boyermoore_stream_perf() {
        let pattern = b"XYZXYZ12";
        let bm = BoyerMoore::new(pattern);

        let rounds = 200_000usize;
        let mut text = Vec::with_capacity(rounds * 64);
        let mut expected_matches = 0usize;
        for i in 0..rounds {
            text.extend_from_slice(b"abcdefghijklmnopqrstuvwxyz0123456789________");
            if i % 16 == 0 {
                text.extend_from_slice(pattern);
                expected_matches += 1;
            } else {
                text.extend_from_slice(b"........");
            }
        }

        // Warm-up
        let warmup = bm.stream(text.iter().copied()).count();
        assert_eq!(warmup, expected_matches);

        let start = Instant::now();
        let match_count = bm.stream(text.iter().copied()).count();
        let elapsed = start.elapsed();
        black_box(match_count);

        let mb = text.len() as f64 / (1024.0 * 1024.0);
        let throughput = mb / elapsed.as_secs_f64();
        println!(
            "boyermoore::stream perf => bytes={}, matches={}, elapsed={:?}, throughput={:.2} MiB/s",
            text.len(),
            match_count,
            elapsed,
            throughput
        );

        assert_eq!(match_count, expected_matches);
    }
}
