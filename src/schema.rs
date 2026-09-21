//! The schema IS these structs: a `NODE.json` is exactly one [`Node`], unknown keys refused.

use std::cmp::Ordering;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub use crate::date::ChartedDate;
pub use crate::tag::{Tag, Tags};
pub use crate::vocabulary::NodeType;

/// The file name every node lives in.
pub const NODE_FILE: &str = "NODE.json";

/// The key a node file ranked its categories under before `tags` existed (up to v0.5).
const LEGACY_CATEGORIES_KEY: &str = "categories";

/// One directory's chart: what it is, its rules, its named children and the design pages that govern it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// Repo-relative directory, `.` for the root.
    pub path: NodePath,
    /// The day this node was last charted.
    pub charted: ChartedDate,
    /// A few words for listings (`tree`, `ls`): what this directory is, at a glance.
    pub short: String,
    /// One sentence: what this directory IS.
    pub is: String,
    /// What the directory broadly holds: the one kind that fits best.
    #[serde(rename = "type")]
    pub node_type: NodeType,
    /// Free-form labels, `word` or `namespace:word`; the `category:` ones say what it specifically
    /// holds, ranked from most to least fitting. Author order.
    pub tags: Tags,
    /// One rule per entry, terse; author order.
    pub conventions: Vec<String>,
    /// Files to read first, relative to this directory; author order.
    pub entry_points: Vec<String>,
    /// Direct children worth naming; canonical order is by `name`.
    pub fs: Vec<FsEntry>,
    /// The only place code points at design; canonical order is by `page`.
    pub refs: Vec<Ref>,
    /// One note per entry; author order.
    pub notes: Vec<String>,
}

/// A named child of the directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FsEntry {
    pub name: String,
    pub role: String,
    /// Whether the child carries its own `NODE.json`.
    pub node: bool,
}

/// A Confluence page that governs something in this directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ref {
    pub page: PageId,
    pub title: String,
    pub governs: String,
}

impl Node {
    /// Parses one `NODE.json`; unknown keys, wrong types, bad dates and bad paths are all schema errors.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// Parses a node file written before `tags` existed: its ranked `categories` become
    /// `category:` tags, in the same order. The one door for such a file — `nodes migrate` walks a
    /// tree through it; a file that already carries `tags` parses as usual, so `categories` left
    /// beside them are refused rather than silently dropped.
    pub fn parse_migrating(text: &str) -> Result<Self, serde_json::Error> {
        let mut whole: serde_json::Value = serde_json::from_str(text)?;
        if let Some(fields) = whole.as_object_mut() {
            let tags_key = Field::Tags.key();
            let carries_tags = fields.contains_key(tags_key);
            let legacy_categories = (!carries_tags)
                .then(|| fields.remove(LEGACY_CATEGORIES_KEY))
                .flatten();
            if let Some(categories) = legacy_categories {
                fields.insert(tags_key.to_owned(), category_tags(categories));
            }
        }
        serde_json::from_value(whole)
    }

    /// Parses a node file whose classification the caller supplies, replacing whatever the file
    /// says: its `type`, and its `category:` tags — the given ones lead, the file's other tags
    /// follow. The one door for a file written before `type` existed. Everything else must fit
    /// the schema as usual.
    pub fn parse_classified(
        text: &str,
        node_type: NodeType,
        categories: &[Tag],
    ) -> Result<Self, serde_json::Error> {
        let mut whole: serde_json::Value = serde_json::from_str(text)?;
        if let Some(fields) = whole.as_object_mut() {
            fields.remove(LEGACY_CATEGORIES_KEY);
            let type_key = Field::Type.key().to_owned();
            let tags_key = Field::Tags.key().to_owned();
            let other_tags: Vec<serde_json::Value> = fields
                .get(&tags_key)
                .and_then(serde_json::Value::as_array)
                .map(|tags| tags.iter().filter(|tag| !is_category_tag(tag)).cloned())
                .map(Iterator::collect)
                .unwrap_or_default();
            let mut tags: Vec<serde_json::Value> = categories
                .iter()
                .map(|category| serde_json::json!(category))
                .collect();
            tags.extend(other_tags);
            fields.insert(type_key, serde_json::json!(node_type));
            fields.insert(tags_key, serde_json::Value::Array(tags));
        }
        serde_json::from_value(whole)
    }

