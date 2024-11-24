use std::cmp::max;
use std::cmp::min;

const MATCH: i16 = 3;
const MISMATCH: i16 = -2;
const GAP: i16 = -1;
pub(crate) const MAXDIMS: usize = 9182;
const MINDIMS: usize = 512;

//
pub(crate) struct SmithWaterman<'a> {
    cache: &'a mut [i16],
    n: usize,
}

impl<'a> SmithWaterman<'a> {
    pub(crate) fn new(cache: &'a mut [i16]) -> SmithWaterman<'a> {
        SmithWaterman { cache: cache, n: 0 }
    }
}

#[derive(Debug)]
pub(crate) struct Match {
    pub score: i16,
    pub start: usize,
    pub end: usize,
}

struct Matrix<'a> {
    matrix: &'a mut [i16],
    row: usize,
    col: usize,
}

impl<'a> Matrix<'a> {
    fn new(matrix: &'a mut [i16], i: usize, j: usize) -> Matrix {
        Matrix {
            matrix: matrix,
            row: i,
            col: j,
        }
    }

    fn set(&mut self, i: usize, j: usize, n: i16) {
        self.matrix[i * (self.col) + j] = n
    }

    fn get(&mut self, i: usize, j: usize) -> i16 {
        *self.matrix.get(i * (self.col) + j).unwrap_or(&0) //[i * (self.col) + j]
    }

    fn print(&self) {
        // 打印矩阵
        for row in 0..self.row {
            for col in 0..self.col {
                let index = row * self.col + col;
                print!("{} ", self.matrix[index]);
            }
            println!(); // 换行
        }
    }
}

impl<'a> SmithWaterman<'a> {
    pub(crate) fn find(&mut self, pattern: &str, text: &str) -> Vec<Match> {
        // let pattern = p.as_bytes();
        // let text = t.as_bytes();
        let len1 = pattern.len();
        let len2 = text.len();
        let m = (len1 + 1) * (len2 + 1);
        if m > MAXDIMS {
            panic!("Cannot be larger than the maximum dimension 9182");
        }
        let alloc = &mut self.cache[..m];
        alloc.fill(0);
        let mut score_matrix = Matrix::new(alloc, len1 + 1, len2 + 1);
        let mut max_score = 0;
        let mut pos = Vec::new();
        let mut pattern_len = 0;
        for (i, c1) in pattern.chars().enumerate() {
            for (j, c2) in text.chars().enumerate() {
                let score = if c1 == c2 { MATCH } else { MISMATCH };
                let a = score_matrix.get(i, j);
                let b = score_matrix.get(i, j + 1);
                let c = score_matrix.get(i + 1, j);
                let cur_score = max(0, max(a + score, max(b + GAP, c + GAP)));
                score_matrix.set(i + 1, j + 1, cur_score);
                // 更新最大得分和位置
                if cur_score > max_score {
                    max_score = cur_score;
                    pos.clear();
                    pos.push((i + 1, j + 1));
                } else if cur_score == max_score {
                    pos.push((i + 1, j + 1));
                }
            }
            pattern_len += 1;
        }
        // 输出得分矩阵
        let mut matchs: Vec<Match> = Vec::new();
        // 后向追踪 回溯
        if max_score >= (2 * pattern_len) as i16 {
            for (max_i, max_j) in pos.into_iter() {
                let mut i = max_i;
                let mut j = max_j;
                while i > 0 && j > 0 && score_matrix.get(i, j) > 0 {
                    if score_matrix.get(i, j)
                        == score_matrix.get(i - 1, j - 1)
                            + if pattern.chars().nth(i - 1) == text.chars().nth(j - 1) {
                                MATCH
                            } else {
                                MISMATCH
                            }
                    {
                        i -= 1;
                        j -= 1;
                    } else if score_matrix.get(i, j) == score_matrix.get(i - 1, j) + GAP {
                        i -= 1;
                    } else {
                        j -= 1;
                    }
                }

                matchs.push(Match {
                    score: max_score,
                    start: text.char_indices().nth(j).map(|(i, _)| i).unwrap_or(0),
                    end: text
                        .char_indices()
                        .nth(max_j)
                        .map(|(i, _)| i)
                        .unwrap_or(text.len()),
                });
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
            //  let chars: String = text.chars().skip(v.start).take(v.end - v.start).collect();
            println!(
                "{:?}",
                String::from_utf8_lossy(&text.as_bytes()[v.start..v.end])
            );
        }

        let text = "12396874,这是中文文本，包含一些特殊字符：@#%&*()，以及英文文字: Hello World! <>/。阿拉伯文: السلام عليكم。韩文: 안녕하세요。日文: こんにちは。#RustExample";

        let patterns = vec![
            "ec",          // 中文
            "Hxllo",       // 英文
            "لسلام عليك",  // 阿拉伯文
            "하세요",      // 韩文
            "にちは",      // 日文
            "[@#%&*()]+",  // 特殊字符
            "96974",       // 数字
            "@#?&*",       // 标点符号
            "RustUxample", // 特定字符串
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
            // let v: String = text.chars().skip(v.start).take(v.end - v.start).collect();
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
            // let v: String = text.chars().skip(v.start).take(v.end - v.start).collect();
            println!(
                "pattern:{},get:{:?}",
                pattern,
                str::from_utf8(&text.as_bytes()[v.start..v.end]).unwrap()
            );
        }
    }
}
