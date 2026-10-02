### TD-OILS-A-COMMAND-RUN-FROM-A-CUT-SHORT-ARITHMETIC-SUBSTITUTION-IS-BLAMED-AT-THE-WRONG-LINE. `` echo $(( $(case x in x) echo 5;; esac) )) `` — 2026-08-07 — ✅ FIXED 2026-08-07 by `ecd8e610d`

**Where:** `userspace/oils/src/interp.rs` — the line a command inherits when it
is run from text the arithmetic scan left over.

**What.** Both shells end the substitution at the `case` pattern's `)` — that is
the scan's table, not a defect (see `Shell::skip_opaque`) — so `5` is left as a
bare word and looked up as a command. They disagree only on the line blamed:

```text
bash: case.sh: line 25: 5: command not found
osh : case.sh: line 21: 5: command not found
```

Line 21 is the `( eval "$1" )` inside the corpus helper `e`, i.e. the line the
*enclosing* command reached. Line 25 is further on, so bash is counting
something osh is not; the probe was one physical line either way, so this is not
a body-line-vs-physical-line question of the kind
TD-OILS-A-FAILING-BACKTICK-INSIDE-AN-ARITHMETIC-EXPANSION-LOSES-ITS-DIAGNOSTIC
settled.

**The 25 explained, and closed with it.** The recorded next step was to measure
what bash counts before changing anything. It turned out to need no change of
its own: the text the arithmetic scan cuts short is re-read as a substitution
body, so it is numbered from the closing delimiter the way every substitution
body is — and once `src` became the re-print
(TD-OILS-A-SPLICED-REPRINT-DOES-NOT-MOVE-LINENO, commit `ecd8e610d`) the count
started from the same place bash's does. Both shells now agree on the recorded
shape and on three more, including a doubly-nested one blamed fourteen lines
past where it was written.

Now probed in `tests/corpus/arith-expansion-scan-and-classify.sh`, which had
excluded this shape for exactly this reason; the exclusion note was replaced by
the two live probes in commit `3bf18529e`.
