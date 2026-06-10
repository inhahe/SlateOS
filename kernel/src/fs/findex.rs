//! File metadata index for queryable attributes.
//!
//! Maintains an in-memory index of file metadata extracted from the
//! filesystem, enabling fast attribute-based queries (BeOS BFS-inspired).
//! The file explorer uses this to populate detail columns, and users can
//! search by any indexed attribute (artist, bitrate, dimensions, etc.).
//!
//! ## Architecture
//!
//! ```text
//! findex::build(root)
//!   → fswalk::walk(root)
//!   → for each file: fileinfo::extract(path) → fields
//!   → insert into in-memory index tables
//!
//! findex::query("audio.artist = Radiohead")
//!   → parse query → scan index → return matching paths
//! ```
//!
//! ## Features
//!
//! - **Attribute index** — maps field names to (path, value) pairs
//! - **Path index** — maps paths to all their fields
//! - **Query language** — simple attribute filters with =, !=, <, >, contains
//! - **Incremental update** — add/remove individual files without full rebuild
//! - **Field statistics** — which fields exist and how many files have them
//! - **Column discovery** — given a directory, determine which columns to show
//!
//! ## Design Notes
//!
//! - Maximum indexed files: 16384 (configurable).
//! - Maximum unique fields tracked: 256.
//! - Index is entirely in-memory; no persistence across reboots.
//! - Build is synchronous; for large trees, consider incremental updates.
//! - This is the kernel-space bootstrap; a userspace daemon can extend it.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::fileinfo::{self, FieldValue, FileInfo};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum indexed files.
const MAX_INDEXED_FILES: usize = 16384;

/// Maximum unique field names tracked.
const MAX_FIELD_NAMES: usize = 256;

/// Maximum query results.
const MAX_QUERY_RESULTS: usize = 1024;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A query comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryOp {
    /// Exact match (=).
    Eq,
    /// Not equal (!=).
    Ne,
    /// Less than (<).
    Lt,
    /// Greater than (>).
    Gt,
    /// Less or equal (<=).
    Le,
    /// Greater or equal (>=).
    Ge,
    /// String contains.
    Contains,
    /// String starts with.
    StartsWith,
    /// String ends with.
    EndsWith,
}

/// A single query predicate.
#[derive(Debug, Clone)]
pub struct QueryPredicate {
    /// Field name to match (e.g. "audio.artist").
    pub field: String,
    /// Comparison operator.
    pub op: QueryOp,
    /// Value to compare against.
    pub value: String,
}

/// An indexed file entry.
#[derive(Debug, Clone)]
struct IndexedFile {
    /// Full file path.
    path: String,
    /// MIME type.
    mime: String,
    /// Indexed fields (name → value).
    fields: Vec<(String, FieldValue)>,
}

