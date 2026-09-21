//! The closed vocabulary a node is classified with: its one [`NodeType`]. A term outside the
//! vocabulary is a schema error; a new term is a new release. (What a directory specifically
//! holds is free-form: the `category:` tags of [`crate::tag`].)

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Declares a closed vocabulary: the enum, its terms in declaration order (`ALL`), and the std
/// traits that carry a term to and from its word — `Display`, `FromStr`, `TryFrom<String>` and
/// `From<Self> for String`, which serde rides on. The table below each invocation is the one
/// place a term is spelled and given its meaning (`Variant => "word", "meaning";`).
macro_rules! closed_vocabulary {
    (
        $(#[$vocabulary_doc:meta])*
        pub enum $name:ident, a $singular:literal among the $plural:literal {
            $( $variant:ident => $word:literal, $meaning:literal; )+
        }
    ) => {
        $(#[$vocabulary_doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub enum $name {
            $( #[doc = $meaning] $variant, )+
        }

        impl $name {
            /// Every term, in vocabulary order.
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )+ ];

            /// The term as a node file spells it.
            pub const fn word(self) -> &'static str {
                match self {
                    $( Self::$variant => $word, )+
                }
            }

            /// What the term means, in a few words — what `nodes vocabulary` prints.
            pub const fn meaning(self) -> &'static str {
                match self {
                    $( Self::$variant => $meaning, )+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.word())
            }
        }

        impl FromStr for $name {
            type Err = UnknownTerm;

            fn from_str(text: &str) -> Result<Self, UnknownTerm> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|term| term.word() == text)
                    .ok_or_else(|| {
                        let known: Vec<&str> = Self::ALL.iter().map(|term| term.word()).collect();
                        UnknownTerm {
                            singular: $singular,
                            plural: $plural,
                            given: text.to_owned(),
                            known: known.join(", "),
                        }
                    })
            }
        }

        impl TryFrom<String> for $name {
            type Error = UnknownTerm;

            fn try_from(text: String) -> Result<Self, UnknownTerm> {
                text.parse()
            }
        }

        impl From<$name> for String {
            fn from(term: $name) -> Self {
                term.word().to_owned()
            }
        }
    };
}

closed_vocabulary! {
    /// What a directory broadly holds — the one kind that fits it best.
    pub enum NodeType, a "type" among the "types" {
        Code => "code", "program source and what is built from it";
        Document => "document", "paperwork and written records";
        Art => "art", "artwork";
        Media => "media", "music, pictures, video";
        Data => "data", "datasets, exports, dumps";
        Software => "software", "installed applications, games, servers — not their source";
    }
}

/// A word that is not a term of the vocabulary it was offered to.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownTerm {
    singular: &'static str,
    plural: &'static str,
    given: String,
    known: String,
}

impl fmt::Display for UnknownTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown {} `{}`; the {} are {}",
            self.singular, self.given, self.plural, self.known
        )
    }
}

impl std::error::Error for UnknownTerm {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_term_is_spelled_once_and_carries_a_meaning() {
        let type_words: BTreeSet<&str> = NodeType::ALL.iter().map(|term| term.word()).collect();
        assert_eq!(type_words.len(), NodeType::ALL.len());
        assert!(NodeType::ALL.iter().all(|term| !term.meaning().is_empty()));
    }

    #[test]
    fn a_term_round_trips_through_its_word() {
        for node_type in NodeType::ALL {
            assert_eq!(node_type.word().parse::<NodeType>(), Ok(*node_type));
        }
        let unknown = "paperwork".parse::<NodeType>().expect_err("closed");
        assert_eq!(
            unknown.to_string(),
            "unknown type `paperwork`; the types are code, document, art, media, data, software"
        );
    }
}
