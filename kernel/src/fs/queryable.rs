//! Queryable file metadata / indexed attributes (BeOS BFS-inspired).
//!
//! Provides a structured attribute store where files can carry typed
//! key-value metadata beyond the basic filesystem attributes.  Unlike
//! simple tags (which are untyped strings), queryable attributes have
//! explicit types and support relational queries (equality, range,
//! prefix, contains).
//!
//! ## Design Reference
//!
//! design.txt lines 35-37: "BFS (Be File System) had rich queryable
//! metadata built in — you could search files by any attribute (artist,
//! bitrate, email sender) as fast as a database query."
//!
//! design.txt line 249: "Database-as-filesystem — rich metadata and
//! queries built into the filesystem."
//!
//! roadmap.md: "Queryable file metadata / indexed attributes (BeOS
//! BFS-inspired)"
//!
//! ## Architecture
//!
//! ```text
//! Application sets attribute:
//!   set_attr("/music/song.mp3", "Audio:Artist", AttrValue::Text("Beatles"))
//!   set_attr("/music/song.mp3", "Audio:Bitrate", AttrValue::Int(320))
//!
//! Application queries:
//!   query(&[Predicate::eq("Audio:Artist", "Beatles")], "/music")
//!   query(&[Predicate::gt("Audio:Bitrate", 256)], "/music")
//!   query(&[Predicate::contains("Email:Subject", "urgent")], "/mail")
//!
//! Index accelerates lookups:
//!   AttrIndex: attr_name → BTreeMap<value, Set<path>>
//! ```
//!
//! ## Attribute Naming Convention
//!
//! Attributes use a `Category:Name` convention (like BeOS):
//! - `Audio:Artist`, `Audio:Album`, `Audio:Bitrate`, `Audio:Duration`
//! - `Image:Width`, `Image:Height`, `Image:ColorSpace`
//! - `Email:From`, `Email:Subject`, `Email:Date`
//! - `Document:Author`, `Document:Title`, `Document:PageCount`
//! - `App:*` — application-specific attributes
//!
//! ## Limits
//!
//! - Max 64 attributes per file
//! - Max 256 characters per attribute name
//! - Max 4096 bytes per text attribute value
//! - Max 65536 files in the attribute store
//! - Max 1024 indexed attribute names

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum attributes per file.
const MAX_ATTRS_PER_FILE: usize = 64;

/// Maximum attribute name length.
const MAX_ATTR_NAME_LEN: usize = 256;

/// Maximum text value length in bytes.
const MAX_TEXT_VALUE_LEN: usize = 4096;

/// Maximum files in the store.
const MAX_FILES: usize = 65536;

/// Maximum indexed attribute names.
const MAX_INDEXED_ATTRS: usize = 1024;

/// Maximum query results.
const MAX_QUERY_RESULTS: usize = 4096;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Typed attribute value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AttrValue {
    /// Text string.
    Text(String),
    /// 64-bit signed integer.
    Int(i64),
    /// Boolean.
    Bool(bool),
    /// Raw bytes.
    Bytes(Vec<u8>),
}

impl AttrValue {
    /// Returns the type name for display.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Int(_) => "int",
            Self::Bool(_) => "bool",
            Self::Bytes(_) => "bytes",
        }
    }

    /// Returns approximate size in bytes.
    pub fn size(&self) -> usize {
        match self {
            Self::Text(s) => s.len(),
            Self::Int(_) => 8,
            Self::Bool(_) => 1,
            Self::Bytes(b) => b.len(),
        }
    }
}

/// Comparison operator for queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    /// Exact equality.
    Equal,
    /// Not equal.
    NotEqual,
    /// Less than (Int only).
    LessThan,
    /// Less than or equal (Int only).
    LessEqual,
    /// Greater than (Int only).
    GreaterThan,
    /// Greater than or equal (Int only).
    GreaterEqual,
    /// Text contains substring (Text only).
    Contains,
    /// Text starts with prefix (Text only).
    StartsWith,
    /// Text ends with suffix (Text only).
    EndsWith,
}

/// A single query predicate.
#[derive(Debug, Clone)]
pub struct Predicate {
    /// Attribute name to match.
    pub attr_name: String,
    /// Comparison operator.
    pub op: CompareOp,
    /// Value to compare against.
    pub value: AttrValue,
}

