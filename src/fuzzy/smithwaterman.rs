use std::cmp::max;

const MATCH: i16 = 3;
const MISMATCH: i16 = -2;
const GAP: i16 = -1;
pub(crate) const MAXDIMS: usize = 9182;
const MINDIMS: usize = 512;

// Opt-3: Vec 缓冲区存入结构体，find() 只 clear+extend，不再重复 malloc/free
pub(crate) struct SmithWaterman<'a> {
    cache: &'a mut [i16],
    n: usize,
    pattern_chars: Vec<char>,
    text_chars: Vec<char>,
    byte_offsets: Vec<usize>,
    pos: Vec<(usize, usize)>,
}

impl<'a> SmithWaterman<'a> {
    pub(crate) fn new(cache: &'a mut [i16]) -> SmithWaterman<'a> {
        SmithWaterman {
            cache,
            n: 0,
            pattern_chars: Vec::new(),
            text_chars: Vec::new(),
            byte_offsets: Vec::new(),
            pos: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Match {
    pub score: i16,
    pub start: usize,
    pub end: usize,
}

impl<'a> SmithWaterman<'a> {
    pub(crate) fn find(&mut self, pattern: &str, text: &str) -> Vec<Match> {
        // Opt-3: clear 复用，不重新分配
        self.pattern_chars.clear();
        self.pattern_chars.extend(pattern.chars());
        self.text_chars.clear();
        self.text_chars.extend(text.chars());
        let len1 = self.pattern_chars.len();
        let len2 = self.text_chars.len();

        let m = (len1 + 1) * (len2 + 1);
        if m > MAXDIMS {
            panic!("Cannot be larger than the maximum dimension 9182");
        }

        self.byte_offsets.clear();
        self.byte_offsets.extend(text.char_indices().map(|(i, _)| i));

        let col = len2 + 1;
        let alloc = &mut self.cache[..m];
        alloc.fill(0);

        let mut max_score = 0i16;
        self.pos.clear();
        let mut pattern_len = 0usize;

        for (i, &c1) in self.pattern_chars.iter().enumerate() {
            // Opt-2: 行偏移提到外层循环，消除内层重复乘法
            let row_i  = i * col;
            let row_i1 = row_i + col;

            for (j, &c2) in self.text_chars.iter().enumerate() {
                let score = if c1 == c2 { MATCH } else { MISMATCH };
                // Opt-1+2: get_unchecked + 预算行偏移，消除 O(m×n) bounds check 与乘法
                // SAFETY: row_i+j < (len1+1)*(len2+1) == alloc.len() 由上方 panic 保证
                let (a, b, c) = unsafe {(
                    *alloc.get_unchecked(row_i  + j),     // get(i,   j)
                    *alloc.get_unchecked(row_i  + j + 1), // get(i,   j+1)
                    *alloc.get_unchecked(row_i1 + j),     // get(i+1, j)
                )};
                let cur_score = max(0, max(a + score, max(b + GAP, c + GAP)));
                unsafe { *alloc.get_unchecked_mut(row_i1 + j + 1) = cur_score; }

                if cur_score > max_score {
                    max_score = cur_score;
                    self.pos.clear();
                    self.pos.push((i + 1, j + 1));
                } else if cur_score == max_score && max_score > 0 {
                    // Opt-4: max_score==0 时不积累无效位置
                    self.pos.push((i + 1, j + 1));
                }
            }
            pattern_len += 1;
        }

        let mut matchs: Vec<Match> = Vec::new();
        if max_score >= (2 * pattern_len) as i16 {
            for &(max_i, max_j) in self.pos.iter() {
                let mut i = max_i;
                let mut j = max_j;
                while i > 0 && j > 0 {
                    // SAFETY: i <= len1, j <= len2，均在 alloc 范围内
                    let cur = unsafe { *alloc.get_unchecked(i * col + j) };
                    if cur == 0 { break; }
                    let diag = if self.pattern_chars[i - 1] == self.text_chars[j - 1] {
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

                let start = self.byte_offsets.get(j).copied().unwrap_or(0);
                let end = self.byte_offsets.get(max_j).copied().unwrap_or(text.len());
                matchs.push(Match { score: max_score, start, end });
            }
        }
        matchs.sort_by(|a, b| a.start.cmp(&b.start));
        matchs
    }
}

#[cfg(test)]
mod tests {
    use core::str;
    use super::*;

    // ── ASCII exact ──────────────────────────────────────────────────────────

    #[test]
    fn test_exact_ascii_middle() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "say hello world";
        let matches = sw.find("hello", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "hello");
        assert_eq!(matches[0].score, MATCH * 5);
    }

    #[test]
    fn test_exact_ascii_at_start() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "hello world";
        let matches = sw.find("hello", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 0);
        assert_eq!(&text[matches[0].start..matches[0].end], "hello");
    }

    #[test]
    fn test_exact_ascii_at_end() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "say hello";
        let matches = sw.find("hello", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].end, text.len());
        assert_eq!(&text[matches[0].start..matches[0].end], "hello");
    }

    #[test]
    fn test_exact_ascii_full_text() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "hello";
        let matches = sw.find("hello", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].end, text.len());
        assert_eq!(matches[0].score, MATCH * 5);
    }

