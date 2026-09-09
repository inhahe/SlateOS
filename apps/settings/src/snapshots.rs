//! The system snapshots page: what the kernel has, not a model of it.
//!
//! # What this file used to be
//!
//! 2,256 lines: `SnapshotId`, `BlockHash`, `SnapshotIncludes`,
//! `SnapshotManager`, a copy-on-write block store, snapshot trees, retention
//! policies, update tracking, and a page renderer for all of it. None of it
//! was reachable -- `main.rs` referred to this module zero times -- and
//! `#![allow(dead_code)]` at the top meant the compiler never said so.
//!
//! It was also redundant. `fs::snapshot` in the kernel does exactly this
//! ("CAS-backed point-in-time directory tree snapshots with
//! create/restore/delete/diff/list; branching (parent->child tree), selective
//! include/exclude filters, metadata preservation", `roadmap.md` line 2434),
//! has done for some time, and publishes the result at `/proc/snapshots`.
//! Meanwhile the page the user could actually reach rendered four invented
//! rows.
//!
//! So the model is gone and this file is the reader: parse what the kernel
//! reports, and let the page draw that. Removing the `allow` first was what
//! made the deletion safe -- the compiler named all 41 unreachable items
//! rather than anyone deciding by eye which were needed.
//!
//! Full history in `known-issues.md`
//! `TD-C-THREE-SETTINGS-PAGES-ARE-BUILT-AND-REACHED-BY-NOTHING`.

// ============================================================================
// Reading the snapshots the system actually has
// ============================================================================

/// One snapshot, as `/proc/snapshots` reports it.
///
/// This is the *kernel's* view. Everything above in this file is a userspace
/// model of the same idea, written before anyone noticed `fs::snapshot` was
/// already finished; see `known-issues.md`
/// `TD-C-THREE-SETTINGS-PAGES-ARE-BUILT-AND-REACHED-BY-NOTHING`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRow {
    /// The kernel's id for this snapshot.
    pub id: u64,
    /// Its name, as whoever created it gave it.
    pub name: String,
    /// The directory it was taken of.
    ///
    /// **Bytes, not a `String`.** Our paths allow every byte except `/` and
    /// NUL, so a path is not text — `CLAUDE.md` item 7. The wire form is
    /// octal-escaped ASCII precisely so it can carry those bytes through a
    /// text table, and turning it into a `String` here would undo that with a
    /// lossy conversion, which is silent data corruption in the one field
    /// where a wrong answer names a different directory.
    pub path: Vec<u8>,
    /// How many files it covers.
    pub files: u64,
    /// How many bytes it covers.
    pub bytes: u64,
    /// The snapshot this one branched from, if any.
    pub parent: Option<u64>,
}

/// Where the kernel publishes the snapshot list.
pub const PROC_SNAPSHOTS: &str = "/proc/snapshots";

/// Undo `escape_octal`: `\NNN` back to the byte it names.
///
/// Returns `None` for input no escaper could have produced — a trailing
/// backslash, or a backslash not followed by exactly three octal digits.
/// Refusing is deliberate and matches the kernel's own inverse: a malformed
/// record is a corrupt record, and salvaging part of it hands the caller a
/// path that names the wrong directory.
fn unescape_octal(s: &str) -> Option<Vec<u8>> {
    let raw = s.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let b = *raw.get(i)?;
        if b != b'\\' {
            out.push(b);
            i = i.checked_add(1)?;
            continue;
        }
        let digits = raw.get(i.checked_add(1)?..i.checked_add(4)?)?;
        let mut value: u16 = 0;
        for d in digits {
            if !(b'0'..=b'7').contains(d) {
                return None;
            }
            // `d - b'0'` is guarded by the range check above, but written as a
            // checked subtraction anyway so the whole expression is one rule
            // rather than "these two are checked and that one is argued".
            let digit = d.checked_sub(b'0')?;
            value = value.checked_mul(8)?.checked_add(u16::from(digit))?;
        }
        out.push(u8::try_from(value).ok()?);
        i = i.checked_add(4)?;
    }
    Some(out)
}