/// Field frequency information.
#[derive(Debug, Clone)]
pub struct FieldStat {
    /// Field name.
    pub name: String,
    /// Human-readable label.
    pub label: String,
    /// Number of files that have this field.
    pub count: usize,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// The index: list of indexed files.
static INDEX: spin::Mutex<Vec<IndexedFile>> = spin::Mutex::new(Vec::new());

/// Known field names with labels.
static FIELD_NAMES: spin::Mutex<Vec<(String, String)>> = spin::Mutex::new(Vec::new());

/// Statistics.
static BUILD_COUNT: AtomicU64 = AtomicU64::new(0);
static INDEX_OPS: AtomicU64 = AtomicU64::new(0);
static QUERY_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API — Indexing
// ---------------------------------------------------------------------------

/// Index a single file's metadata.
///
/// Extracts metadata via `fileinfo::extract()` and adds to the index.
/// If the file is already indexed, updates its entry.
pub fn index_file(path: &str) -> KernelResult<usize> {
    INDEX_OPS.fetch_add(1, Ordering::Relaxed);

    let info = fileinfo::extract(path)?;
    let field_count = info.fields.len();

    let mut index = INDEX.lock();

    // Update existing entry.
    for entry in index.iter_mut() {
        if entry.path == path {
            entry.mime = info.mime.clone();
            entry.fields = info.fields.iter()
                .map(|f| (f.name.clone(), f.value.clone()))
                .collect();
            register_fields(&info);
            return Ok(field_count);
        }
    }

    // New entry.
    if index.len() >= MAX_INDEXED_FILES {
        return Err(KernelError::OutOfMemory);
    }

    register_fields(&info);

    index.push(IndexedFile {
        path: String::from(path),
        mime: info.mime.clone(),
        fields: info.fields.iter()
            .map(|f| (f.name.clone(), f.value.clone()))
            .collect(),
    });

    Ok(field_count)
}

/// Remove a file from the index.
pub fn remove_file(path: &str) -> bool {
    let mut index = INDEX.lock();
    let len_before = index.len();
    index.retain(|e| e.path != path);
    index.len() < len_before
}

/// Build/rebuild the index for all files under a directory.
///
/// Uses `fswalk` to enumerate files and `fileinfo` to extract metadata.
/// Returns the number of files indexed.
pub fn build(root: &str, max_depth: usize) -> KernelResult<usize> {
    BUILD_COUNT.fetch_add(1, Ordering::Relaxed);

    let walk_opts = crate::fs::fswalk::WalkOptions {
        max_depth,
        filter: crate::fs::fswalk::WalkFilter::FilesOnly,
        show_hidden: false,
        limit: MAX_INDEXED_FILES,
        ..Default::default()
    };

    let result = crate::fs::fswalk::walk(root, &walk_opts)?;
    let mut count = 0;

    for entry in &result.entries {
        // Try to extract metadata; skip files that fail (unsupported types, etc.).
        if index_file(&entry.path).is_ok() {
            count += 1;
        }
    }

    Ok(count)
}

/// Clear the entire index.
pub fn clear() {
    INDEX.lock().clear();
}

/// Get the number of indexed files.
pub fn count() -> usize {
    INDEX.lock().len()
}

// ---------------------------------------------------------------------------
// Public API — Querying
// ---------------------------------------------------------------------------

/// Query the index with predicates.
///
/// Returns paths of files matching ALL predicates (AND logic).
pub fn query(predicates: &[QueryPredicate]) -> Vec<String> {
    QUERY_COUNT.fetch_add(1, Ordering::Relaxed);

    let index = INDEX.lock();
    let mut results = Vec::new();

    for entry in index.iter() {
        if results.len() >= MAX_QUERY_RESULTS {
            break;
        }

        let matches_all = predicates.iter().all(|pred| {
            entry.fields.iter().any(|(name, value)| {
                name == &pred.field && compare_value(value, pred.op, &pred.value)
            })
        });

        if matches_all {
            results.push(entry.path.clone());
        }
    }

    results
}

/// Query for files that have a specific field, regardless of value.
pub fn query_has_field(field: &str) -> Vec<String> {
    QUERY_COUNT.fetch_add(1, Ordering::Relaxed);

    let index = INDEX.lock();
    index.iter()
        .filter(|e| e.fields.iter().any(|(n, _)| n == field))
        .take(MAX_QUERY_RESULTS)
        .map(|e| e.path.clone())
        .collect()
}

/// Get all field values for a specific file.
pub fn get_fields(path: &str) -> Vec<(String, String)> {
    let index = INDEX.lock();
    index.iter()
        .find(|e| e.path == path)
        .map(|e| {
            e.fields.iter()
                .map(|(name, value)| (name.clone(), value.display()))
                .collect()
        })
        .unwrap_or_default()
}

/// Discover which columns are relevant for a directory.
///
/// Looks at all indexed files under the given path and returns
/// the union of their field names — exactly what the design spec
/// requires for the file explorer detail column selection.
pub fn columns_for_dir(dir_path: &str) -> Vec<FieldStat> {
    let index = INDEX.lock();
    let prefix = if dir_path.ends_with('/') {
        String::from(dir_path)
    } else {
        format!("{}/", dir_path)
    };

    // Count field occurrences among files in this directory.
    let mut field_counts: Vec<(String, String, usize)> = Vec::new();

    for entry in index.iter() {
        // `prefix` always ends in '/' (built above), so a child is any
        // indexed path strictly under it.  The previous inline boundary
        // check `get(prefix.len()) == Some('/')` was wrong for a
        // trailing-slash prefix — it looked one byte past the slash and so
        // matched nothing, making this function always return empty.  See
        // fs::pathutil for the canonical predicate.
        if !crate::fs::pathutil::path_strictly_under(entry.path.as_str(), prefix.as_str()) {
            continue;
        }
        // Only direct children (no subdirectories).
        let rest = &entry.path[prefix.len()..];
        if rest.is_empty() || rest.contains('/') {
            continue;
        }

        for (name, _value) in &entry.fields {
            if let Some(fc) = field_counts.iter_mut().find(|(n, _, _)| n == name) {
                fc.2 += 1;
            } else {
                let label = field_label(name);
                field_counts.push((name.clone(), label, 1));
            }
        }
    }

    // Sort by frequency (most common first).
    field_counts.sort_by_key(|e| core::cmp::Reverse(e.2));

    field_counts.into_iter()
        .map(|(name, label, count)| FieldStat { name, label, count })
        .collect()
}

/// Parse a simple query string into predicates.
///
/// Format: `"field op value"` or `"field op value AND field op value"`.
/// Operators: `=`, `!=`, `<`, `>`, `<=`, `>=`, `~` (contains),
///            `^` (starts with), `$` (ends with).
pub fn parse_query(query_str: &str) -> Vec<QueryPredicate> {
    let mut predicates = Vec::new();

    // Split on " AND " (case-insensitive).
    let parts: Vec<&str> = split_and(query_str);

    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Try each operator (longer operators first to avoid ambiguity).
        let operators: &[(&str, QueryOp)] = &[
            ("!=", QueryOp::Ne),
            ("<=", QueryOp::Le),
            (">=", QueryOp::Ge),
            ("=", QueryOp::Eq),
            ("<", QueryOp::Lt),
            (">", QueryOp::Gt),
            ("~", QueryOp::Contains),
            ("^", QueryOp::StartsWith),
            ("$", QueryOp::EndsWith),
        ];

        let mut found = false;
        for (op_str, op) in operators {
            if let Some(pos) = part.find(op_str) {
                let field = part[..pos].trim();
                let value = part[pos + op_str.len()..].trim();
                if !field.is_empty() {
                    predicates.push(QueryPredicate {
                        field: String::from(field),
                        op: *op,
                        value: String::from(value),
                    });
                    found = true;
                    break;
                }
            }
        }

        if !found {
            // No operator found — treat as field existence check.
            predicates.push(QueryPredicate {
                field: String::from(part),
                op: QueryOp::Ne,
                value: String::new(), // "field != empty" = "field exists with any value"
            });
        }
    }

