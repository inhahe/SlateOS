## 710. A duplicated utility's live copy is identified by what builds it, not by a table that records which copy was live once

**Date:** 2026-08-30
**Decided by:** Claude (autonomous), lane B
**Lane:** B

**In short:** Forty-one of our command-line tools exist twice in this tree under
the same name — once inside the big `userspace/coreutils` package, once as its
own small `userspace/<name>` crate. Only one of each pair is the program that
actually ships and gets tested; the other is a leftover. `known-issues.md` kept
a table saying which was which. The table went out of date, I trusted it, and
spent a session improving a `tar` that nothing in the tree runs. The decision is
to stop treating that table as the answer and instead read the answer off the
build: whichever copy the differential harness names is the live one.

**What went wrong.** The table (`B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES`)
was written 2026-08-22 and put `tar` in the row meaning "the standalone crate is
the real one, `coreutils`' is a stub". That was true then. In the eight days
since, `coreutils/src/bin/tar.rs` grew to 7189 lines and 131 tests — the
old-option style, the 160-entry long-option refusal table, the delayed-symlink
traversal defence, the whole overwrite family — while `userspace/tar` stayed at
2724 lines and 72 tests. Nothing outside the `userspace/*` workspace glob
references `userspace/tar`: not `scripts/tar-diff.sh`, not the image staging,
not any script. Cargo builds both to one `tar.exe` and the last link wins.

Five commits of correct, GNU-measured parity work went into the copy nobody
runs. One of them solved a problem the live copy does not have: `userspace/tar`
implements `-C` by joining paths, so it must reason about whether to print the
member name or the joined destination; `coreutils` chdirs the way GNU does, and
the question never arises.

**The decision.** Two rules, in order:

1. **Before editing a duplicated utility, determine the live copy from the
   build, not from prose.** The differential harness is the authority, because
   it is the thing that certifies behaviour: `DIFF_PKG` in `scripts/<name>-diff.sh`
   names the package under test (defaulting to `coreutils`). Confirm with a
   search for any reference to the standalone crate outside the workspace glob.
   Two commands, and they cannot go stale.
2. **The table stays, but as history rather than as an index.** It records how
   the fork happened and why resolving it needs a per-utility decision, which is
   still worth having. Its rows are annotated as a snapshot with a date.

**The alternative considered, and why not.** The obvious fix is a script that
regenerates the table — compare line counts and test counts per pair, rewrite
the rows. Rejected: it automates the wrong artifact. A generated table is still
a cache of a fact that the build already states unambiguously, and it would
carry the same failure mode one level removed — a stale *generated* table is
harder to distrust than a stale hand-written one, because it looks authoritative.
The cheaper and more honest move is to delete the reader's reason to consult a
table at all.

**What this does not decide.** Which copy *should* be canonical remains open and
is an operator question: §8 (an operator decision) retires `coreutils/src/bin/*`
in favour of standalone crates, while `tar`, and the seventeen tools in the
first row of that table, have in practice gone the other way. Nothing here moves
or deletes either copy; it only fixes how the live one is identified. The
`tar` pair is now a concrete argument for revisiting §8, because following §8
for `tar` would mean retiring the 7189-line implementation that passes 178
differential cases in favour of the 2724-line one that no harness measures.

**How to reverse.** Nothing to revert in code. If a regenerating script is
written later, the argument above is what it has to answer: say why a generated
table beats reading `DIFF_PKG`, given that the generated one is also a copy.