    /// The same node in canonical order: `fs` by name, `refs` by page, everything else as authored.
    pub fn canonical(mut self) -> Self {
        self.fs.sort_by(|left, right| left.name.cmp(&right.name));
        self.refs.sort_by_key(|reference| reference.page);
        self
    }

    /// The canonical file text: struct field order, two-space indent, one trailing newline.
    pub fn canonical_json(&self) -> String {
        let canonical = self.clone().canonical();
        let mut text = serde_json::to_string_pretty(&canonical).expect("a Node always serializes");
        text.push('\n');
        text
    }

    /// One field as a JSON value.
    pub fn field(&self, field: Field) -> serde_json::Value {
        let whole = serde_json::to_value(self).expect("a Node always serializes");
        whole[field.key()].clone()
    }

    /// The node with one field replaced; the value must fit the field's type.
    pub fn with_field(
        &self,
        field: Field,
        value: serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        let mut whole = serde_json::to_value(self)?;
        whole[field.key()] = value;
        serde_json::from_value(whole)
    }

    /// The ref citing `page`, if any.
    pub fn reference(&self, page: PageId) -> Option<&Ref> {
        self.refs.iter().find(|reference| reference.page == page)
    }
}

/// A legacy `categories` value as the `tags` it becomes: each word behind the `category:`
/// namespace. Whatever is not a list of words is passed on as it is, for the schema to refuse.
fn category_tags(categories: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Array(words) = categories else {
        return categories;
    };
    let tags = words
        .into_iter()
        .map(|word| match word.as_str() {
            Some(word) => serde_json::json!(format!("{}:{word}", crate::tag::CATEGORY_NAMESPACE)),
            None => word,
        })
        .collect();
    serde_json::Value::Array(tags)
}

/// Whether a raw `tags` entry sits in the `category` namespace.
fn is_category_tag(tag: &serde_json::Value) -> bool {
    tag.as_str()
        .and_then(|text| text.split_once(':'))
        .is_some_and(|(namespace, _)| namespace == crate::tag::CATEGORY_NAMESPACE)
}

/// The fields of a node, as `get` and `set` name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Path,
    Charted,
    Short,
    Is,
    Type,
    Tags,
    Conventions,
    EntryPoints,
    Fs,
    Refs,
    Notes,
}

impl Field {
    /// Every field, in schema order.
    pub const ALL: [Self; 11] = [
        Self::Path,
        Self::Charted,
        Self::Short,
        Self::Is,
        Self::Type,
        Self::Tags,
        Self::Conventions,
        Self::EntryPoints,
        Self::Fs,
        Self::Refs,
        Self::Notes,
    ];

    /// The JSON key.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Charted => "charted",
            Self::Short => "short",
            Self::Is => "is",
            Self::Type => "type",
            Self::Tags => "tags",
            Self::Conventions => "conventions",
            Self::EntryPoints => "entry_points",
            Self::Fs => "fs",
            Self::Refs => "refs",
            Self::Notes => "notes",
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

impl FromStr for Field {
    type Err = UnknownField;

    fn from_str(text: &str) -> Result<Self, UnknownField> {
        Self::ALL
            .into_iter()
            .find(|field| field.key() == text)
            .ok_or_else(|| UnknownField(text.to_owned()))
    }
}

/// A field name the schema does not have.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownField(String);

impl fmt::Display for UnknownField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let known = Field::ALL.map(Field::key).join(", ");
        write!(f, "unknown field `{}`; the fields are {known}", self.0)
    }
}

