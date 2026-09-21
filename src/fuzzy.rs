//! How well a partial or slightly-off name fits a path — what `nodes resolve` ranks by. Hand-rolled
//! on purpose: a table of tiers anyone can read beats a matcher crate's opaque numbers.
//!
//! Everything is compared in lowercase and measured in characters. The tiers, best first — the
//! first one that fits decides, and the fraction of the matched text the query covers breaks ties
//! inside a tier:
//!
//! | Fit | Score |
//! |---|---|
//! | the whole path | 1000 |
//! | the path's last components, as many as the query has (one: the name) | 900 |
//! | the name without its extension | 850 |
//! | the start of the name | 700–749 |
//! | the start of a word inside the name | 650–699 |
//! | anywhere inside the name | 600–649 |
//! | anywhere inside the path | 500–549 |
//! | a typo away from the last components, or from the name without its extension | 440, 400 |
//! | the name's characters, in order, with gaps | 300–349 |
//! | the path's characters, in order, with gaps | 200–249 |
//!
//! A typo outranks a scattered match on purpose: `reop` means `repo.rs`, not
//! `render_operations.rs`.

use serde::Serialize;

/// How well a query fits a path; higher is better.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Score(u16);

impl Score {
    const WHOLE_PATH: u16 = 1000;
    const LAST_COMPONENTS: u16 = 900;
    const NAME_WITHOUT_EXTENSION: u16 = 850;
    const NAME_START: u16 = 700;
    const WORD_START: u16 = 650;
    const INSIDE_NAME: u16 = 600;
    const INSIDE_PATH: u16 = 500;
    const ONE_TYPO: u16 = 440;
    const TWO_TYPOS: u16 = 400;
    const SCATTERED_IN_NAME: u16 = 300;
    const SCATTERED_IN_PATH: u16 = 200;
    /// The most a tier adds for how much of the matched text the query covers.
    const COVERAGE: usize = 49;

    pub const fn get(self) -> u16 {
        self.0
    }

    /// A tier's base plus the share of `matched` characters the query's `covered` ones make up.
    fn covering(base: u16, covered: usize, matched: usize) -> Self {
        let share = (Self::COVERAGE * covered)
            .checked_div(matched)
            .unwrap_or(Self::COVERAGE);
        let bonus = u16::try_from(share.min(Self::COVERAGE)).expect("at most the coverage bonus");
        Self(base + bonus)
    }
}

/// The characters a name is made of words by.
const BOUNDARIES: [char; 5] = ['/', '_', '-', '.', ' '];

/// How well `query` fits `path`, or `None` when it does not fit at all.
pub fn score(query: &str, path: &str) -> Option<Score> {
    let query = query.trim_matches('/').to_lowercase();
    let path = path.to_lowercase();
    if query.is_empty() {
        return None;
    }
    let query_length = query.chars().count();
    let component_count = query.split('/').count();
    let name = path.rsplit('/').next().unwrap_or(path.as_str());
    let name_without_extension = match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    };
    let last_components = last_components(&path, component_count);
    if query == path {
        return Some(Score(Score::WHOLE_PATH));
    }
    if query == last_components {
        return Some(Score(Score::LAST_COMPONENTS));
    }
    if query == name_without_extension {
        return Some(Score(Score::NAME_WITHOUT_EXTENSION));
    }
    let name_length = name.chars().count();
    if name.starts_with(&query) {
        let name_start = Score::covering(Score::NAME_START, query_length, name_length);
        return Some(name_start);
    }
    if starts_a_word_in(&query, name) {
        let word_start = Score::covering(Score::WORD_START, query_length, name_length);
        return Some(word_start);
    }
    if name.contains(&query) {
        let inside_name = Score::covering(Score::INSIDE_NAME, query_length, name_length);
        return Some(inside_name);
    }
    if path.contains(&query) {
        let path_length = path.chars().count();
        let inside_path = Score::covering(Score::INSIDE_PATH, query_length, path_length);
        return Some(inside_path);
    }
    let typos = [last_components, name_without_extension]
        .into_iter()
        .filter_map(|intended| typos_between(&query, intended))
        .min();
    match typos {
        Some(1) => return Some(Score(Score::ONE_TYPO)),
        Some(2) => return Some(Score(Score::TWO_TYPOS)),
        _ => {}
    }
    if let Some(span) = scattered_span(&query, name) {
        let scattered = Score::covering(Score::SCATTERED_IN_NAME, query_length, span);
        return Some(scattered);
    }
    let span = scattered_span(&query, &path)?;
    let scattered = Score::covering(Score::SCATTERED_IN_PATH, query_length, span);
    Some(scattered)
}

