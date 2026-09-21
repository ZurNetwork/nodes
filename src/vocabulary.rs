//! The closed vocabularies a node is classified with: its one [`NodeType`] and its ranked
//! [`Categories`]. A term outside a vocabulary is a schema error; a new term is a new release.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Declares a closed vocabulary: the enum, its terms in declaration order (`ALL`), and the std
/// traits that carry a term to and from its word — `Display`, `FromStr`, `TryFrom<String>` and
/// `From<Self> for String`, which serde rides on. The table below each invocation is the one
/// place a term is spelled.
macro_rules! closed_vocabulary {
    (
        $(#[$vocabulary_doc:meta])*
        pub enum $name:ident, a $singular:literal among the $plural:literal {
            $( $(#[$term_doc:meta])* $variant:ident => $word:literal, )+
        }
    ) => {
        $(#[$vocabulary_doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub enum $name {
            $( $(#[$term_doc])* $variant, )+
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
        /// Program source and what is built from it.
        Code => "code",
        /// Paperwork and written records.
        Document => "document",
        /// Artwork.
        Art => "art",
        /// Music, pictures, video.
        Media => "media",
        /// Datasets, exports, dumps.
        Data => "data",
        /// Installed applications, games, servers — not their source.
        Software => "software",
    }
}

closed_vocabulary! {
    /// What a directory specifically holds. A node ranks the categories that fit it, best first;
    /// any category may sit under any [`NodeType`].
    pub enum Category, a "category" among the "categories" {
        /// The root of a whole software project.
        Project => "project",
        /// Hand-written program code.
        Source => "source",
        /// User-interface code.
        Ui => "ui",
        /// Interface definitions: protobuf, lexicons, file formats.
        Schema => "schema",
        /// Suites, harnesses, fixtures, fakes.
        Tests => "tests",
        /// Machine-written output, never edited by hand.
        Generated => "generated",
        /// Scripts, code generators, macros, CI.
        Tooling => "tooling",
        /// Services a project runs on: proxies, containers.
        Infrastructure => "infrastructure",
        /// Configuration and environment.
        Config => "config",
        /// Documentation and pointers.
        Docs => "docs",
        /// Decisions and deliberation.
        Design => "design",
        /// IDs and civil records.
        Identity => "identity",
        /// Diplomas, courses, admissions.
        Education => "education",
        /// Invoices, receipts, taxes, banking.
        Finance => "finance",
        /// Homes, leases, utilities.
        Housing => "housing",
        /// Employers, companies, CVs.
        Work => "work",
        /// Medical records.
        Health => "health",
        /// Contracts and legal papers.
        Legal => "legal",
        /// Artwork.
        Art => "art",
        /// Music, pictures, video.
        Media => "media",
        /// Unsorted intake.
        Inbox => "inbox",
        /// No longer current, kept.
        Archive => "archive",
        /// Catalogs and manifests over other content.
        Index => "index",
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
