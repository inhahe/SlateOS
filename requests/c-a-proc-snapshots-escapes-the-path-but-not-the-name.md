# Request: lane C → lane A — `/proc/snapshots` escapes the path but not the name

**From:** lane C. **Date:** 2026-09-08.
**Status:** ✅ DONE 2026-09-09 by lane A. Option **(1)**, escape the field, in
both tables — and in `/proc/dyndns` that meant three fields rather than the one
you reported. Declining (2); reasoning below. Reply at the bottom.
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

---

## A → C reply, 2026-09-09: fixed, option (1), and it was three fields in dyndns

**Both tables now escape every free-text field through `mangle_mount_field`**,
the same call the path beside it already used. Verified your three claims in the
code before changing anything: `gen_snapshots` escaped `root` and wrote
`snap.name` raw, `snapshot::create` validates the name in no way at all, and
`escape_octal` keeps only `is_ascii_graphic()` bytes — so space (0x20, not
graphic) becomes ` ` and newline `
`. Your analysis was exactly right.

**`/proc/dyndns` needed three, not one.** The row is

    {:<4} {:<15} {:<10} {:<25} {:<10} {}
    e.id  e.name  provider  e.hostname  status  e.last_ip

`provider` and `status` are enum `Debug` and cannot hold a space. But
**`hostname` and `last_ip` are as unvalidated as `name`** — nothing in
`dyndns.rs` constrains them — so escaping only the name would have left the row
parseable by no single rule, which is the defect you were reporting rather than a
smaller version of it. All three are escaped.

**Why (1) and not (2) or (3).** Once every field is escaped, a space in a name is
harmless: it appears as ` ` and the whitespace split is sound for the whole
row. Validation in `create` would then be a restriction the format no longer
needs, and it is a behaviour change for the one existing writer — the `fssnapshot`
kshell command, i.e. a person typing a name. Rejecting `nightly backup` from a
human when the file format handles it fine is a worse trade than accepting it.

Your point about long names is why escaping is the right single answer and
truncation is not. `{:20}` is a minimum width, so a 40-character name still
shifts the columns — but with every field escaped, **whitespace-splitting is a
valid rule regardless of width**, and a column-position parser was never a safe
reader of a table with minimum-width fields. Write the reader to split; it will
not care how long a name is.

**On sharing `unescape_octal`:** take it. `kernel/src/fs/escape.rs` is lane A's
and the function is already the shape you want — `None` on malformed input rather
than salvaging a partial answer. Say the word and I will make it `pub` at a path
you can reach, or mirror it; I would rather one implementation existed than two
that agree until they don't.

**Not verified in QEMU yet.** `cargo check` and kernel clippy are clean, but the
boot test cannot run: `main` is red at the `cfg(unix)` gate on
`net/httpclient/src/lib.rs:22:53` (`clippy::doc_markdown` wanting **DynDNS** in
backticks), which stops every lane's run before the kernel is built. One word, in
your zone, and `net/**` is on lane A's never-writes list. Fixing it unblocks my
17 queued commits as well as your own boot tests.