    predicates
}

// ---------------------------------------------------------------------------
// Public API — Statistics
// ---------------------------------------------------------------------------

/// Get index statistics.
pub fn stats() -> (u64, u64, u64, usize, usize) {
    let index_count = INDEX.lock().len();
    let field_count = FIELD_NAMES.lock().len();
    (
        BUILD_COUNT.load(Ordering::Relaxed),
        INDEX_OPS.load(Ordering::Relaxed),
        QUERY_COUNT.load(Ordering::Relaxed),
        index_count,
        field_count,
    )
}

/// Reset statistics.
pub fn reset_stats() {
    BUILD_COUNT.store(0, Ordering::Relaxed);
    INDEX_OPS.store(0, Ordering::Relaxed);
    QUERY_COUNT.store(0, Ordering::Relaxed);
}

/// List all known field names in the index.
pub fn known_fields() -> Vec<(String, String)> {
    FIELD_NAMES.lock().clone()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Register fields from an extracted FileInfo into the field names table.
fn register_fields(info: &FileInfo) {
    let mut names = FIELD_NAMES.lock();
    for field in &info.fields {
        if names.len() >= MAX_FIELD_NAMES {
            break;
        }
        if !names.iter().any(|(n, _)| n == &field.name) {
            names.push((field.name.clone(), field.label.clone()));
        }
    }
}

/// Look up a human-readable label for a field name.
fn field_label(name: &str) -> String {
    let names = FIELD_NAMES.lock();
    names.iter()
        .find(|(n, _)| n == name)
        .map(|(_, l)| l.clone())
        .unwrap_or_else(|| String::from(name))
}

/// Compare a field value against a query value.
fn compare_value(field_val: &FieldValue, op: QueryOp, query_val: &str) -> bool {
    match field_val {
        FieldValue::Text(s) => compare_text(s, op, query_val),
        FieldValue::Int(n) => {
            if let Ok(qn) = query_val.parse::<i64>() {
                compare_int(*n, op, qn)
            } else {
                compare_text(&format!("{}", n), op, query_val)
            }
        }
        FieldValue::Uint(n) => {
            if let Ok(qn) = query_val.parse::<u64>() {
                compare_uint(*n, op, qn)
            } else {
                compare_text(&format!("{}", n), op, query_val)
            }
        }
        FieldValue::Float(f) => {
            if let Ok(qf) = parse_f64(query_val) {
                compare_float(*f, op, qf)
            } else {
                compare_text(&format!("{:.2}", f), op, query_val)
            }
        }
        FieldValue::Bool(b) => {
            let query_bool = matches!(query_val, "true" | "yes" | "1");
            match op {
                QueryOp::Eq => *b == query_bool,
                QueryOp::Ne => *b != query_bool,
                _ => false,
            }
        }
    }
}

fn compare_text(a: &str, op: QueryOp, b: &str) -> bool {
    match op {
        QueryOp::Eq => a == b,
        QueryOp::Ne => a != b,
        QueryOp::Lt => a < b,
        QueryOp::Gt => a > b,
        QueryOp::Le => a <= b,
        QueryOp::Ge => a >= b,
        QueryOp::Contains => a.contains(b),
        QueryOp::StartsWith => a.starts_with(b),
        QueryOp::EndsWith => a.ends_with(b),
    }
}

fn compare_int(a: i64, op: QueryOp, b: i64) -> bool {
    match op {
        QueryOp::Eq => a == b,
        QueryOp::Ne => a != b,
        QueryOp::Lt => a < b,
        QueryOp::Gt => a > b,
        QueryOp::Le => a <= b,
        QueryOp::Ge => a >= b,
        QueryOp::Contains | QueryOp::StartsWith | QueryOp::EndsWith => a == b,
    }
}

fn compare_uint(a: u64, op: QueryOp, b: u64) -> bool {
    match op {
        QueryOp::Eq => a == b,
        QueryOp::Ne => a != b,
        QueryOp::Lt => a < b,
        QueryOp::Gt => a > b,
        QueryOp::Le => a <= b,
        QueryOp::Ge => a >= b,
        QueryOp::Contains | QueryOp::StartsWith | QueryOp::EndsWith => a == b,
    }
}

fn compare_float(a: f64, op: QueryOp, b: f64) -> bool {
    match op {
        QueryOp::Eq => (a - b).abs() < 0.001,
        QueryOp::Ne => (a - b).abs() >= 0.001,
        QueryOp::Lt => a < b,
        QueryOp::Gt => a > b,
        QueryOp::Le => a <= b,
        QueryOp::Ge => a >= b,
        QueryOp::Contains | QueryOp::StartsWith | QueryOp::EndsWith => (a - b).abs() < 0.001,
    }
}

/// Minimal f64 parser (no std float parsing in no_std).
fn parse_f64(s: &str) -> Result<f64, ()> {
    let s = s.trim();
    if s.is_empty() { return Err(()); }

    let (negative, s) = if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else {
        (false, s)
    };

    let mut result: f64 = 0.0;
    let mut decimal = false;
    let mut decimal_factor: f64 = 0.1;
    let mut has_digits = false;

    for b in s.bytes() {
        match b {
            b'0'..=b'9' => {
                has_digits = true;
                let digit = (b - b'0') as f64;
                if decimal {
                    result += digit * decimal_factor;
                    decimal_factor *= 0.1;
                } else {
                    result = result * 10.0 + digit;
                }
            }
            b'.' => {
                if decimal { return Err(()); }
                decimal = true;
            }
            _ => return Err(()),
        }
    }

    if !has_digits { return Err(()); }
    if negative { result = -result; }
    Ok(result)
}

/// Split a query string on " AND " (case-insensitive).
fn split_and(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let lower = s.to_lowercase();
    let bytes = lower.as_bytes();
    let pattern = b" and ";
    let mut start = 0;

    let mut i = 0;
    while i + pattern.len() <= bytes.len() {
        if &bytes[i..i + pattern.len()] == pattern {
            parts.push(s[start..i].trim());
            start = i + pattern.len();
            i = start;
        } else {
            i += 1;
        }
    }
    parts.push(s[start..].trim());
    parts
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[findex] Running self-test...");

    test_parse_query();
    test_compare_value();
    test_split_and();
    test_parse_f64();
    test_query_predicate();
    test_field_label();

    serial_println!("[findex] Self-test passed (6 tests).");
    Ok(())
}

fn test_parse_query() {
    let preds = parse_query("audio.artist=Radiohead");
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].field, "audio.artist");
    assert_eq!(preds[0].op, QueryOp::Eq);
    assert_eq!(preds[0].value, "Radiohead");

    let preds = parse_query("audio.bitrate_kbps > 192");
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].op, QueryOp::Gt);

    let preds = parse_query("image.width >= 1920 AND image.height >= 1080");
    assert_eq!(preds.len(), 2);

    serial_println!("[findex]   parse_query: ok");
}