impl std::error::Error for UnknownField {}

/// A node's repo-relative directory: `.` for the root, otherwise `a/b/c` — no leading `./` or `/`,
/// no trailing `/`, no `.` or `..` components. Ordered by components, so a directory sorts before
/// everything beneath it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NodePath(String);

/// Why a string is not a `NodePath`.
#[derive(Debug, PartialEq, Eq)]
pub enum PathError {
    Empty,
    Absolute(String),
    Component { path: String, component: String },
    OutsideRepo(String),
    NotUtf8(String),
}

impl NodePath {
    /// The repository root, `.`.
    pub fn root() -> Self {
        Self(".".to_owned())
    }

    pub fn is_root(&self) -> bool {
        self.0 == "."
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path's directory names, top down; none for the root.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        let inner = if self.is_root() { "" } else { self.0.as_str() };
        inner.split('/').filter(|component| !component.is_empty())
    }

    pub fn depth(&self) -> usize {
        self.components().count()
    }

    /// The directory holding this one; `None` for the root.
    pub fn parent(&self) -> Option<Self> {
        if self.is_root() {
            return None;
        }
        let Some(cut) = self.0.rfind('/') else {
            return Some(Self::root());
        };
        Some(Self(self.0[..cut].to_owned()))
    }

    /// The path `below` this one — one or more components, a trailing `/` tolerated.
    pub fn join(&self, below: &str) -> Result<Self, PathError> {
        let below = below.trim_end_matches('/');
        let joined = if self.is_root() {
            below.to_owned()
        } else {
            format!("{}/{below}", self.0)
        };
        Self::try_from(joined)
    }

    /// Whether `other` lies strictly below this path.
    pub fn is_ancestor_of(&self, other: &Self) -> bool {
        self != other && self.relative(other).is_some()
    }

    /// `other` relative to this path (`""` when equal), or `None` when `other` is not at or below it.
    pub fn relative<'a>(&self, other: &'a Self) -> Option<&'a str> {
        if self.is_root() {
            let below = if other.is_root() { "" } else { other.as_str() };
            return Some(below);
        }
        if other == self {
            return Some("");
        }
        other.0.strip_prefix(self.0.as_str())?.strip_prefix('/')
    }

    /// This path as seen from a tree that mounts this path's own tree at `mount_point`: the
    /// mounted root becomes the mount point, everything else sits beneath it.
    pub fn mounted_at(&self, mount_point: &Self) -> Self {
        if self.is_root() {
            return mount_point.clone();
        }
        if mount_point.is_root() {
            return self.clone();
        }
        Self(format!("{mount_point}/{self}"))
    }

    /// The directory on disk under `root`.
    pub fn to_dir(&self, root: &Path) -> PathBuf {
        if self.is_root() {
            root.to_path_buf()
        } else {
            root.join(&self.0)
        }
    }

    /// Turns a typed path — repo-relative as `nodes tree` prints it, or absolute — into a node
    /// path, tolerating `./`, a trailing `/` and a trailing `/NODE.json`.
    pub fn normalize(input: &str, root: &Path) -> Result<Self, PathError> {
        let outside = || PathError::OutsideRepo(input.to_owned());
        let given = Path::new(input);
        let relative = if given.is_absolute() {
            given.strip_prefix(root).map_err(|_| outside())?
        } else {
            given
        };
        let is_node_file = relative.file_name().is_some_and(|name| name == NODE_FILE);
        let dir = if is_node_file {
            relative.parent().unwrap_or_else(|| Path::new(""))
        } else {
            relative
        };
        let mut kept: Vec<&str> = Vec::new();
        for component in dir.components() {
            match component {
                Component::Normal(name) => {
                    let name = name
                        .to_str()
                        .ok_or_else(|| PathError::NotUtf8(input.to_owned()))?;
                    kept.push(name);
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    kept.pop().ok_or_else(outside)?;
                }
                Component::RootDir | Component::Prefix(_) => return Err(outside()),
            }
        }
        if kept.is_empty() {
            return Ok(Self::root());
        }
        Self::try_from(kept.join("/"))
    }
}

