## TD-B-TAR-IGNORES-MEMBER-NAME-FILTERS-ON-EXTRACT-AND-LIST -- WITHDRAWN 2026-08-30

**Status:** **withdrawn** 2026-08-30, the same day it was filed — it was never
true of the `tar` that ships. Kept rather than deleted because *why* it was
wrong is the useful part.
**Where:** `userspace/tar/src/main.rs` — `extract_archive`, `list_archive`.

**Why it is withdrawn.** That file is not the `tar` anyone runs. The shipped
one is `userspace/coreutils/src/bin/tar.rs`, and it has had member-name filters
all along, down to the details this entry predicted would be forgotten: it
warns `tar: NAME: Not found in archive`, exits 2, and quotes the name in GNU's
`\351` style rather than a shell's `$'\351'` (`tar.rs:65`, `:2753`).
`scripts/tar-diff.sh` covers it with three cases — `extract: one member by
name`, `a symlink alone, by name`, `a fifo alone, by name` — all of which have
always passed. The gap described below was real, but only in a dead crate that
nothing outside the `userspace/*` workspace glob references; see
`B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES` for how that happened, and
`design-decisions.md` §710 for the rule that stops it happening again.

**The one part still worth acting on** was the `-C` interaction below — and it
turned out to be a genuine, and much larger, divergence in the *shipped* tar:
`-C` was stored as a single `Option<OsString>` and acted on in exactly one
place, inside the extractor. So it was parsed and then silently discarded under
`-c` and `-t`, and a second `-C` overwrote the first instead of being resolved
relative to it. It is **fixed** rather than filed; `scripts/tar-diff.sh`
section 8 holds the thirteen cases that now pin the behaviour down.

Everything from here down is the entry as originally filed, against the dead
crate.

**In short:** `tar -xf a.tar one two` should extract only the members named
`one` and `two`. Ours extracts the whole archive and ignores the names
entirely. Same for `-t`. Nothing warns; the user gets *more* than they asked
for and no indication that the filter was dropped.

The names are parsed and reach us fine — `Options.operands` now carries them
in order as `Operand::Member` — the two functions simply never consult them.
Create mode is the only mode that reads them.

**The fix.** Both functions need a member-name filter: collect the
`Operand::Member` entries, and if the list is non-empty, skip any archive
member that neither equals a listed name nor lies beneath one as a directory
prefix (GNU matches `dir` against `dir/sub/file`). GNU also warns
`tar: one: Not found in archive` and exits 2 when a requested name matched
nothing, which is the part most likely to be forgotten — a silent zero-match
extraction is exactly the failure this entry describes, one level down.

**The interaction to get right.** With `-C` now positional, GNU allows
`tar -xf o.tar -C d1 one -C ../d2 two`: the *filter* and the *destination*
vary together down the operand list, so each matched member goes to the
directory current at the point its name appeared. That cannot be expressed by
the single folded base directory `resolve_chdir_chain` returns today — it
needs the chdir chain evaluated per member, which is why the filter work and
the per-member destination work are one task and not two.

**What is NOT at risk.** Extracting too much is a usability failure, not a
security one: `sanitize_member_name` still runs on every member, so the extra
members land under the destination like the requested ones. Nothing here can
escape the base directory.