fn test_compare_value() {
    // Text equality.
    assert!(compare_value(&FieldValue::Text(String::from("hello")), QueryOp::Eq, "hello"));
    assert!(!compare_value(&FieldValue::Text(String::from("hello")), QueryOp::Eq, "world"));

    // Text contains.
    assert!(compare_value(&FieldValue::Text(String::from("hello world")), QueryOp::Contains, "world"));

    // Uint comparison.
    assert!(compare_value(&FieldValue::Uint(320), QueryOp::Gt, "192"));
    assert!(!compare_value(&FieldValue::Uint(128), QueryOp::Gt, "192"));
    assert!(compare_value(&FieldValue::Uint(1920), QueryOp::Ge, "1920"));

    // Bool comparison.
    assert!(compare_value(&FieldValue::Bool(true), QueryOp::Eq, "yes"));
    assert!(!compare_value(&FieldValue::Bool(false), QueryOp::Eq, "true"));

    serial_println!("[findex]   compare_value: ok");
}

fn test_split_and() {
    let parts = split_and("a = 1 AND b = 2");
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0], "a = 1");
    assert_eq!(parts[1], "b = 2");

    let parts = split_and("single");
    assert_eq!(parts.len(), 1);

    // Case-insensitive.
    let parts = split_and("x = 1 and y = 2 AND z = 3");
    assert_eq!(parts.len(), 3);

    serial_println!("[findex]   split_and: ok");
}

