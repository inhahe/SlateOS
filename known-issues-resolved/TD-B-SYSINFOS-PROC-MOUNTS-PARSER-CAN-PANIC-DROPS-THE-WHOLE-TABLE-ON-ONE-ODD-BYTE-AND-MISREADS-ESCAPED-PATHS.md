## TD-B-SYSINFOS-PROC-MOUNTS-PARSER-CAN-PANIC-DROPS-THE-WHOLE-TABLE-ON-ONE-ODD-BYTE-AND-MISREADS-ESCAPED-PATHS

**Filed:** 2026-09-04 by lane B. **Where:** `userspace/sysinfo/src/main.rs`,
`show_disk()` (lines 176-200) and `read_proc()` (line 30).

**Status: FIXED — closed 2026-09-16, and it had been fixed for some time.** The
condition this entry parked on was "to be fixed as part of the `procinfo`
extraction requested by lane C". That extraction landed: `userspace/sysinfo`
depends on `procinfo`, and `show_disk` reads `proc.mounts()`. Nothing re-read
this header afterwards, so it went on advertising work that no longer existed
-- the same shape as the "look elsewhere" sentence in
`B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`, which stood for five days after
its question was answered.

All four checked individually rather than inferred from the dependency:

| defect | state |
|---|---|
| 1. `&parts[3][..20]` byte-slice panic | **gone.** The only surviving mention is the historical note at `main.rs:284` explaining what it used to do. |
| 2. escaped paths misread | **gone.** `procinfo::unescape_octal` undoes ` `, `	`, `
` and `\`, so `/mnt/my backup` no longer displays or compares as `/mnt/my backup`. |
| 3. whole table lost behind "(mount info not available)" | **gone.** `status.take("/proc/mounts", proc.mounts())` reports the individual read rather than collapsing the table. |
| 4. clippy never saw any of it | **gone.** `userspace/sysinfo/Cargo.toml` now carries `[lints] workspace = true`, so `indexing_slicing` reaches it -- which is what would have caught defect 1 as a lint rather than as a crash. |

Item 4 is the one worth keeping in mind: the panic was a `clippy::indexing_slicing`
finding that clippy had never been pointed at. The fix that matters long-term
is not the slice, it is the crate joining the lint policy.

**In short:** `sysinfo disk` — and the no-argument summary, which calls it —
reads `/proc/mounts` in three ways that are each wrong for a file whose fields
are *paths*. One of them aborts the program, one silently prints the wrong
filesystem type for a row, and one makes the entire table disappear behind
`(mount info not available)`. None can be triggered by a plain mount today,
which is why all three have survived: they need a mount point or an option
string that is not plain ASCII without spaces.

### 1. A reachable panic on a mount option string

```rust
let options = if parts[3].len() > 20 {
    &parts[3][..20]
} else {
    parts[3]
};
```

`parts[3]` is a `&str`; `.len()` counts **bytes** and `&s[..20]` panics unless
byte 20 is a character boundary. Verified by construction: any option string
whose first 15 bytes are ASCII followed by two-byte characters puts a
continuation byte (`0xa9`) at offset 20, and the slice panics rather than
truncating. The program aborts mid-table, so the rows already printed stay on
screen and the rest are simply gone — which reads like a short mount table, not
like a crash, if stderr is not being watched.

This is a `clippy::indexing_slicing` finding, and clippy has never seen it:
`userspace/sysinfo/Cargo.toml` has no `[lints] workspace = true`, so the
workspace's `indexing_slicing = "warn"` does not reach it. That is a concrete
instance of `TD-B-THE-UNIX-HALF-OF-COREUTILS-IS-NEITHER-LINTED-NOR-TESTED-BY-DEFAULT`
rather than a separate problem — the TD predicted this class would accumulate
unseen, and here is one that did.

### 2. `split_whitespace()` cannot parse `/proc/mounts`

The mount table escapes the four characters that would otherwise break its own
field separation — space, tab, newline and backslash — as `\040`, `\011`,
`\012` and `\134`. `show_disk()` splits on whitespace and unescapes nothing, so
a mount point containing a space arrives as `\040` in the middle of a field
(harmless but wrong on screen), while any *unescaped* whitespace shifts every
later field left by one: `parts[2]` stops being the filesystem type and
`parts[3]` stops being the options. The row still prints, with a plausible
value in every column. That is the failure mode worth naming — not a crash, a
confidently mislabelled filesystem.

### 3. `read_to_string` makes one bad byte delete the whole table

`read_proc()` is `fs::read_to_string(path).ok()`, so a single non-UTF-8 byte
anywhere in `/proc/mounts` turns the whole read into `None` and `show_disk()`
prints `(mount info not available)`. Mount points are paths, and this project's
paths *"allow all characters except `/` and null"* (CLAUDE.md, architectural
rules) — so a non-UTF-8 mount point is legal by our own spec, and one of them
hides every other mount as well as itself. CLAUDE.md self-review item 7 covers
exactly this: paths and OS-boundary data are bytes, and `read_to_string` is the
forced-UTF-8 form it forbids.

The other `read_proc()` callers are not affected in the same way, because
`/proc/cpuinfo`, `/proc/meminfo`, `/proc/loadavg`, `/proc/uptime` and
`/proc/stat` contain no paths and are ASCII by the kernel's own construction.
`/proc/mounts` is the one call site whose content is attacker- or
user-shaped. So the fix is not "stop using `read_to_string` everywhere" — it is
that the mount table needs a byte-oriented reader of its own.

### The proper fix

All three go away together in the `procinfo` crate, because they are the same
mistake — treating a path as a string:

- read `/proc/mounts` with `fs::read` and parse `&[u8]`;
- split on single spaces (the file's actual separator) rather than on runs of
  whitespace, and unescape `\040 \011 \012 \134` per field;
- return `Mount { device: PathBuf, mount_point: PathBuf, fstype: String,
  options: String }`, leaving truncation to the *caller's* formatter, where it
  can be done by characters instead of bytes;
- `#[test]`s for a mount point with a space, one with a backslash, one with a
  non-UTF-8 byte, and an option string that straddles the truncation width.

`userspace/sysinfo` has no tests at all today — it is a single `main.rs` of
`println!` — which is the reason none of this was caught. The extraction is
what makes it testable, so the tests land with it rather than after it.

**If it is never fixed:** `sysinfo disk` is correct for ordinary mounts and
stays correct; the panic needs a non-ASCII option string, the field shift needs
whitespace in a mount point, and the empty table needs a non-UTF-8 path. All
three become reachable the moment the OS mounts anything a user named.