/// Parse the body of `/proc/snapshots`.
///
/// # Why this parses from both ends rather than by column
///
/// The table is written with `{:>4}  {:20}  {:30}  {:>8}  {:>12}  {}`, which
/// looks like fixed columns and is not one: Rust's width is a *minimum*, so a
/// name longer than twenty characters shifts every field after it. And the
/// name is emitted **unescaped**, so a name containing a space splits one
/// field into two — while the path beside it *is* escaped, space included.
/// Reported to lane A as
/// `requests/c-a-proc-snapshots-escapes-the-path-but-not-the-name.md`.
///
/// So neither column positions nor a plain whitespace split is safe. What *is*
/// safe is that the fields at both ends cannot contain a space: the id is a
/// number, the last three are two numbers and an id-or-`-`, and the path is
/// escaped. So this anchors from both ends and treats everything left over as
/// the name — which reads a spaced or over-long name correctly today, and will
/// keep working unchanged if lane A escapes the name too.
///
/// A line that does not fit that shape is skipped rather than failing the
/// whole read: one corrupt row should not blank a settings page.
#[must_use]
pub fn parse_proc_snapshots(text: &str) -> Vec<SnapshotRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        // The count header, the blank, and the column header.
        if line.is_empty() || line.starts_with("Filesystem snapshots:") {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        // id, >=1 name word, path, files, bytes, parent.
        if fields.len() < 6 {
            continue;
        }
        // The column header has the right field count and no numbers.
        let Some(id) = fields.first().and_then(|f| f.parse::<u64>().ok()) else {
            continue;
        };
        let tail = fields.len().saturating_sub(3);
        let (Some(files), Some(bytes)) = (
            fields.get(tail).and_then(|f| f.parse::<u64>().ok()),
            fields
                .get(tail.saturating_add(1))
                .and_then(|f| f.parse::<u64>().ok()),
        ) else {
            continue;
        };
        let parent = match fields.get(tail.saturating_add(2)) {
            Some(&"-") => None,
            Some(p) => match p.parse::<u64>() {
                Ok(n) => Some(n),
                Err(_) => continue,
            },
            None => continue,
        };
        let path_at = tail.saturating_sub(1);
        let Some(path) = fields.get(path_at).and_then(|p| unescape_octal(p)) else {
            continue;
        };
        // Everything between the id and the path is the name. Rejoined with
        // single spaces: a name with a run of them is displayed with one, which
        // is a display nicety rather than a correctness claim — this field is
        // shown to a person and never used to name anything.
        let name = fields
            .get(1..path_at)
            .map(|words| words.join(" "))
            .unwrap_or_default();
        rows.push(SnapshotRow {
            id,
            name,
            path,
            files,
            bytes,
            parent,
        });
    }
    rows
}

/// The snapshots this machine has, or an empty list if the kernel does not
/// publish them.
///
/// Empty rather than an error: a settings page that cannot read `/proc` should
/// say "no snapshots", not refuse to draw. The distinction that would matter —
/// "none exist" against "could not ask" — is not one this file can act on
/// differently, so it is not invented.
#[must_use]
pub fn system_snapshots() -> Vec<SnapshotRow> {
    std::fs::read_to_string(PROC_SNAPSHOTS)
        .map(|text| parse_proc_snapshots(&text))
        .unwrap_or_default()
}

/// Format a byte count as a human-readable string (KiB, MiB, GiB).
pub fn format_size(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test that indexes out of range should fail loudly and point at the line that did it"
)]
mod tests {
    use super::*;

    const PROC_SAMPLE: &str = "Filesystem snapshots: 2\n\
         \n\
         \x20 ID  NAME                  PATH                               FILES         BYTES  PARENT\n\
         \x20  1  nightly               /home/alice                          412       1048576  -\n\
         \x20  2  after-update          /home/alice                          413       1048600  1\n";

