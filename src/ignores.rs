//! Which directories the walk stays out of: gitignore rules read from `.gitignore` and
//! `.chartignore` at every level, plus hidden directories and a few names that are never charted.

use std::fs;
use std::path::{Path, PathBuf};

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::error::Error;

/// Directories never descended into, whatever the ignore files say.
const ALWAYS_SKIPPED: [&str; 4] = [".git", ".jj", "node_modules", "target"];
/// The ignore files read in each directory, gitignore syntax; a later file overrides an earlier one.
const IGNORE_FILES: [&str; 2] = [".gitignore", ".chartignore"];

/// The ignore rules in force at one point of the walk: one layer per directory entered, from the
/// tree's root down. Nothing above the root is read, so a tree reads the same from wherever it is
/// reached.
#[derive(Debug, Default)]
pub struct IgnoreRules {
    layers: Vec<Option<Gitignore>>,
}

impl IgnoreRules {
    /// Adds the rules `dir` itself declares; they govern everything beneath it until [`Self::leave`].
    pub fn enter(&mut self, dir: &Path) -> Result<(), Error> {
        let layer = layer_of(dir)?;
        self.layers.push(layer);
        Ok(())
    }

    /// Drops the rules of the directory entered last.
    pub fn leave(&mut self) {
        self.layers.pop();
    }

    /// Whether the walk stays out of `dir`. The deepest layer with an opinion decides — git's own
    /// precedence; with no opinion anywhere, a hidden directory is skipped, so a `!.name/` line is
    /// what brings one back.
    pub fn skips(&self, dir: &Path) -> bool {
        let name = dir.file_name().map(|name| name.to_string_lossy());
        let Some(name) = name else {
            return false;
        };
        if ALWAYS_SKIPPED.contains(&name.as_ref()) {
            return true;
        }
        let verdict = self
            .layers
            .iter()
            .rev()
            .flatten()
            .map(|layer| layer.matched(dir, true))
            .find(|verdict| !verdict.is_none());
        match verdict {
            Some(Match::Ignore(_)) => true,
            Some(Match::Whitelist(_)) => false,
            Some(Match::None) | None => name.starts_with('.'),
        }
    }
}

/// The rules one directory declares, or `None` when it holds no ignore file. An ignore file that
/// cannot be read stops the walk — rules that silently go missing would chart what was meant to be
/// skipped — while a line git would not understand is passed over, as git does.
fn layer_of(dir: &Path) -> Result<Option<Gitignore>, Error> {
    let declared: Vec<PathBuf> = IGNORE_FILES
        .iter()
        .map(|name| dir.join(name))
        .filter(|file| file.is_file())
        .collect();
    if declared.is_empty() {
        return Ok(None);
    }
    let mut builder = GitignoreBuilder::new(dir);
    for file in &declared {
        let bytes = fs::read(file).map_err(|source| Error::io(file, source))?;
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            let _not_a_pattern = builder.add_line(Some(file.clone()), line);
        }
    }
    let layer = builder.build().map_err(|source| Error::IgnoreRules {
        dir: dir.to_path_buf(),
        source,
    })?;
    Ok(Some(layer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_and_always_skipped_directories_need_no_ignore_file() {
        let rules = IgnoreRules::default();
        assert!(rules.skips(Path::new("/repo/.cache")));
        assert!(rules.skips(Path::new("/repo/backend/target")));
        assert!(rules.skips(Path::new("/repo/node_modules")));
        assert!(!rules.skips(Path::new("/repo/backend")));
    }
}
