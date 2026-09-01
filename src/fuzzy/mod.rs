use crate::fuzzy::{fzf::FzfMatcher, fzf_sw::FzfV2Matcher};
mod bitap;
mod fzf;
mod fzf_sw;

mod levenshtein;
pub(crate) mod smithwaterman;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MatchBounds {
    pub score: i16,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledPattern {
    pub(crate) bytes: Vec<u8>,
    pub(crate) case_sensitive: bool,
}

impl CompiledPattern {
    pub(crate) fn new(pattern: &[u8]) -> Self {
        let case_sensitive = smart_case(pattern);
        let bytes = if case_sensitive {
            pattern.to_vec()
        } else {
            pattern
                .iter()
                .map(|&byte| byte.to_ascii_lowercase())
                .collect()
        };
        Self {
            bytes,
            case_sensitive,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Match {
    pub score: i16,
    pub start: usize,
    pub end: usize,
    pub positions: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FuzzyAlgorithm {
    V1,
    V2,
}

pub(crate) struct FuzzySearch {
    fzf_v1: FzfMatcher,
    fzf_v2: FzfV2Matcher,
}

pub(crate) struct FuzzySession<'a> {
    search: &'a mut FuzzySearch,
    algorithm: FuzzyAlgorithm,
    case_sensitive: bool,
}

impl FuzzySearch {
    pub(crate) fn new() -> Self {
        Self {
            fzf_v1: FzfMatcher::new(),
            fzf_v2: FzfV2Matcher::new(),
        }
    }

    pub(crate) fn begin(
        &mut self,
        algorithm: FuzzyAlgorithm,
        pattern: &CompiledPattern,
    ) -> FuzzySession<'_> {
        match algorithm {
            FuzzyAlgorithm::V1 => self.fzf_v1.prepare(pattern),
            FuzzyAlgorithm::V2 => self.fzf_v2.prepare(pattern),
        }

        FuzzySession {
            search: self,
            algorithm,
            case_sensitive: pattern.case_sensitive,
        }
    }
}

impl FuzzySession<'_> {
    pub(crate) fn find_bounds(&mut self, parts: &[&[u8]]) -> Option<MatchBounds> {
        match self.algorithm {
            FuzzyAlgorithm::V1 => self
                .search
                .fzf_v1
                .find_best_bounds_prepared(parts, self.case_sensitive),
            FuzzyAlgorithm::V2 => self
                .search
                .fzf_v2
                .find_best_bounds_prepared(parts, self.case_sensitive),
        }
    }

    pub(crate) fn find_with_positions(&mut self, parts: &[&[u8]]) -> Option<Match> {
        match self.algorithm {
            FuzzyAlgorithm::V1 => self
                .search
                .fzf_v1
                .find_best_with_positions_prepared(parts, self.case_sensitive),
            FuzzyAlgorithm::V2 => self
                .search
                .fzf_v2
                .find_best_with_positions_prepared(parts, self.case_sensitive),
        }
    }
}

#[inline]
fn smart_case(pattern: &[u8]) -> bool {
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
        let pattern = CompiledPattern::new(b"fb");
        let mut search = FuzzySearch::new();
        let matches = search
            .begin(FuzzyAlgorithm::V1, &pattern)
            .find_with_positions(&[b"foobar fb"])
            .into_iter()
            .collect::<Vec<_>>();

        assert_eq!(matches.len(), 1);
        assert_eq!((matches[0].start, matches[0].end), (0, 4));
    }

    #[test]
    fn fuzzy_search_exposes_fast_v1_filter() {
        let pattern = CompiledPattern::new(b"fb");
        let mut search = FuzzySearch::new();
        let matches = search
            .begin(FuzzyAlgorithm::V1, &pattern)
            .find_with_positions(&[b"/home/unvdb/foo/bar.rs"])
            .into_iter()
            .collect::<Vec<_>>();

        assert_eq!(matches.len(), 1);
        assert_eq!(
            &b"/home/unvdb/foo/bar.rs"[matches[0].start..matches[0].end],
            b"foo/b"
        );
    }

    #[test]
    fn fuzzy_search_supports_cross_slice_input() {
        let pattern = CompiledPattern::new(b"zshc");
        let mut search = FuzzySearch::new();
        let parts: &[&[u8]] = &[b"/.oh-", b"my-zsh", b"/cache"];
        let matches = search
            .begin(FuzzyAlgorithm::V1, &pattern)
            .find_with_positions(parts)
            .into_iter()
            .collect::<Vec<_>>();

        assert_eq!(matches.len(), 1);
        assert_eq!((matches[0].start, matches[0].end), (8, 13));
    }

    #[test]
    fn fuzzy_search_exposes_precise_v2_scoring() {
        let pattern = CompiledPattern::new(b"zshc");
        let mut search = FuzzySearch::new();
        let matches = search
            .begin(FuzzyAlgorithm::V2, &pattern)
            .find_with_positions(&[b"/.oh-my-zsh/cache"])
            .into_iter()
            .collect::<Vec<_>>();

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
        let insensitive = CompiledPattern::new(b"fb");
        let sensitive = CompiledPattern::new(b"FB");

        assert_eq!(
            search
                .begin(FuzzyAlgorithm::V2, &insensitive)
                .find_with_positions(&[b"FooBar"])
                .unwrap()
                .positions,
            [0, 3]
        );
        assert!(search
            .begin(FuzzyAlgorithm::V2, &sensitive)
            .find_with_positions(&[b"foobar"])
            .is_none());
    }

    #[test]
    fn prepared_session_selects_algorithm_and_output_shape() {
        let pattern = CompiledPattern::new(b"fb");
        let mut search = FuzzySearch::new();

        let mut v1 = search.begin(FuzzyAlgorithm::V1, &pattern);
        let bounds = v1.find_bounds(&[b"FooBar"]).unwrap();
        assert_eq!((bounds.start, bounds.end), (0, 4));

        let mut v2 = search.begin(FuzzyAlgorithm::V2, &pattern);
        let matched = v2.find_with_positions(&[b"FooBar"]).unwrap();
        assert_eq!((matched.start, matched.end), (0, 4));
        assert_eq!(matched.positions, [0, 3]);
    }

    #[test]
    fn compiled_pattern_matches_uncompiled_search() {
        let mut search = FuzzySearch::new();
        let compiled = CompiledPattern::new(b"zshc");
        let first = search
            .begin(FuzzyAlgorithm::V2, &compiled)
            .find_bounds(&[b"/.oh-my-zsh/cache"]);
        let second = search
            .begin(FuzzyAlgorithm::V2, &compiled)
            .find_bounds(&[b"/.oh-my-zsh/cache"]);

        assert_eq!(first, second);
    }
}
