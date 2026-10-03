## TD-A-THE-SOURCE-DIGEST-PROTECTS-THE-VERDICT-NOT-THE-RUN (lane A, 2026-09-14)

**In short:** `boot-test.sh` re-digests `kernel/src/**/*.rs` mid-run and sets
`BT_SRC_CHANGED` if it moved, so a boot cannot claim to have tested a tree that
changed under it. That check covers **kernel sources only**, and it is about the
*verdict*. It is not a licence to edit anything else during a run. Several gates
validate tracked non-source files -- `check-accidental-headings` reads every
tracked `*.md` (390 documents), and the known-issues, bands and doc-links gates
read the shared docs -- and they read the **working tree**, so a mid-run edit to
a shared document is evidence the run will judge, roughly 75 gates and twenty
minutes in.

**How it nearly bit.** On 2026-09-14 lane A reasoned "docs are not in the digest,
so editing them during the boot is safe", and then edited `design-decisions.md`,
`known-issues.md`, `todo.txt` and a `requests/` file across a 20-minute run. The
digest reasoning was correct and irrelevant: the run then reached
`=== Checking every tracked *.md for a --- that renders as a heading ===` and read
those edits. It passed, but nothing in the reasoning that authorised the edits
predicted that, and a stray `---` under a text line would have failed the whole
run at that gate.

**The rule.** During your own boot run, "not digested" means "will not invalidate
the verdict", not "will not be read". The safe set during a run is files no gate
reads: scratch under `build/`, and nothing tracked. Editing tracked `.md` is
*usually* fine and occasionally fatal, which is the worst ratio for a habit.

**Proper fix, if it recurs:** have the mid-run digest cover every tracked file a
gate reads rather than just `kernel/src`, and report an edit to one as a distinct
outcome from a source change -- "a document this run validates was edited while
it ran" is a different fact from "the kernel changed", and only the second makes
the boot's verdict meaningless.
