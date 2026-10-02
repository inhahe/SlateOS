### TD3. Prefix-boundary subtree checks: audit every site for trailing-slash correctness — RESOLVED 2026-06-10

**What:** The "is `path` inside directory subtree `prefix`" check was
written inline at ~30 sites as
`path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/')`
(sometimes with a leading `path == prefix ||`).  This idiom is **only
correct when `prefix` has no trailing slash**.  When `prefix` already
ends in `/` (e.g. a registration like `"/protected/"`), the
`get(prefix.len()) == Some(&b'/')` boundary check looks one byte past
the slash and therefore only matches *double-slash* paths
(`/protected//x`), so real children never match — the check silently
fails (open for deny handlers, or simply never fires for "missing file"
/ exclusion logic).

**RESOLUTION (2026-06-10):** Created a single canonical helper module
`kernel/src/fs/pathutil.rs` exposing `path_in_subtree(path, dir)` and
`path_strictly_under(path, dir)`.  Both normalise away an optional
trailing slash (`dir.strip_suffix('/')`) before the component-boundary
check, so they are correct whether or not the caller's prefix carries a
trailing slash.  Five `#[cfg(test)]` unit tests pin the contract
(basic boundary, trailing-slash equivalence, empty/root-matches-all,
strictly-under-excludes-self, strictly-under-root).  Every real subtree
check now routes through this helper; the footgun idiom is gone from the
fs subsystem.

**Confirmed-buggy (silent failures), now fixed via the helper:**
- `integrity.rs` baseline-paths filter (earlier commit `22a8098f`) —
  prefix carried a trailing slash; `verify_dir` never reported missing
  files.  Now also routed through `path_in_subtree` (removed the
  per-iteration `format!("{excl}/")` allocation in the exclude-dir scan).
- `intercept.rs` `pre_check` interceptor filter — prefixes registered
  with trailing slashes (`/protected/`) so every deny handler failed
  open.  `path_matches_prefix()` is now a thin `#[inline]` wrapper over
  `path_in_subtree` (kept for the descriptive call-site name + bug note).
- `findex.rs:304` `columns_for_dir` — built `prefix` *with* a trailing
  slash, so the old boundary check matched nothing and column discovery
  always returned empty.  Now routed through `path_strictly_under`.

**Routed through the helper for robustness (prefix-source could carry a
trailing slash; uniform now):** `undelete.rs` (scan filter), `search.rs`
(exclude prefixes), `queryable.rs` (root filter), `dedup.rs` (exclude
prefixes), `directio.rs` (`is_dio_path`), `index.rs` (exclude/remove/
is_watched ×3), `fswalk.rs` (`is_excluded`, both default + opts),
`fcomment.rs` (search/list/remove_under ×3), `changetrack.rs` (path +
old_path prefix filter), `fileversion.rs` (policy + max-size lookups).

**Verified correct, left as-is (slash-free prefixes by construction):**
`vfs.rs` (mount paths), `freeze.rs:264` (mountpoint), `atime.rs:163`
(mount_path), `overlay.rs:169` (already-normalised `is_under`),
`notify.rs` `path_matches` (distinct `strip_prefix` impl with
recursive/non-recursive semantics the helper does not model),
`apps/defrag/src/main.rs:659` (`/*` glob with the slash already stripped;
separate crate, cannot reach `fs::pathutil`).

Build clean; QEMU boot test green.

**Kernel-wide sweep (2026-06-10):** grepped all of `kernel/src` for the
`get(X.len()) == Some(&b'/')` idiom — the only matches are the six
`fs/` files already accounted for above (plus `pathutil.rs`, the helper
itself).  No sibling instances exist in `net`, `proc`, `ipc`, `mm`, or
any other subsystem, so the footgun is fully contained and closed.
