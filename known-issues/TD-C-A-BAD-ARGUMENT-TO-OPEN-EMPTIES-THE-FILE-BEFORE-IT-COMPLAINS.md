## `TD-C-A-BAD-ARGUMENT-TO-OPEN-EMPTIES-THE-FILE-BEFORE-IT-COMPLAINS` (lane C, 2026-09-17) -- **REMEDY ADOPTED 2026-09-22**

> **Remedy moved into the tree and adopted, 2026-09-22.** `safewrite.py` now
> lives at `scripts/safewrite.py`, 20 truncating writes aimed at tree files
> have been converted, and `scripts/check-destructive-writes.py` refuses new
> ones. The sub-entry below, addressed to lane A, is unrelated and stays open.
>
> **The remedy was not in the repository.** This entry told the reader to use
> `build/safewrite.py`, and `.gitignore` line 88 is `/build/`. So the fix for a
> data-loss defect existed in exactly one working tree: a fresh clone had no
> such file, the other two lanes never had it, and the hundred-odd scripts
> already importing it were themselves ignored scratch. Adoption inside
> `scripts/` -- the durable gates every lane runs, several of which rewrite
> tracked files in place -- was **zero**, and the number of users made it look
> broad.
>
> The shape is worth keeping: **a fix filed in a directory the repository does
> not track is a fix nobody but its author has**, and citing it by path in a
> tracked document makes it look present. What made it visible was trying to
> `git mv` the file and being told it was not under version control.
>
> **What was converted.** The dangerous class is a write whose target is a
> module-level constant built from `__file__`, because that names a file the
> tree already has: the baselines four gates use to remember what they have
> seen, and the documents the `--apply` tools rewrite.
> `check-collapsed-messages.py` was the worst of them -- it rewrites arbitrary
> `.rs` files and is run tree-wide. Also converted: `argv-utf8`,
> `check-argv-ignored`, `check-control-bytes`, `check-design-decisions-bands`,
> `backfill-lane-fields`, `check-env-identity`, `check-read-defaults`,
> `check-roadmap-done`, `check-workspace-lints`, `dup-differential`,
> `host-errmsg`, `multicall-aliases`, `quote-names`, `raced-globals`, and nine
> `reintro-*.py` that rewrite source under `ROOT`.
>
> **The new gate is deliberately narrow.** It ignores writes into a fresh
> temporary directory, which cannot destroy anything that existed a moment
> ago. Flagging them would make the output mostly non-defects, and a gate like
> that is one people learn to skim -- the same reason the variant-list checker
> was first scoped by name. Narrow is affordable here because the wide version
> already exists, for a different property.
>
> **The two gates had to be taught to compose.** `check-text-mode-writes.py`
> requires every text-mode write to pass `newline=`, and converting a site to
> `safewrite.write_text(path, text)` made it red: it reads the name
> `write_text`, sees a `pathlib` call with no newline, and says so. It now
> exempts the *bare* call in a file that imports it from `safewrite`, while
> still grading `p.write_text(...)` -- so the exemption cannot be borrowed by
> adding an unused import. Three self-test cases hold that line, 50 in total.
>
> That interaction is the most useful thing here, because it is causal rather
> than coincidental: **the older gate is what put a hand-typed escape at every
> write site in the tree, and a mistyped escape is what emptied the hook.** A
> gate that demands an argument raises the odds of a bad argument; a
> destructive default turns a bad argument into data loss. Passing the newline
> once, inside `safewrite`, is what makes the first rule safe to keep.
> Requiring callers to spell it again would have restored the exposure while
> appearing to tighten it.


**In short:** `io.open(path, "w", ...)` truncates the file and *then* validates
its arguments. A typo in one of them destroys the target and raises afterwards,
so a script that never wrote a byte can still leave nothing behind. This
emptied `scripts/hooks/pre-push` -- 5,591 lines, the hook all three lanes push
through -- from a script whose only fault was `newline="\\n"` where it meant
`newline="\n"`.

**Demonstrated, not inferred:**

    >>> io.open(P, "w", encoding="utf-8", newline=chr(92) + "n")
    ValueError: illegal newline value: \n
    >>> io.open(P, encoding="utf-8").read()
    ''

The file is empty and the exception makes it look as though nothing happened.

**Why it was nearly invisible.** An empty hook is not an obviously broken one:
`sh -n` accepts it, git runs it, it exits 0, and every gate silently does not
run. What caught it was `scripts/test-pre-push-gates.py` reporting that
`pre-push` had no shebang -- a structural check that had no idea what it was
really looking at. Restored with `git restore`; it had never been committed or
pushed.

**The transferable part is the shape, not the typo.** A destructive operation
sequenced before its own validation. The same shape as `fs::write` truncating
before it writes, which is the entire reason `safeio::write_atomically` exists
in this tree for Rust -- and the Python side had no equivalent, so every
generator script in `build/` and `scripts/` carries this.

**What to do instead.** Build the text, write it beside the target, rename over
it. A rename within a directory is atomic, so a failure anywhere before it
leaves the original untouched. `scripts/safewrite.py` is that, and its
self-test shows the same typo leaving the file as it was. It was written in
`build/`, which is gitignored, so for five days the remedy existed only in the
one working tree that wrote it -- see the note at the top of this entry.

**Why the fix is not "be careful".** I typed this exact escape three times in
one session -- twice caught before running, once not. A habit that fails one
time in three is not a habit, and the answer to a destructive default is to
stop calling it, not to concentrate harder.