    #[test]
    fn format_size_display() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.0 GiB");
    }

    #[test]
    fn a_normal_table_parses() {
        let rows = parse_proc_snapshots(PROC_SAMPLE);
        assert_eq!(rows.len(), 2, "header lines were taken for snapshots");
        assert_eq!(rows[0].id, 1);
        assert_eq!(rows[0].name, "nightly");
        assert_eq!(rows[0].path, b"/home/alice");
        assert_eq!(rows[0].files, 412);
        assert_eq!(rows[0].bytes, 1_048_576);
        assert_eq!(rows[0].parent, None, "`-` is no parent, not parent zero");
        assert_eq!(rows[1].parent, Some(1));
    }

    /// The path is octal-escaped by the kernel, so it can carry bytes a text
    /// table could not otherwise hold — a space, a newline, a non-UTF-8 byte.
    /// Un-escaping it is this reader's job, and it must produce *bytes*.
    #[test]
    fn an_escaped_path_comes_back_as_the_bytes_it_names() {
        let line = "   7  weird  /home/a\\040b\\012c\\377d  1  2  -\n";
        let rows = parse_proc_snapshots(line);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].path, b"/home/a b\nc\xffd",
            "the escape was not undone, or was undone through a String"
        );
    }

    /// A name with a space in it. The kernel escapes the *path* and not the
    /// name, so this row has seven whitespace-separated fields where the
    /// format promises six.
    ///
    /// Reported as `requests/c-a-proc-snapshots-escapes-the-path-but-not-the-
    /// name.md`. Until it changes, this reader has to cope — and it does,
    /// because it anchors on the fields at both ends, none of which can
    /// contain a space.
    #[test]
    fn a_name_containing_a_space_is_still_read_correctly() {
        let line = "   3  nightly backup  /var/pkg  9  99  -\n";
        let rows = parse_proc_snapshots(line);
        assert_eq!(rows.len(), 1, "the row was rejected for having a space");
        assert_eq!(rows[0].name, "nightly backup");
        assert_eq!(rows[0].path, b"/var/pkg");
        assert_eq!(rows[0].files, 9);
        assert_eq!(rows[0].bytes, 99);
    }

    /// And a name longer than the column, which shifts every field after it
    /// because `{:20}` is a *minimum* width in Rust and does not truncate. A
    /// column-position parser reads the wrong bytes for the whole row; this
    /// one does not use column positions.
    #[test]
    fn a_name_longer_than_its_column_does_not_shift_the_other_fields() {
        let line = "   4  a-very-long-snapshot-name-indeed  /etc  1  2  -\n";
        let rows = parse_proc_snapshots(line);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "a-very-long-snapshot-name-indeed");
        assert_eq!(rows[0].path, b"/etc");
        assert_eq!(rows[0].files, 1);
    }

    /// A corrupt row is skipped, not fatal: one bad line should not blank the
    /// whole settings page.
    #[test]
    fn a_malformed_row_is_skipped_and_the_rest_survive() {
        let text = "   1  ok  /a  1  2  -\n\
                    this line is not a snapshot at all\n\
                    \x20  2  fine  /b  3  4  1\n";
        let rows = parse_proc_snapshots(text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "ok");
        assert_eq!(rows[1].name, "fine");
    }

    /// A backslash escape that no escaper could have produced is refused
    /// rather than salvaged — the same rule the kernel's own inverse follows.
    /// Half a path names a different directory.
    #[test]
    fn a_bad_escape_rejects_the_row_rather_than_guessing() {
        assert!(parse_proc_snapshots("   1  x  /a\\09  1  2  -\n").is_empty());
        assert!(parse_proc_snapshots("   1  x  /a\\  1  2  -\n").is_empty());
        assert!(parse_proc_snapshots("   1  x  /a\\778  1  2  -\n").is_empty());
    }

    /// The empty case: a kernel with no snapshots writes a header and nothing
    /// else, and that is zero rows rather than one malformed one.
    #[test]
    fn no_snapshots_is_no_rows() {
        assert!(parse_proc_snapshots("Filesystem snapshots: 0\n\n").is_empty());
    }
}
