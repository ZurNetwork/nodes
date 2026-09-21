//! The free-form labels a node carries: its [`Tags`], each a [`Tag`] — `word` or `namespace:word`.
//! Nothing here is a closed vocabulary; the one namespace the tool reads is `category`, which says
//! what a directory specifically holds.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The namespace of the tags that say what a directory specifically holds.
pub const CATEGORY_NAMESPACE: &str = "category";

/// One label: `word` or `namespace:word`, each part in lowercase letters, digits, `_` and `-`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Tag(String);

/// A string that is not a tag.
#[derive(Debug, PartialEq, Eq)]
pub struct MalformedTag(String);

impl Tag {
    /// The tag `category:<word>`. A named constructor because the namespace is supplied rather
    /// than parsed: `--category ui` and `classify … ui` both mean `category:ui`.
    pub fn category(word: &str) -> Result<Self, MalformedTag> {
        let spelled = format!("{CATEGORY_NAMESPACE}:{word}");
        Self::try_from(spelled)
    }

    /// What comes before the `:`, when the tag has one.
    pub fn namespace(&self) -> Option<&str> {
        self.0.split_once(':').map(|(namespace, _)| namespace)
    }

    /// What comes after the `:` — the whole tag when it has no namespace.
    pub fn word(&self) -> &str {
        self.0
            .split_once(':')
            .map_or(self.0.as_str(), |(_, word)| word)
    }

    /// Whether the tag says what the directory specifically holds.
    pub fn is_category(&self) -> bool {
        self.namespace() == Some(CATEGORY_NAMESPACE)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether `part` — a namespace or a word — is spelled in the characters a tag allows.
fn is_well_formed_part(part: &str) -> bool {
    let is_allowed = |character: char| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '_' | '-')
    };
    !part.is_empty() && part.chars().all(is_allowed)
}

impl TryFrom<String> for Tag {
    type Error = MalformedTag;

    fn try_from(text: String) -> Result<Self, MalformedTag> {
        let (namespace, word) = match text.split_once(':') {
            Some((namespace, word)) => (Some(namespace), word),
            None => (None, text.as_str()),
        };
        let namespace_fits = namespace.is_none_or(is_well_formed_part);
        if namespace_fits && is_well_formed_part(word) {
            Ok(Self(text))
        } else {
            Err(MalformedTag(text))
        }
    }
}

impl TryFrom<&str> for Tag {
    type Error = MalformedTag;

    fn try_from(text: &str) -> Result<Self, MalformedTag> {
        Self::try_from(text.to_owned())
    }
}

impl FromStr for Tag {
    type Err = MalformedTag;

    fn from_str(text: &str) -> Result<Self, MalformedTag> {
        Self::try_from(text)
    }
}

impl From<Tag> for String {
    fn from(tag: Tag) -> Self {
        tag.0
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for MalformedTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a tag: a tag is `word` or `namespace:word`, in lowercase letters, digits, `_` and `-`",
            self.0
        )
    }
}

impl std::error::Error for MalformedTag {}

/// A node's tags, in the author's order — `fmt` never sorts them: no tag twice, and at least one
/// `category:` tag. The categories are ranked from most to least fitting by where they stand
/// among each other.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Tag>", into = "Vec<Tag>")]
pub struct Tags(Vec<Tag>);

/// Why a list of tags is not a node's tags.
#[derive(Debug, PartialEq, Eq)]
pub enum TagsError {
    NoCategory,
    Repeated(Tag),
}

impl Tags {
    /// The tags, as authored.
    pub fn authored(&self) -> &[Tag] {
        &self.0
    }

    /// Whether the node carries `tag`.
    pub fn carries(&self, tag: &Tag) -> bool {
        self.0.contains(tag)
    }

    /// Where `tag` stands among the node's tags of the same namespace: `0` for the first — a
    /// category's best fit — and `None` when the node does not carry it.
    pub fn rank_of(&self, tag: &Tag) -> Option<usize> {
        self.0
            .iter()
            .filter(|carried| carried.namespace() == tag.namespace())
            .position(|carried| carried == tag)
    }
}

impl TryFrom<Vec<Tag>> for Tags {
    type Error = TagsError;