/// The paths `query` fits, best first: by score, then the shorter path, then alphabetically.
pub fn rank<C: AsRef<str>>(query: &str, candidates: Vec<C>) -> Vec<(C, Score)> {
    let mut fitting: Vec<(C, Score)> = candidates
        .into_iter()
        .filter_map(|candidate| {
            let fit = score(query, candidate.as_ref())?;
            Some((candidate, fit))
        })
        .collect();
    fitting.sort_by(|(left, left_score), (right, right_score)| {
        let left_length = left.as_ref().chars().count();
        let right_length = right.as_ref().chars().count();
        right_score
            .cmp(left_score)
            .then(left_length.cmp(&right_length))
            .then_with(|| left.as_ref().cmp(right.as_ref()))
    });
    fitting
}

/// The last `count` components of `path`, or all of it when it has no more than that.
fn last_components(path: &str, count: usize) -> &str {
    let start = path
        .rmatch_indices('/')
        .nth(count.saturating_sub(1))
        .map_or(0, |(slash, _)| slash + 1);
    &path[start..]
}

/// Whether `query` sits in `name` right after a boundary: the start of a word, not of the name.
fn starts_a_word_in(query: &str, name: &str) -> bool {
    name.match_indices(query).any(|(at, _)| {
        name[..at]
            .chars()
            .next_back()
            .is_some_and(|before| BOUNDARIES.contains(&before))
    })
}

/// How many typos — a character dropped, added, swapped for another, or two neighbours
/// transposed — lie between `query` and `intended`, when few enough to be a slip of the hand
/// rather than another word: none allowed under four characters, one up to seven, two beyond.
fn typos_between(query: &str, intended: &str) -> Option<usize> {
    let typed: Vec<char> = query.chars().collect();
    let meant: Vec<char> = intended.chars().collect();
    let allowed = match typed.len() {
        0..=3 => return None,
        4..=7 => 1,
        _ => 2,
    };
    if typed.len().abs_diff(meant.len()) > allowed {
        return None;
    }
    let distance = optimal_string_alignment(&typed, &meant);
    (1..=allowed).contains(&distance).then_some(distance)
}

/// The optimal-string-alignment distance: Levenshtein, plus a transposition of two neighbours
/// counted as one edit.
fn optimal_string_alignment(typed: &[char], meant: &[char]) -> usize {
    let width = meant.len() + 1;
    let mut before_previous: Vec<usize> = vec![0; width];
    let mut previous: Vec<usize> = (0..width).collect();
    for (row, typed_char) in typed.iter().enumerate() {
        let mut current: Vec<usize> = vec![row + 1; width];
        for (column, meant_char) in meant.iter().enumerate() {
            let substitution = previous[column] + usize::from(typed_char != meant_char);
            let deletion = previous[column + 1] + 1;
            let insertion = current[column] + 1;
            let mut best = substitution.min(deletion).min(insertion);
            let transposed = row > 0
                && column > 0
                && *typed_char == meant[column - 1]
                && typed[row - 1] == *meant_char;
            if transposed {
                best = best.min(before_previous[column - 1] + 1);
            }
            current[column + 1] = best;
        }
        before_previous = previous;
        previous = current;
    }
    previous[meant.len()]
}