impl Predicate {
    /// Create an equality predicate for text.
    pub fn eq_text(name: &str, value: &str) -> Self {
        Self {
            attr_name: String::from(name),
            op: CompareOp::Equal,
            value: AttrValue::Text(String::from(value)),
        }
    }

    /// Create an equality predicate for int.
    pub fn eq_int(name: &str, value: i64) -> Self {
        Self {
            attr_name: String::from(name),
            op: CompareOp::Equal,
            value: AttrValue::Int(value),
        }
    }

    /// Create a greater-than predicate for int.
    pub fn gt_int(name: &str, value: i64) -> Self {
        Self {
            attr_name: String::from(name),
            op: CompareOp::GreaterThan,
            value: AttrValue::Int(value),
        }
    }

    /// Create a less-than predicate for int.
    pub fn lt_int(name: &str, value: i64) -> Self {
        Self {
            attr_name: String::from(name),
            op: CompareOp::LessThan,
            value: AttrValue::Int(value),
        }
    }

    /// Create a contains predicate for text.
    pub fn contains(name: &str, substring: &str) -> Self {
        Self {
            attr_name: String::from(name),
            op: CompareOp::Contains,
            value: AttrValue::Text(String::from(substring)),
        }
    }

    /// Create a starts-with predicate for text.
    pub fn starts_with(name: &str, prefix: &str) -> Self {
        Self {
            attr_name: String::from(name),
            op: CompareOp::StartsWith,
            value: AttrValue::Text(String::from(prefix)),
        }
    }

    /// Evaluate this predicate against a stored value.
    fn matches(&self, stored: &AttrValue) -> bool {
        match (&self.op, &self.value, stored) {
            // Text equality.
            (CompareOp::Equal, AttrValue::Text(a), AttrValue::Text(b)) => {
                a.eq_ignore_ascii_case(b)
            }
            (CompareOp::NotEqual, AttrValue::Text(a), AttrValue::Text(b)) => {
                !a.eq_ignore_ascii_case(b)
            }

            // Text substring operations.
            (CompareOp::Contains, AttrValue::Text(needle), AttrValue::Text(haystack)) => {
                let h_lower = to_ascii_lowercase(haystack);
                let n_lower = to_ascii_lowercase(needle);
                h_lower.contains(n_lower.as_str())
            }
            (CompareOp::StartsWith, AttrValue::Text(prefix), AttrValue::Text(haystack)) => {
                let h_lower = to_ascii_lowercase(haystack);
                let p_lower = to_ascii_lowercase(prefix);
                h_lower.starts_with(p_lower.as_str())
            }
            (CompareOp::EndsWith, AttrValue::Text(suffix), AttrValue::Text(haystack)) => {
                let h_lower = to_ascii_lowercase(haystack);
                let s_lower = to_ascii_lowercase(suffix);
                h_lower.ends_with(s_lower.as_str())
            }

            // Int comparisons.
            (CompareOp::Equal, AttrValue::Int(a), AttrValue::Int(b)) => a == b,
            (CompareOp::NotEqual, AttrValue::Int(a), AttrValue::Int(b)) => a != b,
            (CompareOp::LessThan, AttrValue::Int(threshold), AttrValue::Int(val)) => {
                *val < *threshold
            }
            (CompareOp::LessEqual, AttrValue::Int(threshold), AttrValue::Int(val)) => {
                *val <= *threshold
            }
            (CompareOp::GreaterThan, AttrValue::Int(threshold), AttrValue::Int(val)) => {
                *val > *threshold
            }
            (CompareOp::GreaterEqual, AttrValue::Int(threshold), AttrValue::Int(val)) => {
                *val >= *threshold
            }

            // Bool equality.
            (CompareOp::Equal, AttrValue::Bool(a), AttrValue::Bool(b)) => a == b,
            (CompareOp::NotEqual, AttrValue::Bool(a), AttrValue::Bool(b)) => a != b,

            // Bytes equality.
            (CompareOp::Equal, AttrValue::Bytes(a), AttrValue::Bytes(b)) => a == b,
            (CompareOp::NotEqual, AttrValue::Bytes(a), AttrValue::Bytes(b)) => a != b,

            // Type mismatch — no match.
            _ => false,
        }
    }
}

/// How to combine multiple predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    /// All predicates must match (AND).
    All,
    /// At least one predicate must match (OR).
    Any,
}

/// A query result entry.
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// File path.
    pub path: String,
    /// Matching attribute values (name → value).
    pub matched_attrs: Vec<(String, AttrValue)>,
}

