## B-PATCH-IGNORED-THE-TARGET-YOU-NAMED (lane B, 2026-09-12) — FIXED

Three fixes, `patch-diff.sh` 51 passed / 14 differed to **56 / 9**.

**An explicit target operand was parsed and then discarded.** `patch -i u.patch
-p1 a/base.txt` patches `a/base.txt` whatever the patch says — that is the
point of naming it. This build stored the operand in `target_file` and never
read it, so the name was accepted and thrown away. Because the patch's own path
did not resolve, the case failed with `can't find file to patch` while GNU
patched happily. **An option that is parsed but unread is worse than one that is
refused**: the refusal at least tells you.

**`-s` suppresses the narration, not the outcome.** Measured: `patch -s` on a
failing patch still prints `1 out of 1 hunk FAILED -- saving rejects to file
X.rej`, while suppressing `patching file X` and the per-hunk lines. Ours
suppressed everything, so a script running `patch -s` and reading stdout was
told nothing at all about a failure. Silent means do not narrate the work; it
does not mean hide that the work did not happen.

**`-l`/`--ignore-whitespace` IS IMPLEMENTED as of 2026-09-15.** It was accepted
and inert, on the same terms as `-N`/`-f`/`-F`/`-Z` above: it changes an answer
only where a hunk differs from the target in whitespace alone, and no case in
this tree did.

That argument was wrong, and the way it was wrong is worth keeping. Lane C's
dead-field detector found the flag was parsed and read by nothing, and the
defect is not the inertness -- it is that `--help` advertised "Match ignoring
whitespace." with no hint of it. Of the four places the option was written
down, three said inert and the only one a user reads said it worked. **An inert
option is defensible exactly as long as nothing promises otherwise.**

The matching rule is measured, not guessed: strip trailing whitespace, then
treat any run of whitespace as equal to any other run -- a run matches a
different run but never matches nothing. Whitespace is SPACE and TAB only;
`\v`, `\f` and `\r` all fail against a space, so `is_ascii_whitespace()` would
have been wrong three ways with every fixture still green. Eighteen measured
cases, in `loose_eq`'s doc comment and its tests.

`patch-diff.sh` had an `--ignore-whitespace` case throughout, and it passed
throughout, because its fixture's target and patch agree about whitespace. A
flag-bearing case whose fixture makes the flag irrelevant is not coverage. The
three cases added alongside this fix use a tab-indented target against a
space-indented patch, and two of them fail if the flag goes inert again.