    // ── score verification ───────────────────────────────────────────────────

    #[test]
    fn test_score_one_substitution() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        // "hxllo" vs "hello": 4 MATCH + 1 MISMATCH = 4*3 + 1*(-2) = 10
        let matches = sw.find("hello", "hxllo");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].score, 4 * MATCH + MISMATCH); // 10
        assert_eq!(&"hxllo"[matches[0].start..matches[0].end], "hxllo");
    }

    #[test]
    fn test_no_match_two_substitutions() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        // "hxxlo" vs "hello": 3*3 + 2*(-2) = 5, threshold = 10 → no match
        let matches = sw.find("hello", "hxxlo");
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_no_match_completely_different() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let matches = sw.find("zzzzz", "abcdefgh");
        assert_eq!(matches.len(), 0);
    }

    // ── byte offset correctness ──────────────────────────────────────────────

    #[test]
    fn test_byte_offset_ascii_middle() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "abc hello xyz";
        let matches = sw.find("hello", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 4); // "abc " = 4 bytes
        assert_eq!(matches[0].end, 9);   // "abc hello" = 9 bytes
        assert_eq!(&text[matches[0].start..matches[0].end], "hello");
    }

    #[test]
    fn test_byte_offset_after_multibyte_chars() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        // "中" = 3 bytes, "文" = 3 bytes → "中文" = 6 bytes before "hello"
        let text = "中文hello";
        let matches = sw.find("hello", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 6);
        assert_eq!(&text[matches[0].start..matches[0].end], "hello");
    }

    #[test]
    fn test_byte_offset_mixed_cjk_ascii() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "abc端口def";
        let matches = sw.find("端口", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 3); // "abc" = 3 bytes
        assert_eq!(&text[matches[0].start..matches[0].end], "端口");
    }

    // ── Unicode exact matches ────────────────────────────────────────────────

    #[test]
    fn test_chinese_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "检查端口是否启动的函数";
        let matches = sw.find("端口", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "端口");
    }

    #[test]
    fn test_japanese_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "こんにちは世界";
        let matches = sw.find("にちは", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "にちは");
    }

    #[test]
    fn test_korean_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "안녕하세요 세계";
        let matches = sw.find("하세요", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "하세요");
    }

    #[test]
    fn test_arabic_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "السلام عليكم";
        // "لسلام" is a substring starting at char index 1
        let matches = sw.find("لسلام", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "لسلام");
    }

    #[test]
    fn test_chinese_unicode_score() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "端口";
        let matches = sw.find("端口", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].score, MATCH * 2); // 6
    }

    // ── numbers and special characters ──────────────────────────────────────

    #[test]
    fn test_numbers_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "abc12345def";
        let matches = sw.find("12345", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "12345");
    }

    #[test]
    fn test_numbers_fuzzy() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        // "12x45" vs "12345": 4 match + 1 mismatch = 10, threshold = 10
        let text = "abc12345def";
        let matches = sw.find("12x45", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "12345");
    }

    // ── case sensitivity ─────────────────────────────────────────────────────

    #[test]
    fn test_case_sensitive_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        // uppercase "HELLO" should not match lowercase "hello" pattern
        // All 5 chars mismatch: score = 5*(-2) = -10, clamped to 0
        let matches = sw.find("hello", "HELLO WORLD");
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_case_sensitive_partial() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        // Pattern "hello" vs text containing "Hello": H≠h is clamped to 0 in local alignment,
        // so the alignment starts from 'e'. Score = 4*3 = 12, threshold = 10. Match is "ello".
        let text = "say Hello";
        let matches = sw.find("hello", text);
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "ello");
    }

    // ── sorted output ────────────────────────────────────────────────────────

    #[test]
    fn test_results_sorted_by_start() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "hello world hello";
        let matches = sw.find("hello", text);
        assert!(!matches.is_empty());
        for i in 1..matches.len() {
            assert!(matches[i].start >= matches[i - 1].start);
        }
    }

    // ── cache reuse across calls ─────────────────────────────────────────────

    #[test]
    fn test_reuse_across_calls() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);

        let text = "say hello world";
        let m1 = sw.find("hello", text);
        assert_eq!(&text[m1[0].start..m1[0].end], "hello");

        let m2 = sw.find("world", text);
        assert_eq!(&text[m2[0].start..m2[0].end], "world");

        let m3 = sw.find("say", text);
        assert_eq!(&text[m3[0].start..m3[0].end], "say");
    }

    // ── valid UTF-8 slices for fuzzy Unicode ─────────────────────────────────

    #[test]
    fn test_fuzzy_chinese_valid_utf8_slice() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "如果你想从一个字符串中跳过前几个字，并从之后的位跳当前几个字";
        let pattern = "跳去前几行字";
        let matches = sw.find(pattern, text);
        // Verify all returned slices are valid UTF-8 (no panic on indexing)
        for m in &matches {
            let slice = &text[m.start..m.end];
            assert!(std::str::from_utf8(slice.as_bytes()).is_ok());
        }
    }

    #[test]
    fn test_mixed_unicode_valid_slices() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
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
            let matches = sw.find(pattern, text);
            for m in &matches {
                let slice = &text[m.start..m.end];
                assert!(
                    std::str::from_utf8(slice.as_bytes()).is_ok(),
                    "pattern={pattern} produced invalid UTF-8 slice [{}, {}]",
                    m.start, m.end
                );
            }
        }
    }

    // ── long text ────────────────────────────────────────────────────────────

    #[test]
    fn test_long_text_exact() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "Elasticsearch is a distributed search and analytics engine, scalable data store and vector database optimized for speed and relevance on production-scale workloads.";
        let pattern = "applications"; // not present → should be empty or fuzzy
        let matches = sw.find(pattern, text);
        // Just verify no panic; slices must be valid if any
        for m in &matches {
            let _ = &text[m.start..m.end];
        }
    }

    #[test]
    fn test_long_text_fuzzy() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);
        let text = "Elasticsearch is a distributed search and analytics engine, scalable data store and vector database optimized for speed and relevance on production-scale workloads. Elasticsearch is the foundation of Elastics open Stack platform. Search in near real-time over massive datasets, perform vector searches, integrate with generative AI applications, and much more.";
        let pattern = "apelicetions";
        let matches = sw.find(pattern, text);
        for m in &matches {
            let slice = &text[m.start..m.end];
            assert!(std::str::from_utf8(slice.as_bytes()).is_ok());
        }
    }

    // ── keep original regression tests ───────────────────────────────────────

    #[test]
    fn test_smith_waterman() {
        let mut cache = vec![0i16; MAXDIMS];
        let mut sw = SmithWaterman::new(&mut cache);

        let text = "Elasticsearch is a distributed search and analytics engine, scalable data store and vector database optimized for speed and relevance on production-scale workloads. Elasticsearch is the foundation of Elastics open Stack platform. Search in near real-time over massive datasets, perform vector searches, integrate with generative AI applications, and much more.";
        let pattern = "applications";

        let m = sw.find(pattern, text);
        for v in m.iter() {
            println!("{:?}", &text[v.start..v.end]);
        }

        let text = "Elasticsearch is a distributed search and analytics engine, scalable data store and vector database optimized for speed and relevance on production-scale workloads. Elasticsearch is the foundation of Elastics open Stack platform. Search in near real-time over massive datasets, perform vector searches, integrate with generative AI applications, and much more.";
        let pattern = "apelicetions";

        let m = sw.find(pattern, text);
        for v in m.iter() {
            println!(
                "{:?}",
                String::from_utf8_lossy(&text.as_bytes()[v.start..v.end])
            );
        }

        let text = "如果你想从一个字符串中跳过前几个字，并从之后的位跳当前几个字";
        let pattern = "跳去前几行字";
        let m = sw.find(pattern, text);
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
            let m = sw.find(pattern, text);
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

        let m = sw.find(pattern, text);
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
        let mut sw = SmithWaterman::new(&mut cache);

        let text = "hello abc";
        let pattern = "hxlloo";

        let m = sw.find(pattern, text);
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
        let mut sw = SmithWaterman::new(&mut cache);

        let text = "检查端口是否启动的函数";
        let pattern = "端口";

        let m = sw.find(pattern, text);
        for v in m.iter() {
            println!(
                "pattern:{},get:{:?}",
                pattern,
                str::from_utf8(&text.as_bytes()[v.start..v.end]).unwrap()
            );
        }
    }
}
