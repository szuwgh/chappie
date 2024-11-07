mod bitap;
mod boyermoore;
mod levenshtein;
mod smithwaterman;

pub enum Match {
    Char(Vec<CharMatch>),
    Byte(Vec<ByteMatch>),
}

#[derive(Debug)]
struct CharMatch {
    pub distance: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug)]
struct ByteMatch {
    pub distance: usize,
    pub start: usize,
    pub end: usize,
}

fn fuzzy_search(pattern: &str, text: &str, is_exact: bool) -> Match {
    if is_exact { //使用 bm算法
    } else {
    }
    todo!()
}
