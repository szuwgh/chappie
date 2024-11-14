use crate::fuzzy::smithwaterman::MAXDIMS;
use bitap::Bitap;
use boyermoore::BoyerMoore;
use smithwaterman::SmithWaterman;
mod bitap;
mod boyermoore;
mod levenshtein;
mod smithwaterman;
#[derive(Debug)]
pub enum Match {
    Char(Vec<CharMatch>),
    Byte(Vec<ByteMatch>),
}
impl Match {
    pub(crate) fn is_match(&self) -> bool {
        match &self {
            Match::Byte(v) => v.len() > 0,
            Match::Char(v) => v.len() > 0,
        }
    }
}

#[derive(Debug)]
pub(crate) struct CharMatch {
    pub distance: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug)]
pub(crate) struct ByteMatch {
    pub distance: usize,
    pub start: usize,
    pub end: usize,
}

pub(crate) fn fuzzy_search(pattern: &str, text: &str, is_exact: bool) -> Match {
    let pattern_bytes = pattern.as_bytes();
    let text_bytes = text.as_bytes();
    if is_exact {
        //使用 bm算法
        return Match::Byte(
            BoyerMoore::new(pattern)
                .find(text)
                .map(|e| ByteMatch {
                    distance: 0,
                    start: e,
                    end: e + pattern_bytes.len(),
                })
                .collect(),
        );
    } else {
        if pattern_bytes.len() * text_bytes.len() > MAXDIMS {
            // let bitap = Bitap::new(pattern);
            // let m = bitap.fuzzy_search(text, 1);
            // return Match::Byte(
            //     m.map(|e| ByteMatch {
            //         distance: e.distance,
            //         start: 0,
            //         end: e.end,
            //     })
            //     .collect(),
            // );
            return Match::Byte(Vec::new());
        } else {
            let mut sw = SmithWaterman::new();
            let m = sw.find(pattern, text);
            return Match::Byte(
                m.iter()
                    .map(|e| ByteMatch {
                        distance: e.score as usize,
                        start: e.start,
                        end: e.end,
                    })
                    .collect(),
            );
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_fuzzy_search() {
        let text = "nohup ./gost -L :1081 -F https://sto:vimfaith1987@www.vlist.top:443 > vpn.lo";
        let pattern = "echo";

        let m = fuzzy_search(pattern, text, false);
        let mut spans = Vec::new();
        let line = text.as_bytes();
        match m {
            Match::Char(_) => {
                todo!()
            }
            Match::Byte(v) => {
                let mut current_idx = 0;
                for bm in v.into_iter() {
                    if current_idx < bm.start && bm.start <= line.len() {
                        spans.push(std::str::from_utf8(&line[current_idx..bm.start]).unwrap());
                    }
                    // 添加高亮文本
                    if bm.start < line.len() && bm.end <= line.len() {
                        spans.push(std::str::from_utf8(&line[bm.start..bm.end]).unwrap());
                    }
                    // 更新当前索引为高亮区间的结束位置
                    current_idx = bm.end;
                }
                // 添加剩余的文本（如果有）
                if current_idx < line.len() {
                    spans.push(std::str::from_utf8(&line[current_idx..]).unwrap());
                }
            }
        }

        println!("{:?}", spans);
    }
}
