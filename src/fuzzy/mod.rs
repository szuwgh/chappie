use crate::fuzzy::{fzf::FzfMatcher, fzf_sw::FzfV2Matcher};
mod bitap;
pub(crate) mod fzf;
pub(crate) mod fzf_sw;

mod levenshtein;
pub(crate) mod smithwaterman;

#[derive(Debug, Clone)]
pub(crate) struct Match {
    pub score: i16,
    pub start: usize,
    pub end: usize,
    pub positions: Vec<usize>,
}

pub(crate) struct FuzzySearch {
    fzf_v1: FzfMatcher,
    fzf_v2: FzfV2Matcher,
}

impl FuzzySearch {
    pub(crate) fn new() -> Self {
        Self {
            fzf_v1: FzfMatcher::new(),
            fzf_v2: FzfV2Matcher::new(),
        }
    }

    pub(crate) fn find_parts(&mut self, pattern: &[u8], parts: &[&[u8]]) -> Vec<Match> {
        self.find_parts_v1(pattern, parts)
    }

    pub(crate) fn find_parts_v1(&mut self, pattern: &[u8], parts: &[&[u8]]) -> Vec<Match> {
        self.fzf_v1.find_parts_smart_case(pattern, parts)
    }

    pub(crate) fn find_parts_v2(&mut self, pattern: &[u8], parts: &[&[u8]]) -> Vec<Match> {
        self.fzf_v2.find_parts_smart_case(pattern, parts)
    }
}

#[inline]
pub(crate) fn smart_case(pattern: &[u8]) -> bool {
    pattern.iter().any(|byte| byte.is_ascii_uppercase())
}

impl Default for FuzzySearch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_search_uses_v1_filter_by_default() {
        let mut search = FuzzySearch::new();
        let matches = search.find_parts(b"fb", &[b"foobar fb"]);

        assert_eq!(matches.len(), 1);
        assert_eq!((matches[0].start, matches[0].end), (0, 4));
    }

    #[test]
    fn fuzzy_search_exposes_fast_v1_filter() {
        let mut search = FuzzySearch::new();
        let matches = search.find_parts_v1(b"fb", &[b"/home/unvdb/foo/bar.rs"]);

        assert_eq!(matches.len(), 1);
        assert_eq!(
            &b"/home/unvdb/foo/bar.rs"[matches[0].start..matches[0].end],
            b"foo/b"
        );
    }

    #[test]
    fn fuzzy_search_supports_cross_slice_input() {
        let mut search = FuzzySearch::new();
        let parts: &[&[u8]] = &[b"/.oh-", b"my-zsh", b"/cache"];
        let matches = search.find_parts(b"zshc", parts);

        assert_eq!(matches.len(), 1);
        assert_eq!((matches[0].start, matches[0].end), (8, 13));
    }

    #[test]
    fn fuzzy_search_exposes_precise_v2_scoring() {
        let mut search = FuzzySearch::new();
        let matches = search.find_parts_v2(b"zshc", &[b"/.oh-my-zsh/cache"]);

        assert_eq!(matches.len(), 1);
        assert_eq!(
            (matches[0].start, matches[0].end, matches[0].score),
            (8, 13, 102)
        );
        assert_eq!(matches[0].positions, [8, 9, 10, 12]);
    }

    #[test]
    fn fuzzy_search_v2_uses_smart_case() {
        let mut search = FuzzySearch::new();

        assert_eq!(
            search.find_parts_v2(b"fb", &[b"FooBar"])[0].positions,
            [0, 3]
        );
        assert!(search.find_parts_v2(b"FB", &[b"foobar"]).is_empty());
    }
}
