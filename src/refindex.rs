//! The reference index `check` resolves `refs[].page` against, when one is given.
//!
//! Any text file works: a line's subject is its first run of digits, taken as a page id, and the
//! line is that page's entry. An entry containing the superseded marker (`SUPERSEDED` unless told
//! otherwise) flags a page that should no longer be cited as-is.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::error::Error;
use crate::schema::PageId;

/// The default word that marks an index entry as superseded.
pub const DEFAULT_SUPERSEDED_MARKER: &str = "SUPERSEDED";

/// The page ids an index lists, each with its entry line.
#[derive(Debug, Default)]
pub struct RefIndex {
    entries: BTreeMap<PageId, IndexEntry>,
}

/// One index line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub line: String,
    pub superseded: bool,
}

impl RefIndex {
    /// Reads the index at `file`.
    pub fn load(file: &Path, superseded_marker: &str) -> Result<Self, Error> {
        let text = fs::read_to_string(file).map_err(|source| Error::io(file, source))?;
        Ok(Self::parse(&text, superseded_marker))
    }

    /// Reads the entries out of the text.
    pub fn parse(text: &str, superseded_marker: &str) -> Self {
        let mut entries: BTreeMap<PageId, IndexEntry> = BTreeMap::new();
        for line in text.lines() {
            let Some(page) = subject_of(line) else {
                continue;
            };
            let superseded = line.contains(superseded_marker);
            let entry = entries.entry(page).or_insert_with(|| IndexEntry {
                line: line.trim().to_owned(),
                superseded: false,
            });
            entry.superseded |= superseded;
        }
        Self { entries }
    }

    pub fn get(&self, page: PageId) -> Option<&IndexEntry> {
        self.entries.get(&page)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The first run of digits on the line, as a page id.
fn subject_of(line: &str) -> Option<PageId> {
    let start = line.find(|c: char| c.is_ascii_digit())?;
    let digits: String = line[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_subjects_and_superseded_marks() {
        let text = "\
# Index built 2026-06-30\n\
- `37519361` — Code Style — Semantic Rulings (Rust) (LIVING page)\n\
- `3670017` — MVP & Roadmap — SUPERSEDED, merged → Project MVP (`589826`)\n\
- `589826` — Project MVP\n\
  - `28409880` — Commission Tree Storage (SUPERSEDED IN PART 2026-08-04 → 45514754)\n\
no digits here\n";
        let index = RefIndex::parse(text, DEFAULT_SUPERSEDED_MARKER);
        assert_eq!(index.len(), 5);
        let living = index.get(PageId::from(37_519_361)).expect("listed");
        assert!(!living.superseded);
        let merged = index.get(PageId::from(3_670_017)).expect("listed");
        assert!(merged.superseded);
        let mentioned_on_a_superseded_line = index.get(PageId::from(589_826)).expect("listed");
        assert!(!mentioned_on_a_superseded_line.superseded);
        let in_part = index.get(PageId::from(28_409_880)).expect("listed");
        assert!(in_part.superseded);
        assert!(index.get(PageId::from(45_514_754)).is_none());
        assert!(
            index.get(PageId::from(2026)).is_some(),
            "a prose line's first digits are its subject"
        );
    }

    #[test]
    fn the_marker_is_configurable() {
        let index = RefIndex::parse("- 100 — old (RETIRED)\n", "RETIRED");
        assert!(index.get(PageId::from(100)).expect("listed").superseded);
    }
}
