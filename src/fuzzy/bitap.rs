use std::{collections::HashMap, usize};

// https://en.wikipedia.org/wiki/Bitap_algorithm
#[derive(Debug)]
struct Match {
    pub distance: usize,
    pub end: usize,
}

struct Bitap {
    length: usize,
    masks: HashMap<char, usize>,
}

impl Bitap {
    fn new(pattern: &str) -> Bitap {
        let mut masks = HashMap::new();
        let mut length = 0;
        for (i, b) in pattern.chars().enumerate() {
            masks
                .entry(b)
                .and_modify(|mask| *mask &= !(1usize << i))
                .or_insert(!(1usize << i));
            length += 1;
        }
        Bitap {
            length: length,
            masks: masks,
        }
    }

    fn search<'a>(&'a self, text: &'a str) -> impl Iterator<Item = usize> + 'a {
        let m = self.length;
        let mut r: usize = !1usize; // Ini
        let matches = text.chars().enumerate().filter_map(move |(i, b)| {
            r |= self.masks.get(&b).unwrap_or(&!0usize);
            r <<= 1;
            if (r & (1usize << m)) == 0 {
                return Some(i + 1 - m); // Return the start index of the match
            }
            None
        });
        matches
    }

    fn fuzzy_search<'a>(
        &'a self,
        text: &'a str,
        max_distance: usize,
    ) -> impl Iterator<Item = Match> + 'a {
        let m = self.length;
        let max_distance = std::cmp::min(max_distance, m);
        let mut r: Vec<usize> = (0..=max_distance).map(|i| !1usize << i).collect();
        let matches = text.chars().enumerate().filter_map(move |(i, b)| {
            let mask = self.masks.get(&b).unwrap_or(&!0usize);
            let mut prev_parent = r[0];
            r[0] |= mask;
            r[0] <<= 1;
            for j in 1..r.len() {
                let prev = r[j];
                let current = (prev | mask) << 1;
                let replace = prev_parent << 1;
                let delete = r[j - 1] << 1;
                let insert = prev_parent;
                r[j] = current & insert & delete & replace;
                prev_parent = prev;
            }
            for (k, rv) in r.iter().enumerate() {
                if rv & (1usize << m) == 0 {
                    return Some(Match {
                        distance: k,
                        end: i,
                    });
                }
            }
            None
        });
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_bitapsearch() {
        let p = "跳去前几行字";
        let t = "如果你想从一个字符串中跳过前几个字并从之后的位置开始截取";
        let actual = Bitap::new(p).fuzzy_search(t, 2).collect::<Vec<_>>();
        println!("{:?}: find({:?}, {:?})", actual, p, t);
    }

    #[test]
    fn test_lev_search() {
        let test_cases = vec![
            ("hello world", "world", 0, Some(6)),      // 基本匹配
            ("hello world", "worlf", 1, Some(6)),      // 允许的编辑距离
            ("hello world", "hello wold", 1, Some(0)), // 缺失字符
            ("hell world", "hello", 1, Some(0)),       // 插入字符
            ("ababcababc", "abc", 0, Some(2)),         // 多个匹配
            ("hello", "world", 0, None),               // 无匹配
            ("abcdef", "abcdef", 0, Some(0)),          // 完全相同
            ("abc", "abcdef", 0, None),                // 模式长于文本
            ("hello", "", 0, Some(0)),                 // 边界情况：空模式
            ("", "a", 0, None),                        // 边界情况：空文本
        ];

        for (text, pattern, k, expected) in test_cases {
            for i in Bitap::new(pattern).fuzzy_search(text, k) {
                println!("text:{},pattern:{},{:?}", text, pattern, i);
            }
        }

        //println!("{:?}", actual);
    }
}
