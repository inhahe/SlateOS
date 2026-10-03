## §261 — The shell becomes byte-clean along the *expanded word*, not along the command line

**Date:** 2026-08-21
**Decided by:** Operator (Claude proposed and recommended this option; operator
agreed)
**Lane:** A

**In short:** Our shell cannot type, or tab-complete, a filename whose name is
not valid text — a name containing a stray byte that isn't part of any
character. The fix could either be to rewrite the entire shell (85,000 lines,
879 function signatures) so that *everything* it handles is raw bytes, or to
convert only the path that a filename actually travels: the word after it has
been expanded, the path resolver, tab completion, and the commands that consume
paths. The decision is the second one. A raw byte still cannot be *typed*
literally — you write `$'\xff'`, exactly as in bash — but every such filename
becomes reachable, listable and completable.

### Why the smaller change is not the lesser change

`known-issues.md` → `TD-KSHELL-LINE-EDITOR-IS-UTF8` originally prescribed
converting the line editor **and** the statement executors together, on the
argument that a partial conversion merely moves the lossy step from the keyboard
to the parser, where it is *less* visible. That argument is correct, and it is
not an argument against what was chosen — it targets a **layer** split (editor
byte-clean, parser not). This is not a layer split; it is a *data-flow* split.
One path — the path a filename takes from the user's keystrokes to the syscall —
becomes byte-clean end to end, with no lossy step anywhere along it. The source
line stays text, which is what it is in bash too: bash's script source is text
and its expanded argument is a byte string.

The measurement that forced the question: `kernel/src/kshell.rs` is 84,845 lines
and 879 of its 1,024 functions take or return `&str`/`String`. The entry's
"~1520 call sites" had counted method calls, not signatures. Option A was
therefore a single unreviewable commit rewriting a working shell, for a defect
the entry itself classifies as *not* data loss. Worth noting what is not
implicated: only 6 `from_utf8_lossy` sites exist in the whole file and all six
format file *content* (`column`, `diff`), not paths.

### What A would have bought that B does not

Byte-purity for non-path arguments. No known use case needs it, and the escape
mechanism (`$'…'`, already parsed at 7 sites) reaches it anyway.

### Where it lands

`kernel/src/kshell.rs`: word expansion, `resolve_path` (208; 270 call sites),
`get_cwd` (194), tab completion, and the path-consuming commands. Helpers exist
in `kernel/src/bytestr.rs` (stage (a), commit `d19372dd4`). Completion emits the
`$'\xff'` spelling for candidates that are not valid UTF-8.
