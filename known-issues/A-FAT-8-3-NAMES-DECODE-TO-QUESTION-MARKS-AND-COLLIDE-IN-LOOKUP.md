### A-FAT-8-3-NAMES-DECODE-TO-QUESTION-MARKS-AND-COLLIDE-IN-LOOKUP (lane A, 2026-09-12)

**In short:** FAT stores short filenames in a DOS codepage, not UTF-8. We decode them as
UTF-8 and substitute `????????` when that fails — so any accented short name becomes the
same eight question marks, and two different files answer to one name.

**Where:** `kernel/src/fs/fat.rs`. `display_name()` (715) falls back to the 8.3 name when
there is no long-name entry; `short_name()` (740) uses it **always**. Both decode with
`core::str::from_utf8(..).unwrap_or("????????")` for the base and `"???"` for the
extension.

**Why it is not an edge case.** 8.3 names are codepage-encoded *by specification* —
CP437/CP850 and friends — so a byte ≥ 0x80 is the normal representation of an accented
character, not corruption. Any disk formatted or written by a DOS-era tool, a camera, or
embedded firmware carries them.

**The collision is in a matching path, which makes it worse than the procfs case.**
`fat.rs:1892-1899`:

```rust
if e.display_name().eq_ignore_ascii_case(&target) { return true; }
if e.long_name.is_some() { return e.short_name().eq_ignore_ascii_case(&target); }
```

So a lookup for the literal `????????.???` matches **every** entry whose short name failed
to decode, and returns whichever comes first; and a file whose real short name is
non-ASCII cannot be found by its real name at all. `A-EXEC-WRITES-A-COMM-...` manufactures
collisions in a *display*; this one manufactures them in a *resolver*.

**[A] 2026-09-12 — the COLLISION is fixed; the DISPLAY question stays open, and the two are
separable in a way this entry did not make clear.** A-Q12 asks which code page an 8.3 name
was written in. That is the operator's and it governs *display*. It does not govern
*matching*, and the matching bug is wrong under every possible answer to it:
`display_name()` and `short_name()` substitute `????????`, which is a **constant**, so a
lookup for that literal matched every undecodable entry and returned whichever came first.

`FatDirEntry::short_name_decodes()` now guards both comparison arms in the path lookup
(`fat.rs`). An undecodable short name is no longer compared at all, so such a file is
unfindable — which it already was — without the lookup ever handing back a *different* file.
**Turning a wrong answer into no answer is the fail-safe direction and needed no policy
decision.**

The second guard is the one worth pointing at: `short_name()` uses the 8.3 bytes **always**,
even when a long name exists, so that arm could fabricate for a file whose LFN decodes
perfectly. Guarding only the no-LFN path would have looked complete and left the more
common case open.

Still open and still the operator's: what an undecodable short name should *look like*. The
recommendation in A-Q12 is escapes by default with a per-mount code page, on the argument
that the escape is the correct answer in the absence of information.

**Proper fix:** the same shape as the comm work — keep the 8.3 field as the eleven bytes it
is, compare bytes, and decode lossily only for display. `DirEntry.name` is already byte-
clean since `D-VFS-PATHS-ARE-STR-NOT-BYTES`, so the destination type exists; what does not
is a decision about *which codepage* to use for display, which is a real question and not
one to answer in passing.

**Found by enumerating the defect rather than the subsystem**, which is the only reason it
surfaced: grepping `comm_truncate` found three sites in one file; grepping the literal
`"???"` found four there, a fifth surface (`/proc/<pid>/cmdline`), this, and a *fixed*
instance in `fs/ar.rs` whose comment records the same reasoning. A search keyed on the
path being worked on cannot contain a defect in a different subsystem.
