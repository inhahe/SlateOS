## B-CALCULATOR-OUTPUT-DETAILS-NEVER-CHECKED-AGAINST-A-REAL-`bc` (lane B, 2026-08-16) — ✅ **FIXED 2026-08-16**

**In short:** our `bc` and `dc` computed the right *numbers* but nobody had ever
checked how they *write* them down, because this machine appeared to have no
real `bc` or `dc` to compare against — so several formatting details were
decided from memory. It turned out the machine did have them: GNU `bc` 1.07.1
was already installed in the WSL Ubuntu distribution, and GNU `dc` 1.4.1 was one
`apt-get` away. `scripts/calc-diff.sh` now runs 200 cases through both ours and
GNU's and compares stdout byte-for-byte, and **seven** of the things we
"remembered" were wrong.

**Where:** `userspace/bignum/src/decimal.rs`, `userspace/bc/src/main.rs`,
`userspace/dc/src/main.rs`. The harness is `scripts/calc-diff.sh`; run it with
`bash scripts/calc-diff.sh` after any change to how either program prints.

**What the harness found on its first two runs** — every one of these had passed
a unit test, because the test asserted the same belief the code did:

| # | We did | GNU does | Why it mattered |
|---|---|---|---|
| 1 | wrapped `bc` output at 70 columns | 69: `bc` breaks at `BC_LINE_LENGTH - 2`, `dc` at `DC_LINE_LENGTH - 1` | Same source tarball, two different arithmetics. Nothing but measurement would have found this. |
| 2 | `[] a` produced an empty string | a one-byte NUL (`[] a Z p` says `1`) | `a` was total in GNU and partial in ours. |
| 3 | printed `obase>16` with letters `G`–`Z` | space-separated zero-padded decimal groups: `obase=36; 1295` → ` 35 35` | A script reading base-36 output got a completely different lexeme. |
| 4 | `x++` as a statement printed nothing | prints the value *before* the increment (`++x` prints the value after) | We had classified `++` as an assignment, so `x++` on its own line was silent. |
| 5 | interpreted `\n` in *every* string | only in `print`; a bare `"a\nb"` statement writes four literal characters | Escape handling belongs to `print`, not to the lexer. |
| 6 | `R` rotated the top *n* one way | the other way (`1 2 3 4 3 R f` → `2 4 3 1`) | A 50/50 coin-flip we lost. |
| 7 | a failed operator consumed its operands | leaves them exactly where they were (`1 0 / f` still shows `1 0`) | Ours left a half-eaten stack after every diagnostic. |

The zero-at-a-non-zero-scale question this entry used to leave open is settled
by the same means: GNU prints plain `0`, which is what we already did.

**`dc`'s `a` command is implemented** (was listed here as the last gap): the top
of the stack becomes a one-byte string — a number contributes its low-order
byte, a string its first. Implementing it forced `dc`'s strings to become byte
strings rather than `String`, which also fixed a separate bug found on the way:
`dc` read its script files with `read_to_string`, so a script holding a
non-UTF-8 byte in a string literal was refused before its first command ran.
Every POSIX `dc` command is now present.

**The lesson worth keeping** is the one the entry half-anticipated: the
`bc`↔`dc` cross-check we had been relying on could not substitute for a real
reference, because both front-ends format through the same `Decimal` — a
misreading is inherited by both, and they agree with each other while both being
wrong. Six of the seven rows above are exactly that failure. The other lesson is
narrower and more embarrassing: **look for the reference implementation before
concluding there isn't one.** `bc` was on the machine the whole time.