/// The length of the tightest stretch of `text` that holds `query`'s characters in order: a
/// greedy pass forward finds where a match can end, a pass back from there finds its latest start.
fn scattered_span(query: &str, text: &str) -> Option<usize> {
    let wanted: Vec<char> = query.chars().collect();
    let held: Vec<char> = text.chars().collect();
    let mut matched = 0;
    let mut end = None;
    for (index, character) in held.iter().enumerate() {
        if *character == wanted[matched] {
            matched += 1;
        }
        if matched == wanted.len() {
            end = Some(index);
            break;
        }
    }
    let end = end?;
    let mut remaining = wanted.len();
    let mut start = end;
    for index in (0..=end).rev() {
        if held[index] == wanted[remaining - 1] {
            remaining -= 1;
            start = index;
        }
        if remaining == 0 {
            break;
        }
    }
    Some(end - start + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scored(query: &str, path: &str) -> Option<u16> {
        score(query, path).map(Score::get)
    }

    fn ranked<'a>(query: &str, paths: &[&'a str]) -> Vec<&'a str> {
        rank(query, paths.to_vec())
            .into_iter()
            .map(|(path, _)| path)
            .collect()
    }

    #[test]
    fn an_exact_path_beats_a_path_that_merely_holds_it() {
        assert_eq!(scored("backend", "backend"), Some(1000));
        assert_eq!(scored("backend", "backend/crates"), Some(524));
        assert_eq!(
            scored("Backend/", "backend"),
            Some(1000),
            "case and a trailing slash are nothing"
        );
    }

    #[test]
    fn the_last_components_fit_as_well_as_a_name_does() {
        assert_eq!(scored("crates", "backend/crates"), Some(900));
        assert_eq!(scored("crates", "crates-old"), Some(729));
        assert_eq!(scored("crates/api", "backend/crates/api"), Some(900));
        let deeper = scored("crates/api", "backend/crates/api/src");
        assert_eq!(deeper, Some(522), "inside the path, not its end");
    }

    #[test]
    fn a_name_fits_whole_then_from_its_start_then_from_a_word_then_anywhere() {
        let paths = [
            "docs/reindexer.rs",
            "docs/design-index.md",
            "docs/index.md",
            "docs/indexes.md",
        ];
        let expected_order = [
            "docs/index.md",
            "docs/indexes.md",
            "docs/design-index.md",
            "docs/reindexer.rs",
        ];
        assert_eq!(ranked("index", &paths), expected_order);
        assert_eq!(scored("index", "docs/index.md"), Some(850));
        assert_eq!(scored("index", "docs/design-index.md"), Some(666));
        assert_eq!(scored("index", "docs/reindexer.rs"), Some(620));
    }

    #[test]
    fn a_typo_finds_the_name_it_meant_and_nothing_else() {
        let paths = ["frontend", "backend", "backend/crates", "docs/index.md"];
        assert_eq!(ranked("frotnend", &paths), ["frontend"]);
        assert_eq!(
            scored("frotnend", "frontend"),
            Some(440),
            "two neighbours transposed"
        );
        assert_eq!(
            scored("fronntendd", "frontend"),
            Some(400),
            "two slips in a long name"
        );
        assert_eq!(
            scored("shcema", "src/schema.rs"),
            Some(440),
            "the extension is not a typo"
        );
    }

    #[test]
    fn a_typo_outranks_characters_scattered_through_a_longer_name() {
        let paths = ["src/render_operations.rs", "src/repo.rs"];
        assert_eq!(
            ranked("reop", &paths),
            ["src/repo.rs", "src/render_operations.rs"]
        );
        assert_eq!(scored("reop", "src/render_operations.rs"), Some(321));
        assert_eq!(
            ranked("bakend", &["backend/crates", "backend"]),
            ["backend", "backend/crates"]
        );
        assert_eq!(scored("bakend", "backend/crates"), Some(242));
    }

    #[test]
    fn short_words_are_never_typos_and_strangers_do_not_fit() {
        assert_eq!(
            scored("api", "app"),
            None,
            "three characters: too short for a slip"
        );
        assert_eq!(scored("zzz", "backend/crates"), None);
        assert_eq!(scored("", "backend"), None);
        assert_eq!(
            scored("30 HOUS", "Life/30 Housing"),
            Some(734),
            "spaces and case are fine"
        );
    }

    #[test]
    fn equal_scores_list_the_shorter_path_first_then_alphabetically() {
        let paths = ["zeta/lib.rs", "alpha/deeper/lib.rs", "beta/lib.rs"];
        let expected_order = ["beta/lib.rs", "zeta/lib.rs", "alpha/deeper/lib.rs"];
        assert_eq!(ranked("lib.rs", &paths), expected_order);
    }
}