impl TryFrom<String> for NodePath {
    type Error = PathError;

    fn try_from(value: String) -> Result<Self, PathError> {
        if value == "." {
            return Ok(Self(value));
        }
        if value.is_empty() {
            return Err(PathError::Empty);
        }
        if value.starts_with('/') {
            return Err(PathError::Absolute(value));
        }
        let offending = value
            .split('/')
            .find(|component| component.is_empty() || matches!(*component, "." | ".."))
            .map(str::to_owned);
        if let Some(component) = offending {
            return Err(PathError::Component {
                path: value,
                component,
            });
        }
        Ok(Self(value))
    }
}

impl TryFrom<&str> for NodePath {
    type Error = PathError;

    fn try_from(value: &str) -> Result<Self, PathError> {
        Self::try_from(value.to_owned())
    }
}

impl FromStr for NodePath {
    type Err = PathError;

    fn from_str(value: &str) -> Result<Self, PathError> {
        Self::try_from(value)
    }
}

impl From<NodePath> for String {
    fn from(path: NodePath) -> Self {
        path.0
    }
}

impl fmt::Display for NodePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Ord for NodePath {
    fn cmp(&self, other: &Self) -> Ordering {
        self.components().cmp(other.components())
    }
}

impl PartialOrd for NodePath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a node path cannot be empty (the root is `.`)"),
            Self::Absolute(path) => write!(f, "`{path}` is absolute; node paths are repo-relative"),
            Self::Component { path, component } => write!(
                f,
                "`{path}`: component `{component}` is not allowed (no empty, `.` or `..` components)"
            ),
            Self::OutsideRepo(path) => write!(f, "`{path}` lies outside the repository"),
            Self::NotUtf8(path) => write!(f, "`{path}` is not UTF-8"),
        }
    }
}

impl std::error::Error for PathError {}

/// A Confluence page id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PageId(u64);

/// A page id that is not a run of digits.
#[derive(Debug, PartialEq, Eq)]
pub struct PageIdError(String);

impl PageId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for PageId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl FromStr for PageId {
    type Err = PageIdError;

    fn from_str(text: &str) -> Result<Self, PageIdError> {
        text.parse::<u64>()
            .map(Self)
            .map_err(|_| PageIdError(text.to_owned()))
    }
}

