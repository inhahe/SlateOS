## `B-WHICH-DIVERGES-FROM-GNU-IN-FOUR-MEASURED-PLACES` (lane B, 2026-08-27) — **open**, deliberate divergence

**In short:** Four places where our `which` deliberately does something other
than what GNU which 2.21 was measured doing. Two are upstream bugs we declined
to copy, one is a house rule about typed options, one is a message-shape choice.
Recorded here so a future reader who diffs the two does not "fix" them back.

| # | Upstream (measured) | Ours | Why |
|---|---|---|---|
| 1 | With `PATH` unset, the slash-split branch is skipped entirely, so `which /bin/sh` reports it missing even though it is right there. | An absolute or slashed command resolves without consulting `PATH` at all. | Upstream bug. The command names its own directory; `PATH` is irrelevant to it. |
| 2 | `which /init` splits into directory `""` and searches nothing, reporting `no init in ()`. | A command directly under the root searches `/`. | Upstream bug — the empty string is the wrong reading of "the part before the last slash" when the slash is the first byte. |
| 3 | An unrecognised option is reported and then *ignored*; the search proceeds. | Fatal: one diagnostic, `Try 'which --help' for more information.`, status 255. | Silently proceeding after ignoring an option the user typed is the same defect class this sweep exists to remove. The house `getopt` also ends its iteration at the first error. |
| 4 | Diagnostics go to stderr unprefixed in some paths. | Every diagnostic is `which: …` through `stdfd::diag_bytes`, so a non-UTF-8 path survives it. | House rule; also what makes a `PATH` with arbitrary bytes reportable at all. |

**Everything else was measured and matched**, including the surprising parts:
the exit status is the *count* of commands not found, wrapped to a byte (300
missing gives 44); `PATH=""` searches nothing while `PATH=":"` searches the
working directory twice; `--skip-dot` drops every *relative* element, not only
the dotted ones its own help text describes; `--skip-tilde` also drops absolute
elements that sit under `$HOME`; a fifo with the execute bit matches, so the
test is "not a directory, and `X_OK`", not `S_ISREG`; a leading `//` in a `PATH`
element survives cleaning but `///` collapses to `/`; and `--tty-only` freezes
only the four show/skip options written to its *right*.

**Where it lives:** `userspace/coreutils/src/bin/which.rs`, "Where this
deliberately diverges" in the module doc, and the tests named for each.