/// Attribute schema for indexed attributes.
#[derive(Debug, Clone)]
pub struct AttrSchema {
    /// Attribute name.
    pub name: String,
    /// Expected value type.
    pub value_type: &'static str,
    /// Whether this attribute is indexed for fast queries.
    pub indexed: bool,
    /// Description.
    pub description: String,
}

// ---------------------------------------------------------------------------
// Internal storage
// ---------------------------------------------------------------------------

/// Per-file attribute set.
struct FileAttrs {
    /// File path.
    path: String,
    /// Attribute name → value.
    attrs: BTreeMap<String, AttrValue>,
}

/// Global attribute store.
struct AttrStore {
    /// Path → index in `files`.
    path_index: BTreeMap<String, usize>,
    /// All files with attributes.
    files: Vec<FileAttrs>,
    /// Free slots (indices of removed entries).
    free_slots: Vec<usize>,
    /// Registered schemas.
    schemas: BTreeMap<String, AttrSchema>,
    /// Attribute name → (value_key → set of paths) for indexed attrs.
    ///
    /// The value_key is a string representation of the value for
    /// BTreeMap ordering. This trades exact type fidelity for O(log n)
    /// indexed lookup.
    indexes: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
    /// Which attribute names are indexed.
    indexed_names: BTreeSet<String>,
}

impl AttrStore {
    const fn new() -> Self {
        Self {
            path_index: BTreeMap::new(),
            files: Vec::new(),
            free_slots: Vec::new(),
            schemas: BTreeMap::new(),
            indexes: BTreeMap::new(),
            indexed_names: BTreeSet::new(),
        }
    }
}

static STORE: Mutex<AttrStore> = Mutex::new(AttrStore::new());
static SET_COUNT: AtomicU64 = AtomicU64::new(0);
static GET_COUNT: AtomicU64 = AtomicU64::new(0);
static QUERY_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// ASCII-lowercase a string (no_std friendly).
fn to_ascii_lowercase(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push((c as u8 + 32) as char);
        } else {
            out.push(c);
        }
    }
    out
}

/// Convert an AttrValue to a sortable string key for the index.
fn value_to_key(val: &AttrValue) -> String {
    match val {
        AttrValue::Text(s) => {
            let mut key = String::from("T:");
            key.push_str(&to_ascii_lowercase(s));
            key
        }
        AttrValue::Int(n) => {
            // Encode so negative numbers sort correctly.
            // Map i64::MIN..i64::MAX to 0..u64::MAX.
            let mapped = (*n as u64) ^ (1u64 << 63);
            alloc::format!("I:{:020}", mapped)
        }
        AttrValue::Bool(b) => {
            if *b { String::from("B:1") } else { String::from("B:0") }
        }
        AttrValue::Bytes(b) => {
            let mut key = String::from("X:");
            for byte in b.iter().take(64) {
                key.push_str(&alloc::format!("{:02x}", byte));
            }
            key
        }
    }
}

/// Validate an attribute name.
fn validate_name(name: &str) -> KernelResult<()> {
    if name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if name.len() > MAX_ATTR_NAME_LEN {
        return Err(KernelError::InvalidArgument);
    }
    // Must contain only printable ASCII (and colon for category:name).
    for c in name.chars() {
        if !c.is_ascii_graphic() {
            return Err(KernelError::InvalidArgument);
        }
    }
    Ok(())
}