impl fmt::Display for PageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for PageIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` is not a Confluence page id (digits only)", self.0)
    }
}

impl std::error::Error for PageIdError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> &'static str {
        r#"{
  "path": "backend/crates",
  "charted": "2026-09-12",
  "short": "The crates",
  "is": "Twelve crates.",
  "type": "code",
  "tags": ["category:source", "wip", "category:tests"],
  "conventions": ["b second", "a first"],
  "entry_points": ["src/lib.rs"],
  "fs": [
    {"name": "domain/", "role": "the core", "node": true},
    {"name": "api/", "role": "the driver", "node": true}
  ],
  "refs": [
    {"page": 55836674, "title": "The Application Layer", "governs": "use cases"},
    {"page": 11763713, "title": "Domains and Applications", "governs": "the dependency rule"}
  ],
  "notes": []
}"#
    }

    #[test]
    fn canonical_form_sorts_fs_and_refs_but_keeps_author_order_elsewhere() {
        let node = Node::parse(sample()).expect("valid").canonical();
        let fs_names: Vec<&str> = node.fs.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(fs_names, ["api/", "domain/"]);
        let pages: Vec<u64> = node.refs.iter().map(|r| r.page.get()).collect();
        assert_eq!(pages, [11_763_713, 55_836_674]);
        assert_eq!(node.conventions, ["b second", "a first"]);
    }

    #[test]
    fn canonical_json_is_a_fixed_point() {
        let once = Node::parse(sample()).expect("valid").canonical_json();
        let twice = Node::parse(&once).expect("still valid").canonical_json();
        assert_eq!(once, twice);
        assert!(once.ends_with("}\n"));
        assert!(once.starts_with("{\n  \"path\": \"backend/crates\",\n  \"charted\":"));
    }

    #[test]
    fn tags_keep_their_order_and_the_type_stays_closed() {
        let node = Node::parse(sample()).expect("valid").canonical();
        let authored: Vec<&str> = node.tags.authored().iter().map(Tag::as_str).collect();
        assert_eq!(
            authored,
            ["category:source", "wip", "category:tests"],
            "as authored, never sorted"
        );
        let tests = Tag::category("tests").expect("well-formed");
        let finance = Tag::category("finance").expect("well-formed");
        assert_eq!(node.tags.rank_of(&tests), Some(1));
        assert_eq!(node.tags.rank_of(&finance), None);
        assert_eq!(node.node_type, NodeType::Code);
        let free_form = sample().replacen(r#""wip""#, r#""artist:starsie""#, 1);
        assert!(
            Node::parse(&free_form).is_ok(),
            "no vocabulary closes the tags"
        );
        let malformed = sample().replacen(r#""wip""#, r#""Code Source""#, 1);
        let malformed_error = Node::parse(&malformed).expect_err("not a tag");
        assert!(
            malformed_error
                .to_string()
                .contains("`Code Source` is not a tag: a tag is `word` or `namespace:word`")
        );
        let unknown_type = sample().replacen(r#""type": "code""#, r#""type": "paperwork""#, 1);
        let unknown_type_error = Node::parse(&unknown_type).expect_err("closed");
        assert!(unknown_type_error.to_string().contains(
            "unknown type `paperwork`; the types are code, document, art, media, data, software"
        ));
        let no_category = sample().replacen(
            r#"["category:source", "wip", "category:tests"]"#,
            r#"["wip"]"#,
            1,
        );
        let no_category_error = Node::parse(&no_category).expect_err("at least one");
        assert!(
            no_category_error
                .to_string()
                .contains("at least one `category:` tag")
        );
        let repeated = sample().replacen(r#""category:tests"]"#, r#""wip"]"#, 1);
        let repeated_error = Node::parse(&repeated).expect_err("tagged once");
        assert!(
            repeated_error
                .to_string()
                .contains("`wip` is tagged more than once")
        );
    }

    #[test]
    fn a_file_with_ranked_categories_is_migrated_and_refused_everywhere_else() {
        let legacy = sample().replacen(
            r#""tags": ["category:source", "wip", "category:tests"]"#,
            r#""categories": ["tests", "source"]"#,
            1,
        );
        let refused = Node::parse(&legacy).expect_err("the schema has no `categories`");
        assert!(refused.to_string().contains("unknown field `categories`"));
        let migrated = Node::parse_migrating(&legacy).expect("migrated");
        let authored: Vec<&str> = migrated.tags.authored().iter().map(Tag::as_str).collect();
        assert_eq!(
            authored,
            ["category:tests", "category:source"],
            "the ranking carries over"
        );
        let current = Node::parse(sample()).expect("valid");
        assert_eq!(
            Node::parse_migrating(sample()).expect("already migrated"),
            current,
            "a file that carries `tags` is left as it is"
        );
        let both = sample().replacen(r#""notes": []"#, r#""notes": [], "categories": ["ui"]"#, 1);
        let both_error = Node::parse_migrating(&both).expect_err("which one is meant?");
        assert!(
            both_error
                .to_string()
                .contains("unknown field `categories`"),
            "`categories` left beside `tags` are refused, never silently dropped"
        );
    }

    #[test]
    fn classifying_replaces_the_categories_and_keeps_the_other_tags() {
        let ui = Tag::category("ui").expect("well-formed");
        let classified =
            Node::parse_classified(sample(), NodeType::Document, &[ui]).expect("classified");
        let authored: Vec<&str> = classified.tags.authored().iter().map(Tag::as_str).collect();
        assert_eq!(authored, ["category:ui", "wip"]);
        assert_eq!(classified.node_type, NodeType::Document);
    }

    #[test]
    fn unknown_keys_and_missing_fields_are_schema_errors() {
        let extra = sample().replacen("\"notes\": []", "\"notes\": [], \"extra\": 1", 1);
        let extra_error = Node::parse(&extra).expect_err("unknown key refused");
        assert!(extra_error.to_string().contains("unknown field `extra`"));
        let missing = sample().replacen("\"notes\": []", "\"notes_\": []", 1);
        assert!(Node::parse(&missing).is_err());
    }

    #[test]
    fn set_replaces_one_field_and_validates_it() {
        let node = Node::parse(sample()).expect("valid");
        let renamed = node
            .with_field(Field::Is, serde_json::json!("Thirteen crates."))
            .expect("a string fits `is`");
        assert_eq!(renamed.is, "Thirteen crates.");
        let wrong_type = node.with_field(Field::Notes, serde_json::json!("not a list"));
        assert!(wrong_type.is_err());
    }

    #[test]
    fn node_paths_validate_and_order_by_components() {
        assert!(NodePath::try_from("./a").is_err());
        assert!(NodePath::try_from("a/").is_err());
        assert!(NodePath::try_from("a/../b").is_err());
        assert!(NodePath::try_from("/a").is_err());
        let root = NodePath::root();
        let backend: NodePath = "backend".parse().expect("valid");
        let backend_crates: NodePath = "backend/crates".parse().expect("valid");
        let backend_dash: NodePath = "backend-x".parse().expect("valid");
        assert!(root < backend);
        assert!(backend < backend_crates);
        assert!(backend_crates < backend_dash);
        assert_eq!(backend_crates.parent(), Some(backend.clone()));
        assert_eq!(backend.parent(), Some(root.clone()));
        assert!(root.is_ancestor_of(&backend_crates));
        assert!(backend.is_ancestor_of(&backend_crates));
        assert!(!backend_dash.is_ancestor_of(&backend_crates));
        assert_eq!(backend.relative(&backend_crates), Some("crates"));
        assert_eq!(root.relative(&backend_crates), Some("backend/crates"));
    }

    #[test]
    fn a_mounted_path_is_rebased_onto_the_mount_point() {
        let root = NodePath::root();
        let mount_point: NodePath = "code/zurfur".parse().expect("valid");
        let backend: NodePath = "backend".parse().expect("valid");
        let rebased: NodePath = "code/zurfur/backend".parse().expect("valid");
        assert_eq!(backend.mounted_at(&mount_point), rebased);
        assert_eq!(root.mounted_at(&mount_point), mount_point);
        assert_eq!(backend.mounted_at(&root), backend);
    }

    #[test]
    fn normalize_accepts_the_shapes_people_type() {
        let root = Path::new("/repo");
        let expected: NodePath = "backend/crates".parse().expect("valid");
        for input in [
            "backend/crates",
            "backend/crates/",
            "./backend/crates",
            "backend/crates/NODE.json",
            "/repo/backend/crates/NODE.json",
            "backend/x/../crates",
        ] {
            let normalized = NodePath::normalize(input, root).expect(input);
            assert_eq!(normalized, expected, "{input}");
        }
        assert_eq!(NodePath::normalize("", root), Ok(NodePath::root()));
        assert_eq!(NodePath::normalize(".", root), Ok(NodePath::root()));
        assert_eq!(
            NodePath::normalize("/repo/NODE.json", root),
            Ok(NodePath::root())
        );
        assert!(NodePath::normalize("/elsewhere/x", root).is_err());
        assert!(NodePath::normalize("../x", root).is_err());
    }
}
