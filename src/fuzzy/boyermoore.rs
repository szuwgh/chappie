use std::cmp::max;
// https://go.dev/src/strings/search.go
// https://en.wikipedia.org/wiki/Boyer-Moore_string_search_algorithm

pub(crate) struct BoyerMoore<'a> {
    pattern: &'a str,

    bad_char_skip: [usize; 256],

    good_suffix_skip: Vec<usize>,
}

impl<'a> BoyerMoore<'a> {
    pub(crate) fn new(pattern: &str) -> BoyerMoore {
        let pattern_bytes = pattern.as_bytes();
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

    pub(crate) fn find(&'a self, text: &'a str) -> impl Iterator<Item = usize> + 'a {
        let text_bytes = text.as_bytes();
        let mut i = self.pattern.as_bytes().len() - 1;
        std::iter::from_fn(move || {
            while i < text_bytes.len() {
                let mut j = self.pattern.as_bytes().len() - 1;
                while text_bytes[i] == self.pattern.as_bytes()[j] {
                    if j == 0 {
                        let match_pos = i;
                        i = i + self.pattern.as_bytes().len(); // Skip ahead by pattern length
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

    #[test]
    fn test_longest_common_suffix() {
        let i = longest_common_suffix_bytes("ababc".as_bytes(), "babc".as_bytes());
        println!("{}", i);
    }

    #[test]
    fn test_boyermoore() {
        let bm = BoyerMoore::new("abc");
        let i: Vec<usize> = bm.find("abcadceagedcabcge").collect();
        println!("{:?}", i);
    }

    #[test]
    fn test_boyermoore2() {
        let pattern = "英文";
        let bm = BoyerMoore::new(pattern);
        let text = "12396874,这是中文文本，包含一些特殊字符：@#%&*()，以及英文文字: Hello World! <>/。阿拉伯文: السلام عليكم。英文,韩文: 안녕하세요。日文: こんにちは。#RustExample 英文";
        for i in bm.find(text) {
            println!(
                "{},{:?}",
                i,
                String::from_utf8_lossy(&text.as_bytes()[i..i + pattern.as_bytes().len()])
            );
        }
    }
}