    fn try_from(authored: Vec<Tag>) -> Result<Self, TagsError> {
        let repeated = authored
            .iter()
            .enumerate()
            .find(|(place, tag)| authored[..*place].contains(tag))
            .map(|(_, tag)| tag.clone());
        if let Some(tag) = repeated {
            return Err(TagsError::Repeated(tag));
        }
        if !authored.iter().any(Tag::is_category) {
            return Err(TagsError::NoCategory);
        }
        Ok(Self(authored))
    }
}

impl From<Tags> for Vec<Tag> {
    fn from(tags: Tags) -> Self {
        tags.0
    }
}

impl fmt::Display for TagsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCategory => write!(f, "a node carries at least one `category:` tag"),
            Self::Repeated(tag) => write!(f, "`{tag}` is tagged more than once"),
        }
    }
}

impl std::error::Error for TagsError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(words: &[&str]) -> Result<Tags, TagsError> {
        let authored: Vec<Tag> = words
            .iter()
            .map(|word| word.parse().expect("a well-formed tag"))
            .collect();
        Tags::try_from(authored)
    }

    #[test]
    fn a_tag_is_a_word_or_a_namespaced_word_in_the_allowed_characters() {
        for well_formed in [
            "wip",
            "category:ui",
            "category:code_source",
            "artist:star-sie",
            "2026",
        ] {
            let tag: Tag = well_formed.parse().expect(well_formed);
            assert_eq!(tag.to_string(), well_formed);
        }
        for malformed in [
            "",
            "Code Source",
            "category:UI",
            "a:b:c",
            ":ui",
            "category:",
            "ui!",
        ] {
            let refused = malformed.parse::<Tag>().expect_err(malformed);
            let expected_message = format!(
                "`{malformed}` is not a tag: a tag is `word` or `namespace:word`, in lowercase letters, digits, `_` and `-`"
            );
            assert_eq!(refused.to_string(), expected_message);
        }
    }

    #[test]
    fn a_tag_knows_its_namespace_and_its_word() {
        let category: Tag = "category:ui".parse().expect("well-formed");
        assert_eq!(category.namespace(), Some("category"));
        assert_eq!(category.word(), "ui");
        assert!(category.is_category());
        assert_eq!(Tag::category("ui"), Ok(category));
        let bare: Tag = "wip".parse().expect("well-formed");
        assert_eq!(bare.namespace(), None);
        assert_eq!(bare.word(), "wip");
        assert!(!bare.is_category());
        assert!(
            Tag::category("a:b").is_err(),
            "a category is one word, never a namespace of its own"
        );
    }

    #[test]
    fn tags_keep_their_order_and_rank_within_a_namespace() {
        let authored = tags(&["wip", "category:tests", "artist:starsie", "category:source"]);
        let authored = authored.expect("a category, none twice");
        let tests: Tag = "category:tests".parse().expect("well-formed");
        let source: Tag = "category:source".parse().expect("well-formed");
        let finance: Tag = "category:finance".parse().expect("well-formed");
        let starsie: Tag = "artist:starsie".parse().expect("well-formed");
        assert_eq!(authored.authored()[0].as_str(), "wip", "never sorted");
        assert_eq!(authored.rank_of(&tests), Some(0));
        assert_eq!(
            authored.rank_of(&source),
            Some(1),
            "ranked among the categories, whatever stands between them"
        );
        assert_eq!(authored.rank_of(&starsie), Some(0));
        assert_eq!(authored.rank_of(&finance), None);
        assert!(authored.carries(&starsie));
        assert!(!authored.carries(&finance));
    }

    #[test]
    fn tags_need_a_category_and_refuse_a_repeat() {
        let no_category = tags(&["wip", "artist:starsie"]).expect_err("no category");
        assert_eq!(
            no_category.to_string(),
            "a node carries at least one `category:` tag"
        );
        let nothing = tags(&[]).expect_err("no category");
        assert_eq!(nothing, TagsError::NoCategory);
        let repeated = tags(&["category:ui", "wip", "wip"]).expect_err("repeated");
        assert_eq!(repeated.to_string(), "`wip` is tagged more than once");
    }
}
