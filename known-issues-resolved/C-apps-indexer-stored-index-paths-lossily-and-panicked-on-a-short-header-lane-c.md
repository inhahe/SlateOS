## `apps/indexer` stored index paths lossily and panicked on a short header (lane C)

**Status: FIXED 2026-08-15** (lane C), commit below. Third instance of the
lossy-path class, found by continuing the sweep. The index is a binary,
length-prefixed format, so unlike `meta.txt` and the backup manifest there was
never a readability tradeoff to weigh — it simply stored the wrong bytes:

- `serialize` wrote `entry.path.to_string_lossy().as_bytes()` and
  `deserialize` read them back with `String::from_utf8_lossy`. A file whose
  name is not UTF-8 was indexed under a name containing U+FFFD, so the search
  hit that named it could not be opened. Both sides now carry
  `OsStr::as_encoded_bytes` verbatim; `INDEX_VERSION` goes 1 → 2. No migration
  is needed — the index is a derived cache and the existing version check
  already tells the user to reindex.
- **Panic on a truncated index.** The header check was `data.len() < 28`, but
  `dirs_scanned` is read from bytes `24..32`, so a file of 28..=31 bytes
  passed the check and then indexed out of bounds. The existing
  `test_index_deserialize_too_short` used a 4-byte input and never reached it.
  Now `< INDEX_HEADER_LEN` (32), with a test that sweeps every length below it.

Two smaller things fixed in passing: the two scanners each carried a verbatim
copy of the directory-exclusion check (now one `is_excluded_dir`), and each
copy tested `dir_str.ends_with(excl) || dir_str.contains(excl)` — the same
predicate written twice, since `contains` is true whenever `ends_with` is.

The `filename` field stays a lossy `String`, now documented as a **search key
only**: a query is UTF-8 text the user typed, so matching against a lossy
rendering is a selection heuristic. It is never displayed and never used to
name a file — `path` is, and `path` is exact. Both producers of the key now go
through one `filename_key` function so they cannot drift.
