use std::cmp::min;

// 计算两个字符串的编辑距离，适合相似度比较
// 由一个转换成另一个所需的最少编辑操作次数
// 将其中一个字符替换成另一个字符（Substitutions）。
// 插入一个字符（Insertions）。
// 删除一个字符（Deletions）。
fn levenshtein(a: &str, b: &str) -> usize {
    let mut column: Vec<usize> = (0..=a.chars().count()).collect();
    for (x, ch2) in b.chars().enumerate() {
        let mut last_diag = x;
        column[0] = x + 1;
        for (y, ch1) in a.chars().enumerate() {
            let old_diag = column[y + 1];
            let cost = if ch1 == ch2 { 0 } else { 1 };
            column[y + 1] = min3(
                column[y + 1] + 1, // Deletion
                column[y] + 1,     // Insertion
                last_diag + cost,  // Substitution
            );
            last_diag = old_diag;
        }
    }
    column[a.chars().count()]
}

#[inline]
fn min3(a: usize, b: usize, c: usize) -> usize {
    min(min(a, b), c)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestCase {
        s: &'static str,
        t: String,
        wanted: usize,
    }

    #[test]
    fn test_levenshtein() {
        let test_cases = vec![
            TestCase {
                s: "a",
                t: "a".to_string(),
                wanted: 0,
            },
            TestCase {
                s: "ab",
                t: "ab".to_string(),
                wanted: 0,
            },
            TestCase {
                s: "ab",
                t: "aa".to_string(),
                wanted: 1,
            },
            TestCase {
                s: "ab",
                t: "aaa".to_string(),
                wanted: 2,
            },
            TestCase {
                s: "bbb",
                t: "a".to_string(),
                wanted: 3,
            },
            TestCase {
                s: "kitten",
                t: "sitting".to_string(),
                wanted: 3,
            },
            TestCase {
                s: "ёлка",
                t: "ёлочка".to_string(),
                wanted: 2,
            },
            TestCase {
                s: "ветер",
                t: "ёлочка".to_string(),
                wanted: 6,
            },
            TestCase {
                s: "中国",
                t: "中华人民共和国".to_string(),
                wanted: 5,
            },
            TestCase {
                s: "小日本",
                t: "中华人民共和国".to_string(),
                wanted: 7,
            },
            TestCase {
                s: "小日本",
                t: "中华人民共和国".to_string(),
                wanted: 7,
            },
        ];

        for test in test_cases {
            let distance = levenshtein(test.s, &test.t); // t 的引用传递
            assert_eq!(
                distance, test.wanted,
                "got distance {}, expected {} for '{}' in '{}'",
                distance, test.wanted, test.s, test.t
            );
        }
    }
}
