//! The closed vocabularies a node is classified with: its one [`NodeType`] and its ranked
//! [`Categories`]. A term outside a vocabulary is a schema error; a new term is a new release.

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

closed_vocabulary! {
    /// What a directory specifically holds. A node ranks the categories that fit it, best first;
    /// any category may sit under any [`NodeType`].
    pub enum Category, a "category" among the "categories" {
        Project => "project", "the root of a whole software project";
        Source => "source", "hand-written program code";
        Ui => "ui", "user-interface code";
        Schema => "schema", "interface definitions: protobuf, lexicons, file formats";
        Tests => "tests", "suites, harnesses, fixtures, fakes";
        Generated => "generated", "machine-written output, never edited by hand";
        Tooling => "tooling", "scripts, code generators, macros, CI";
        Infrastructure => "infrastructure", "services a project runs on: proxies, containers";
        Config => "config", "configuration and environment";
        Docs => "docs", "documentation and pointers";
        Design => "design", "decisions and deliberation";
        Identity => "identity", "IDs and civil records";
        Education => "education", "diplomas, courses, admissions";
        Finance => "finance", "invoices, receipts, taxes, banking";
        Housing => "housing", "homes, leases, utilities";
        Work => "work", "employers, companies, CVs";
        Health => "health", "medical records";
        Legal => "legal", "contracts and legal papers";
        Art => "art", "artwork";
        Media => "media", "music, pictures, video";
        Inbox => "inbox", "unsorted intake";
        Archive => "archive", "no longer current, kept";
        Index => "index", "catalogs and manifests over other content";
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

/// A node's categories, ranked from most to least fitting: at least one, none twice. The order is
/// the author's and `fmt` never sorts it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Category>", into = "Vec<Category>")]
pub struct Categories(Vec<Category>);

/// Why a list of categories is not a ranking.
#[derive(Debug, PartialEq, Eq)]
pub enum CategoriesError {
    Empty,
    Repeated(Category),
}

impl Categories {
    /// The categories, best fit first.
    pub fn ranked(&self) -> &[Category] {
        &self.0
    }

    /// How well `category` fits: `0` for the best fit, `None` when the node does not carry it.
    pub fn rank_of(&self, category: Category) -> Option<usize> {
        self.0.iter().position(|ranked| *ranked == category)
    }
}

impl TryFrom<Vec<Category>> for Categories {
    type Error = CategoriesError;

    fn try_from(ranked: Vec<Category>) -> Result<Self, CategoriesError> {
        if ranked.is_empty() {
            return Err(CategoriesError::Empty);
        }
        let repeated = ranked
            .iter()
            .enumerate()
            .find(|(rank, category)| ranked[..*rank].contains(category))
            .map(|(_, category)| *category);
        if let Some(category) = repeated {
            return Err(CategoriesError::Repeated(category));
        }
        Ok(Self(ranked))
    }
}

impl From<Categories> for Vec<Category> {
    fn from(categories: Categories) -> Self {
        categories.0
    }
}

impl fmt::Display for CategoriesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a node carries at least one category"),
            Self::Repeated(category) => write!(f, "`{category}` is ranked more than once"),
        }
    }
}

impl std::error::Error for CategoriesError {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_term_is_spelled_once_and_carries_a_meaning() {
        let type_words: BTreeSet<&str> = NodeType::ALL.iter().map(|term| term.word()).collect();
        assert_eq!(type_words.len(), NodeType::ALL.len());
        let category_words: BTreeSet<&str> = Category::ALL.iter().map(|term| term.word()).collect();
        assert_eq!(category_words.len(), Category::ALL.len());
        assert!(NodeType::ALL.iter().all(|term| !term.meaning().is_empty()));
        assert!(Category::ALL.iter().all(|term| !term.meaning().is_empty()));
    }

    #[test]
    fn a_term_round_trips_through_its_word() {
        for category in Category::ALL {
            assert_eq!(category.word().parse::<Category>(), Ok(*category));
        }
        let unknown = "paperwork".parse::<NodeType>().expect_err("closed");
        assert_eq!(
            unknown.to_string(),
            "unknown type `paperwork`; the types are code, document, art, media, data, software"
        );
    }
}