/// Validate an attribute value.
fn validate_value(val: &AttrValue) -> KernelResult<()> {
    match val {
        AttrValue::Text(s) if s.len() > MAX_TEXT_VALUE_LEN => {
            Err(KernelError::InvalidArgument)
        }
        AttrValue::Bytes(b) if b.len() > MAX_TEXT_VALUE_LEN => {
            Err(KernelError::InvalidArgument)
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Set an attribute on a file.
pub fn set_attr(path: &str, name: &str, value: AttrValue) -> KernelResult<()> {
    validate_name(name)?;
    validate_value(&value)?;
    SET_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut store = STORE.lock();

    let idx = if let Some(&i) = store.path_index.get(path) {
        i
    } else {
        // New file entry.
        if store.path_index.len() >= MAX_FILES {
            return Err(KernelError::ResourceExhausted);
        }
        let idx = if let Some(free) = store.free_slots.pop() {
            store.files[free] = FileAttrs {
                path: String::from(path),
                attrs: BTreeMap::new(),
            };
            free
        } else {
            let i = store.files.len();
            store.files.push(FileAttrs {
                path: String::from(path),
                attrs: BTreeMap::new(),
            });
            i
        };
        store.path_index.insert(String::from(path), idx);
        idx
    };

    // Check capacity before mutating.
    {
        let file = &store.files[idx];
        if !file.attrs.contains_key(name) && file.attrs.len() >= MAX_ATTRS_PER_FILE {
            return Err(KernelError::ResourceExhausted);
        }
    }

    // Update index if this attribute is indexed.
    let attr_name_owned = String::from(name);
    let is_indexed = store.indexed_names.contains(&attr_name_owned);
    if is_indexed {
        // Remove old index entry if value is changing.
        let old_key = store.files[idx].attrs.get(name).map(value_to_key);
        if let Some(ok) = old_key {
            if let Some(val_map) = store.indexes.get_mut(name) {
                if let Some(paths) = val_map.get_mut(&ok) {
                    paths.remove(path);
                    if paths.is_empty() {
                        val_map.remove(&ok);
                    }
                }
            }
        }
        // Insert new index entry.
        let new_key = value_to_key(&value);
        let val_map = store.indexes.entry(attr_name_owned.clone())
            .or_default();
        val_map.entry(new_key)
            .or_default()
            .insert(String::from(path));
    }

    store.files[idx].attrs.insert(attr_name_owned, value);
    Ok(())
}

/// Get an attribute from a file.
pub fn get_attr(path: &str, name: &str) -> KernelResult<AttrValue> {
    GET_COUNT.fetch_add(1, Ordering::Relaxed);
    let store = STORE.lock();
    let idx = store.path_index.get(path).ok_or(KernelError::NotFound)?;
    let file = &store.files[*idx];
    file.attrs.get(name).cloned().ok_or(KernelError::NotFound)
}

/// Remove an attribute from a file.
pub fn remove_attr(path: &str, name: &str) -> KernelResult<()> {
    let mut store = STORE.lock();
    let idx = store.path_index.get(path).ok_or(KernelError::NotFound)?;
    let idx = *idx;

    let removed = store.files[idx].attrs.remove(name);
    if removed.is_none() {
        return Err(KernelError::NotFound);
    }

    // Clean up index.
    if let Some(old_val) = &removed {
        if store.indexed_names.contains(name) {
            let old_key = value_to_key(old_val);
            if let Some(val_map) = store.indexes.get_mut(name) {
                if let Some(paths) = val_map.get_mut(&old_key) {
                    paths.remove(path);
                    if paths.is_empty() {
                        val_map.remove(&old_key);
                    }
                }
            }
        }
    }

    // If file has no more attributes, remove it entirely.
    if store.files[idx].attrs.is_empty() {
        store.path_index.remove(path);
        store.free_slots.push(idx);
    }

    Ok(())
}

/// List all attributes on a file.
pub fn list_attrs(path: &str) -> KernelResult<Vec<(String, AttrValue)>> {
    let store = STORE.lock();
    let idx = store.path_index.get(path).ok_or(KernelError::NotFound)?;
    let file = &store.files[*idx];
    Ok(file.attrs.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

/// Remove all attributes from a file.
pub fn clear_attrs(path: &str) -> KernelResult<usize> {
    let mut store = STORE.lock();
    let idx = store.path_index.get(path).ok_or(KernelError::NotFound)?;
    let idx = *idx;
    let count = store.files[idx].attrs.len();

    // Collect index cleanup info before mutating.
    let to_clean: Vec<(String, String)> = store.files[idx].attrs.iter()
        .filter(|(name, _)| store.indexed_names.contains(name.as_str()))
        .map(|(name, val)| (name.clone(), value_to_key(val)))
        .collect();

    // Clean up indexes.
    for (name, key) in &to_clean {
        if let Some(val_map) = store.indexes.get_mut(name.as_str()) {
            if let Some(paths) = val_map.get_mut(key) {
                paths.remove(path);
                if paths.is_empty() {
                    val_map.remove(key);
                }
            }
        }
    }

    store.files[idx].attrs.clear();
    store.path_index.remove(path);
    store.free_slots.push(idx);
    Ok(count)
}

// ---------------------------------------------------------------------------
// Index management
// ---------------------------------------------------------------------------

/// Mark an attribute name as indexed for fast queries.
///
/// Once indexed, all set/remove operations on this attribute update
/// the reverse index automatically.
pub fn create_index(attr_name: &str) -> KernelResult<()> {
    validate_name(attr_name)?;
    let mut store = STORE.lock();
    if store.indexed_names.len() >= MAX_INDEXED_ATTRS {
        return Err(KernelError::ResourceExhausted);
    }
    let name = String::from(attr_name);
    if store.indexed_names.contains(&name) {
        return Ok(()); // Already indexed.
    }
    store.indexed_names.insert(name.clone());

    // Build index from existing data.
    let mut val_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for file in &store.files {
        if let Some(val) = file.attrs.get(attr_name) {
            let key = value_to_key(val);
            val_map.entry(key)
                .or_default()
                .insert(file.path.clone());
        }
    }
    if !val_map.is_empty() {
        store.indexes.insert(name, val_map);
    }
    Ok(())
}

/// Remove an index on an attribute name.
pub fn drop_index(attr_name: &str) -> KernelResult<()> {
    let mut store = STORE.lock();
    let name = String::from(attr_name);
    if !store.indexed_names.remove(&name) {
        return Err(KernelError::NotFound);
    }
    store.indexes.remove(&name);
    Ok(())
}

/// List all indexed attribute names.
pub fn list_indexes() -> Vec<String> {
    let store = STORE.lock();
    store.indexed_names.iter().cloned().collect()
}

// ---------------------------------------------------------------------------
// Schema management
// ---------------------------------------------------------------------------

/// Register an attribute schema.
pub fn register_schema(name: &str, value_type: &'static str, description: &str) -> KernelResult<()> {
    validate_name(name)?;
    let mut store = STORE.lock();
    let is_indexed = store.indexed_names.contains(name);
    store.schemas.insert(String::from(name), AttrSchema {
        name: String::from(name),
        value_type,
        indexed: is_indexed,
        description: String::from(description),
    });
    Ok(())
}

/// List all registered schemas.
pub fn list_schemas() -> Vec<AttrSchema> {
    let store = STORE.lock();
    store.schemas.values().cloned().collect()
}

// ---------------------------------------------------------------------------
// Query engine
// ---------------------------------------------------------------------------

/// Query files by attribute predicates.
///
/// Returns files where the predicates match according to the given mode
/// (All = AND, Any = OR).  Optionally restricts to files under `root_path`.
pub fn query(
    predicates: &[Predicate],
    mode: QueryMode,
    root_path: Option<&str>,
) -> Vec<QueryResult> {
    QUERY_COUNT.fetch_add(1, Ordering::Relaxed);

    if predicates.is_empty() {
        return Vec::new();
    }

    let store = STORE.lock();
    let mut results = Vec::new();

    for file in &store.files {
        // Skip files not under root_path.  The canonical subtree predicate
        // avoids /tmp matching /tmpfile and tolerates a trailing slash on
        // `root`. See fs::pathutil.
        if let Some(root) = root_path {
            if !crate::fs::pathutil::path_in_subtree(file.path.as_str(), root) {
                continue;
            }
        }

        if file.attrs.is_empty() {
            continue;
        }

        let mut matched_attrs = Vec::new();
        let mut all_match = true;
        let mut any_match = false;

        for pred in predicates {
            if let Some(stored) = file.attrs.get(&pred.attr_name) {
                if pred.matches(stored) {
                    any_match = true;
                    matched_attrs.push((pred.attr_name.clone(), stored.clone()));
                } else {
                    all_match = false;
                }
            } else {
                all_match = false;
            }
        }

        let include = match mode {
            QueryMode::All => all_match,
            QueryMode::Any => any_match,
        };

        if include {
            results.push(QueryResult {
                path: file.path.clone(),
                matched_attrs,
            });
            if results.len() >= MAX_QUERY_RESULTS {
                break;
            }
        }
    }

    results
}

/// Query using the index for a single equality predicate on an indexed attr.
///
/// Falls back to full scan if the attribute is not indexed.
pub fn indexed_query(attr_name: &str, value: &AttrValue) -> Vec<String> {
    QUERY_COUNT.fetch_add(1, Ordering::Relaxed);

    let store = STORE.lock();
    if store.indexed_names.contains(attr_name) {
        let key = value_to_key(value);
        if let Some(val_map) = store.indexes.get(attr_name) {
            if let Some(paths) = val_map.get(&key) {
                return paths.iter().cloned().collect();
            }
        }
        return Vec::new();
    }

    // Fallback: full scan.
    let mut results = Vec::new();
    for file in &store.files {
        if let Some(stored) = file.attrs.get(attr_name) {
            if stored == value {
                results.push(file.path.clone());
                if results.len() >= MAX_QUERY_RESULTS {
                    break;
                }
            }
        }
    }
    results
}

/// Count files that have a given attribute (any value).
pub fn count_with_attr(attr_name: &str) -> usize {
    let store = STORE.lock();
    store.files.iter().filter(|f| f.attrs.contains_key(attr_name)).count()
}

/// Get all unique values for a given attribute name.
pub fn unique_values(attr_name: &str) -> Vec<AttrValue> {
    let store = STORE.lock();
    let mut seen = BTreeSet::new();
    let mut values = Vec::new();
    for file in &store.files {
        if let Some(val) = file.attrs.get(attr_name) {
            let key = value_to_key(val);
            if seen.insert(key) {
                values.push(val.clone());
            }
        }
    }
    values
}

// ---------------------------------------------------------------------------
// Rename support
// ---------------------------------------------------------------------------

/// Update all attributes when a file is renamed/moved.
pub fn rename_path(old_path: &str, new_path: &str) -> KernelResult<()> {
    let mut store = STORE.lock();
    let idx = store.path_index.remove(old_path).ok_or(KernelError::NotFound)?;
    store.path_index.insert(String::from(new_path), idx);
    store.files[idx].path = String::from(new_path);

    // Update all index entries.
    for (attr_name, val) in store.files[idx].attrs.clone() {
        if store.indexed_names.contains(&attr_name) {
            let key = value_to_key(&val);
            if let Some(val_map) = store.indexes.get_mut(&attr_name) {
                if let Some(paths) = val_map.get_mut(&key) {
                    paths.remove(old_path);
                    paths.insert(String::from(new_path));
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (file_count, total_attrs, set_ops, get_ops, query_ops, index_count).
pub fn stats() -> (usize, usize, u64, u64, u64, usize) {
    let store = STORE.lock();
    let file_count = store.path_index.len();
    let total_attrs: usize = store.files.iter()
        .filter(|f| !f.attrs.is_empty())
        .map(|f| f.attrs.len())
        .sum();
    let index_count = store.indexed_names.len();
    (
        file_count,
        total_attrs,
        SET_COUNT.load(Ordering::Relaxed),
        GET_COUNT.load(Ordering::Relaxed),
        QUERY_COUNT.load(Ordering::Relaxed),
        index_count,
    )
}

/// Reset statistics.
pub fn reset_stats() {
    SET_COUNT.store(0, Ordering::Relaxed);
    GET_COUNT.store(0, Ordering::Relaxed);
    QUERY_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data (attributes, indexes, schemas).
pub fn clear_all() {
    let mut store = STORE.lock();
    store.path_index.clear();
    store.files.clear();
    store.free_slots.clear();
    store.schemas.clear();
    store.indexes.clear();
    store.indexed_names.clear();
}

// ---------------------------------------------------------------------------
// Built-in schemas
// ---------------------------------------------------------------------------

/// Register common attribute schemas (Audio, Image, Document, Email).
pub fn register_builtins() -> KernelResult<()> {
    // Audio attributes.
    register_schema("Audio:Artist", "text", "Music artist or band name")?;
    register_schema("Audio:Album", "text", "Album name")?;
    register_schema("Audio:Title", "text", "Track title")?;
    register_schema("Audio:Genre", "text", "Music genre")?;
    register_schema("Audio:Year", "int", "Release year")?;
    register_schema("Audio:Track", "int", "Track number")?;
    register_schema("Audio:Bitrate", "int", "Audio bitrate in kbps")?;
    register_schema("Audio:Duration", "int", "Duration in seconds")?;

    // Image attributes.
    register_schema("Image:Width", "int", "Image width in pixels")?;
    register_schema("Image:Height", "int", "Image height in pixels")?;
    register_schema("Image:ColorSpace", "text", "Color space (sRGB, AdobeRGB, etc.)")?;
    register_schema("Image:Camera", "text", "Camera make and model")?;
    register_schema("Image:DateTaken", "int", "Timestamp when photo was taken")?;

    // Document attributes.
    register_schema("Document:Author", "text", "Document author")?;
    register_schema("Document:Title", "text", "Document title")?;
    register_schema("Document:Subject", "text", "Document subject")?;
    register_schema("Document:PageCount", "int", "Number of pages")?;
    register_schema("Document:WordCount", "int", "Number of words")?;

    // Email attributes.
    register_schema("Email:From", "text", "Sender email address")?;
    register_schema("Email:To", "text", "Recipient email address")?;
    register_schema("Email:Subject", "text", "Email subject line")?;
    register_schema("Email:Date", "int", "Email send timestamp")?;
    register_schema("Email:Read", "bool", "Whether email has been read")?;

    // Generic attributes.
    register_schema("App:Rating", "int", "User rating (1-5)")?;
    register_schema("App:Comment", "text", "User comment")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the queryable metadata module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Save and reset state.
    clear_all();
    reset_stats();

    // Test 1: set and get attributes.
    {
        set_attr("/test/song.mp3", "Audio:Artist", AttrValue::Text(String::from("Beatles")))?;
        set_attr("/test/song.mp3", "Audio:Bitrate", AttrValue::Int(320))?;
        let artist = get_attr("/test/song.mp3", "Audio:Artist")?;
        assert_eq!(artist, AttrValue::Text(String::from("Beatles")));
        let bitrate = get_attr("/test/song.mp3", "Audio:Bitrate")?;
        assert_eq!(bitrate, AttrValue::Int(320));
        serial_println!("[queryable] test 1 passed: set/get attributes");
    }

    // Test 2: list attributes.
    {
        let attrs = list_attrs("/test/song.mp3")?;
        assert_eq!(attrs.len(), 2);
        serial_println!("[queryable] test 2 passed: list attributes");
    }

    // Test 3: query with equality.
    {
        set_attr("/test/song2.mp3", "Audio:Artist", AttrValue::Text(String::from("Beatles")))?;
        set_attr("/test/song3.mp3", "Audio:Artist", AttrValue::Text(String::from("Stones")))?;
        let results = query(
            &[Predicate::eq_text("Audio:Artist", "Beatles")],
            QueryMode::All,
            None,
        );
        assert_eq!(results.len(), 2);
        serial_println!("[queryable] test 3 passed: equality query");
    }

    // Test 4: query with range.
    {
        set_attr("/test/song2.mp3", "Audio:Bitrate", AttrValue::Int(128))?;
        set_attr("/test/song3.mp3", "Audio:Bitrate", AttrValue::Int(256))?;
        let results = query(
            &[Predicate::gt_int("Audio:Bitrate", 200)],
            QueryMode::All,
            None,
        );
        // song.mp3 (320) and song3.mp3 (256) should match.
        assert_eq!(results.len(), 2);
        serial_println!("[queryable] test 4 passed: range query");
    }

    // Test 5: AND query (multiple predicates).
    {
        let results = query(
            &[
                Predicate::eq_text("Audio:Artist", "Beatles"),
                Predicate::gt_int("Audio:Bitrate", 200),
            ],
            QueryMode::All,
            None,
        );
        // Only song.mp3 has Beatles AND bitrate > 200.
        assert_eq!(results.len(), 1);
        assert!(results[0].path.contains("song.mp3") && !results[0].path.contains("song2"));
        serial_println!("[queryable] test 5 passed: AND query");
    }

    // Test 6: indexed query.
    {
        create_index("Audio:Artist")?;
        let paths = indexed_query("Audio:Artist", &AttrValue::Text(String::from("Beatles")));
        assert_eq!(paths.len(), 2);
        serial_println!("[queryable] test 6 passed: indexed query");
    }

    // Test 7: remove and rename.
    {
        remove_attr("/test/song3.mp3", "Audio:Artist")?;
        let results = query(
            &[Predicate::eq_text("Audio:Artist", "Stones")],
            QueryMode::All,
            None,
        );
        assert_eq!(results.len(), 0);

        rename_path("/test/song.mp3", "/music/song.mp3")?;
        let artist = get_attr("/music/song.mp3", "Audio:Artist")?;
        assert_eq!(artist, AttrValue::Text(String::from("Beatles")));
        // Old path should be gone.
        assert!(get_attr("/test/song.mp3", "Audio:Artist").is_err());
        serial_println!("[queryable] test 7 passed: remove/rename");
    }

    // Cleanup.
    clear_all();
    reset_stats();

    serial_println!("[queryable] all 7 self-tests passed");
    Ok(())
}
