## 762. Both WSL crossings collapse to one output stream, but in opposite directions — toward the verdict in one, toward the report in the other

**Lane:** B
**Date:** 2026-09-04
**Decided by:** Claude (autonomous)

**In short:** Programs write two output streams — "results" (stdout) and
"progress and complaints" (stderr) — and a caller can point both at one log
file. Anything we run inside WSL, the Linux environment on this Windows box,
loses one of those two streams entirely when that happens: the bytes are
overwritten with no error (known-issues.md Lesson 113). The fix is to make the
WSL side emit *one* stream instead of two. That works either way round, and the
choice of which stream to keep is not free, so the two places that needed it
chose differently and this records why.

**The constraint.** The collapse has to happen on the far side of the boundary,
inside WSL, because that is the last point at which the two streams are still
ordinary pipes rather than file handles `wsl.exe` has reopened. So it is a
one-line `exec` redirect either way; there is no arrangement in which both
streams survive across the boundary into one file, and none in which the near
side can fix it. What is genuinely being chosen is which stream the merged
output rides on, and therefore what is left alone on the other one.

| | `scripts/coreutils-check.sh` | `scripts/diff-wsl.sh` (50 harnesses) |
|---|---|---|
| Collapses | `exec 1>&2` — everything to stderr | `exec 2>&1` — everything to stdout |
| *What changes:* | stdout carries the five-line summary and nothing else | stdout carries the report plus cargo's build chatter |
| Leaves free | stdout, for the verdict | stderr, which ends up empty |
| Why | The script's product is a *verdict* about whether a half ran. Isolating it makes `>verdict 2>log` split cleanly, and puts the verdict somewhere no WSL handle can reach. | The harnesses' product is the report `197 passed, 0 differed`, which has always been on stdout. Moving it to stderr would change the observable behaviour of 50 scripts to fix a bug in none of them. |

**The rejected uniform answer.** Doing the same thing in both places would be
tidier and was the first instinct. Collapsing both to stderr would move every
harness's report off stdout — a user-visible change to 50 scripts, and one that
breaks the reasonable expectation that a passing harness prints its result on
stdout. Collapsing both to stdout would leave `coreutils-check.sh`'s verdict
sharing a file with up to 1.2 MB of cargo manifest warnings, which is how the
whole problem was found in the first place. Uniformity here would buy a
sentence of documentation at the cost of the property each file actually needs.

**What the asymmetry costs.** Someone reading the two scripts side by side sees
opposite redirects and has to read a comment to learn why; that is the price,
and both comments state it. It also means neither file preserves the
stdout/stderr *distinction* across the boundary — that is not recoverable, and
pretending otherwise is what the two failed attempts in Lesson 113 did.

**A note on the general rule this implies.** A new WSL-invoking script should
decide, before it is written, which of its two streams carries the thing a
caller must be able to read when everything else has gone wrong, and put only
that on it. The default should be `coreutils-check.sh`'s shape — verdict alone
on stdout — because it is the one that keeps working when the tool underneath
gets loud. `diff-wsl.sh` is the exception, and it is an exception only because
it has 50 existing callers.
