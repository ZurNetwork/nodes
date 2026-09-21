//! Searching one file's text for `nodes grep`: what counts as text, which lines hold the term,
//! and the snippet a hit shows.

use std::fmt;
use std::fs::File;
use std::io::{self, Read as _};
use std::path::Path;
use std::str::FromStr;

/// The largest file `grep` reads. Anything larger is passed over, and said so: a search that
/// silently skipped a file would read as "no hits".
pub const MAX_SEARCHED_BYTES: u64 = 8 * 1024 * 1024;
/// How much of a file is read before deciding it is binary — git's own measure.
const SNIFFED_BYTES: u64 = 8 * 1024;
/// The most characters of a line a hit shows.
const SNIPPET_CHARS: usize = 200;
/// What stands where a snippet was cut.
const ELLIPSIS: char = '…';

/// What `grep` looks for: a term that is not empty, held in lowercase — the search is
/// case-insensitive, as `find`'s is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Needle(String);

/// A search for nothing.
#[derive(Debug, PartialEq, Eq)]
pub struct EmptyTerm;

impl FromStr for Needle {
    type Err = EmptyTerm;

    fn from_str(term: &str) -> Result<Self, EmptyTerm> {
        if term.is_empty() {
            return Err(EmptyTerm);
        }
        Ok(Self(term.to_lowercase()))
    }
}

impl fmt::Display for EmptyTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "an empty term matches every line; give it something to look for"
        )
    }
}

impl std::error::Error for EmptyTerm {}

/// One line of a file that holds the term.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineHit {
    /// Counted from 1, as editors do.
    pub line: usize,
    /// The line, trimmed; a long one is cut to a window around the term.
    pub text: String,
}

/// What became of one file.
#[derive(Debug, PartialEq, Eq)]
pub enum Searched {
    /// A text file: the lines that hold the term, possibly none.
    Text(Vec<LineHit>),
    /// A file holding a NUL byte: not text, passed over without a word.
    Binary,
    /// A file larger than [`MAX_SEARCHED_BYTES`], of this many bytes.
    TooLarge(u64),
}

/// Searches one file. Its head is sniffed before anything else, so a large binary — a video, an
/// archive — is dismissed after a few kilobytes and never counted as too large to search.
pub fn search_file(file: &Path, needle: &Needle) -> io::Result<Searched> {
    let mut opened = File::open(file)?;
    let mut bytes = Vec::new();
    opened
        .by_ref()
        .take(SNIFFED_BYTES)
        .read_to_end(&mut bytes)?;
    if bytes.contains(&0) {
        return Ok(Searched::Binary);
    }
    let size = opened.metadata()?.len();
    if size > MAX_SEARCHED_BYTES {
        return Ok(Searched::TooLarge(size));
    }
    opened.read_to_end(&mut bytes)?;
    if bytes.contains(&0) {
        return Ok(Searched::Binary);
    }
    let text = String::from_utf8_lossy(&bytes);
    Ok(Searched::Text(hits_in(&text, needle)))
}

/// The lines of `text` that hold the needle.
pub fn hits_in(text: &str, needle: &Needle) -> Vec<LineHit> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.to_lowercase().contains(&needle.0))
        .map(|(index, line)| LineHit {
            line: index + 1,
            text: snippet(line, needle),
        })
        .collect()
}

/// The line as a hit shows it: trimmed, and — when longer than [`SNIPPET_CHARS`] — cut to a
/// window centred on the term, an ellipsis wherever something was left out.
fn snippet(line: &str, needle: &Needle) -> String {
    let trimmed = line.trim();
    let characters: Vec<char> = trimmed.chars().collect();
    if characters.len() <= SNIPPET_CHARS {
        return trimmed.to_owned();
    }
    let needle_length = needle.0.chars().count();
    let term_start = term_start(&characters, needle).unwrap_or(0);
    let lead = SNIPPET_CHARS.saturating_sub(needle_length) / 2;
    let latest_start = characters.len() - SNIPPET_CHARS;
    let start = term_start.saturating_sub(lead).min(latest_start);
    let end = start + SNIPPET_CHARS;
    let mut window = String::new();
    if start > 0 {
        window.push(ELLIPSIS);
    }
    window.extend(&characters[start..end]);
    if end < characters.len() {
        window.push(ELLIPSIS);
    }
    window
}

/// Where the term starts in the line, counted in the line's own characters. Found character by
/// character — never by an offset into the lowercased copy, whose characters need not line up
/// with the original's (`İ` lowercases to two).
fn term_start(characters: &[char], needle: &Needle) -> Option<usize> {
    let needle_length = needle.0.chars().count();
    (0..characters.len()).find(|start| {
        characters[*start..]
            .iter()
            .flat_map(|character| character.to_lowercase())
            .take(needle_length)
            .eq(needle.0.chars())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn needle(term: &str) -> Needle {
        term.parse().expect("a term")
    }

    #[test]
    fn a_needle_is_lowercase_and_never_empty() {
        assert_eq!("GraceFul".parse(), Ok(Needle("graceful".to_owned())));
        assert_eq!("".parse::<Needle>(), Err(EmptyTerm));
    }

    #[test]
    fn hits_are_the_lines_holding_the_term_whatever_its_case() {
        let text = "fn main() {\n    // TODO: Graceful Shutdown\n}\n// graceful, again\n";
        let first_hit = LineHit {
            line: 2,
            text: "// TODO: Graceful Shutdown".to_owned(),
        };
        let second_hit = LineHit {
            line: 4,
            text: "// graceful, again".to_owned(),
        };
        assert_eq!(hits_in(text, &needle("GRACEFUL")), [first_hit, second_hit]);
        assert_eq!(hits_in(text, &needle("absent")), []);
    }

    #[test]
    fn a_long_line_is_cut_to_a_window_around_the_term() {
        let padding = "x".repeat(5_000);
        let line = format!("{padding} needle {padding}");
        let hits = hits_in(&line, &needle("needle"));
        let shown = &hits[0].text;
        assert_eq!(
            shown.chars().count(),
            SNIPPET_CHARS + 2,
            "the window and two ellipses"
        );
        assert!(shown.starts_with(ELLIPSIS) && shown.ends_with(ELLIPSIS));
        assert!(
            shown.contains(" needle "),
            "the term sits inside the window"
        );
        let at_the_start = format!("needle {padding}");
        let shown_from_the_start = &hits_in(&at_the_start, &needle("needle"))[0].text;
        assert!(shown_from_the_start.starts_with("needle x"));
        assert!(shown_from_the_start.ends_with(ELLIPSIS));
    }

    #[test]
    fn a_character_that_lowercases_to_two_does_not_shift_the_window() {
        let dotted_capitals = "İ".repeat(300);
        let line = format!("{dotted_capitals} needle");
        let shown = &hits_in(&line, &needle("needle"))[0].text;
        assert!(shown.ends_with(" needle"), "{shown}");
        assert_eq!(shown.chars().count(), SNIPPET_CHARS + 1);
    }
}
