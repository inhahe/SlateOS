## TD-GUI-AND-APPS-HAVE-DRIFTED-FROM-RUSTFMT (lane C, 2026-08-17) -- **CLOSED; entry was stale**

**Closed 2026-09-07 (lane C), on measuring it.** `cargo fmt --all --check`
reports **zero** diffs across the workspace, against the 270 files this entry
describes. The pre-push `rustfmt` gate is presumably what keeps it that way --
it refused a push of mine earlier today, which is the gate working.

**Nothing was done to the code for this closure.**

Original entry follows.

**What.** `CLAUDE.md` says "Formatting: rustfmt defaults. No manual formatting
overrides." The tree does not meet that. `cargo fmt -p osfont -p guitk --check`
reports **270 files' worth of diffs** on the committed tree, with no
`rustfmt.toml` and no toolchain pin to explain it - the code was simply written
in a style close to, but not identical with, what rustfmt produces (single-line
`if c { a } else { b }`, hand-wrapped call chains).

**Why it is worth an entry rather than a `cargo fmt`.** It actively obstructs
normal work: running `cargo fmt -p <crate>` before committing - which is the
documented workflow - rewrites hundreds of unrelated lines and buries the change
in hand. During this task, `cargo fmt -p osfont` turned a 51-line addition into
a 341-line diff. The recovery was to reformat the *committed* version of each
edited file, treat committed-vs-reformatted as the formatting-only patch, and
reverse it out of the working copy. That worked, but it is not a workflow.

**The proper fix.** One dedicated commit per crate that does nothing but
`cargo fmt`, with no semantic change in it, landed when no other work is in
flight in that crate. `gui/**` and `apps/**` are lane C's alone, so this is
lane C's to do and conflicts with nobody. Do `osfont` and `guitk` first (they
are the ones that block edits most often), then the desktop and the apps.
After that, `cargo fmt --check` becomes a usable gate.

**Severity.** Low for correctness - formatting changes nothing that runs. Medium
for friction, because it makes every `cargo fmt` a trap and therefore makes the
documented pre-commit step something an agent learns to skip.

**Progress.** Chipped away at opportunistically, one crate per commit, whenever
a task leaves a file otherwise untouched:

| Crate / file | Diffs cleared | Commit |
|---|---|---|
| `apps/tmux/src/main.rs` | 47 | `fc1ff70b1` (2026-08-20) |

Still outstanding: `osfont`, `guitk`, `gui/desktop`, `apps/hexeditor`
(24 diffs), and the rest of `apps/**`.
