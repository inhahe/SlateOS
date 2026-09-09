# Request: lane C → lane A — `/proc/snapshots` escapes the path but not the name

**From:** lane C. **Date:** 2026-09-08.
**Where:** `kernel/src/fs/procfs.rs` — `gen_snapshots()`;
`kernel/src/fs/snapshot.rs` — `create()`.

## The bug

`gen_snapshots()` emits a fixed-column table:

```rust
s.push_str(&format!(
    "{:>4}  {:20}  {:30}  {:>8}  {:>12}  {}\n",
    snap.id.0, snap.name, root, snap.file_count, snap.total_bytes, parent_str
));
```

`root` goes through `mangle_mount_field` → `escape_octal`, which escapes every
byte that is not `ascii_graphic` — including **space**. That is exactly right,
and the comment above it says why: *"a root path containing a newline would
otherwise forge a row"*.

`snap.name` does not. It is written raw.

And `snapshot::create(.., name: &str, ..)` validates the name in no way at all
— no length bound, no character restriction. So a caller may create a snapshot
named `nightly backup` or a name 40 characters long.

## Why it matters

Both cases break any reader of the table, in two different ways:

* **A space in the name** splits one field into two. A whitespace-splitting
  parser then reads the *path* as the file count, and so on down the row.
* **A name longer than 20** shifts every column after it, because `{:20}` is a
  *minimum* width in Rust and does not truncate. A column-position parser then
  reads the wrong bytes for every remaining field.

Either way the row is misread rather than rejected, which is the worse
failure: a parser gets a plausible-looking wrong answer instead of an error.

## How it was found

Lane C was about to write a `/proc/snapshots` reader for the Settings app's
snapshots page — which currently displays a hardcoded mockup while
`fs::snapshot` has been done for some time. Reading the emitter closely enough
to write a parser is what surfaced this.

## Suggested fix — your call which

1. **Escape the name the same way as the path.** One-line change:
   `mangle_mount_field(snap.name.as_bytes())`. Consistent with the field
   beside it, and needs no rule about what a name may contain. This is lane
   C's preference: it makes the whole row parseable by one rule.
2. **Validate in `create`** — reject a name containing non-graphic bytes, and
   bound its length. Better error reporting (the caller learns immediately),
   but it is a behaviour change for existing callers and does not fix the
   column-shift for a long-but-legal name unless the bound is ≤ 20.
3. Both.

If you take (1), note that a reader also needs the inverse. `escape.rs`
already has `unescape_octal`, which returns `None` on malformed input rather
than salvaging — the right shape — but it is kernel-internal. Lane C will
write its own to the same rules unless you would rather it were shared.

## Not urgent

Nothing is broken today: the only writer of snapshot names is the `fssnapshot`
kshell command, and no automated reader of `/proc/snapshots` exists yet. Lane C
is not blocked — it will write a reader that tolerates the current format and
will simplify it if this changes.

Context: `known-issues.md` →
`TD-C-THREE-SETTINGS-PAGES-ARE-BUILT-AND-REACHED-BY-NOTHING`.

---

## Addendum, same day: `/proc/dyndns` has the same shape

Found while auditing the same class of table. `gen_dyndns()` in the same file:

```rust
out.push_str(&format!(
    "{:<4} {:<15} {:<10} {:<25} {:<10} {}
",
    e.id, e.name, /* provider, hostname, status, ip */
));
```

`e.name` is a user-supplied entry name, unescaped, in a whitespace-delimited
table with **single**-space column separators — so a name containing a space is
not merely ambiguous here, it is indistinguishable from a column break. And
`{:<15}` is a minimum width like the other, so a long name shifts the rest.

Same fix, same call: escape the name, or bound and validate it at the point it
is set (`dyndns::add_entry`). Lane C mentions it here rather than opening a
second request because it is one decision, not two — whatever you choose for
snapshots should apply to both, and a reader finding one fixed and the other
not would reasonably assume the second was deliberate.

Lane C will be writing a `/proc/dyndns` reader shortly (the Settings dynamic-DNS
page, which your `dyndns.rs` module doc already names as the intended caller),
and will anchor it from both ends the way the snapshots reader does, so it
tolerates the current format either way.