fn test_parse_f64() {
    assert!((parse_f64("3.25").unwrap() - 3.25).abs() < 0.001);
    assert!((parse_f64("-2.5").unwrap() + 2.5).abs() < 0.001);
    assert!((parse_f64("42").unwrap() - 42.0).abs() < 0.001);
    assert!(parse_f64("").is_err());
    assert!(parse_f64("abc").is_err());

    serial_println!("[findex]   parse_f64: ok");
}

fn test_query_predicate() {
    // Test all operator parsing.
    let preds = parse_query("a != b");
    assert_eq!(preds[0].op, QueryOp::Ne);

    let preds = parse_query("a <= 10");
    assert_eq!(preds[0].op, QueryOp::Le);

    let preds = parse_query("a >= 10");
    assert_eq!(preds[0].op, QueryOp::Ge);

    let preds = parse_query("a ~ hello");
    assert_eq!(preds[0].op, QueryOp::Contains);

    let preds = parse_query("a ^ prefix");
    assert_eq!(preds[0].op, QueryOp::StartsWith);

    let preds = parse_query("a $ suffix");
    assert_eq!(preds[0].op, QueryOp::EndsWith);

    serial_println!("[findex]   query_predicate: ok");
}

fn test_field_label() {
    // Register a field and look it up.
    {
        let mut names = FIELD_NAMES.lock();
        names.push((String::from("test.field"), String::from("Test Field")));
    }
    assert_eq!(field_label("test.field"), "Test Field");
    assert_eq!(field_label("unknown.field"), "unknown.field");

    // Clean up.
    {
        let mut names = FIELD_NAMES.lock();
        names.retain(|(n, _)| n != "test.field");
    }

    serial_println!("[findex]   field_label: ok");
}
