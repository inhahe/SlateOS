## TD-B-TWENTY-NINE-OF-THE-SEVENTY-TWO-BINS-ON-THE-IMAGE-DECODE-LOSSILY (lane B, 2026-09-14)

**Status: RESOLVED 2026-09-14** — see the resolution section below. One real
defect on the whole image (`diff`), fixed the same day; `scripts/lossy-decode.py`
is the standing instrument.

*Header corrected 2026-09-16.* It read `OPEN — a candidate list, deliberately
not a defect list`, which was true when written and stopped being true in the
same entry, four sections down. Anyone triaging by grepping for OPEN picked
this up as work: I did, today, and read the whole entry before reaching the
answer. That is the cost of a status line that disagrees with its own body --
the body was right the whole time, and nothing re-read the header after the
section that superseded it was appended.

Cross-referencing `scripts/rootfs-bin-manifest.txt` against a grep for
`from_utf8_lossy` in each binary's own source: **29 of the 72** Rust utilities
on the image contain the construct CLAUDE.md names as silent data corruption.

    awk 3   basename 2   cp 3    csplit 4   dd 1    df 4     diff 1
    du 2    ed 1        env 1    expr 4     fetch 2 find 2   hostname 1
    ln 2    mkfifo 1    nl 3     od 2       readlink 1       realpath 9
    sed 20  split 6     stat 2   strings 3  tar 6   test 2   touch 1
    tty 1   which 2

### Why this is NOT "29 broken binaries", and I want to be exact about it

A lossy decode is only a *defect* where the decoded value is then **compared,
stored, or written**. That is what made `diff` wrong: it decoded lossily and
then diffed the result, so two distinct bad bytes became one U+FFFD and
compared equal. A lossy decode used only to put a name in a diagnostic is
cosmetic and, on a name that is not text, arguably the right thing.

This grep cannot tell those apart. It counts occurrences, and occurrences are
not findings — `TD-B-MY-AD-HOC-SEARCHES-OVER-REPORT-BY-AN-ORDER-OF-MAGNITUDE`
records this exact instrument being wrong by 5–20× three times in one day
(9→1, 46,736→13, 45→2). The honest state is: **one confirmed defect (`diff`),
28 other files worth reading**, and a prior expectation that well under half
survive.

**CORRECTED 2026-09-14, and both of the priorities below were wrong.** The
paragraph that stood here read: *"`sed` at 20 and `realpath` at 9 are the two
worth opening first — not because the count is high, but because both are
fundamentally about transforming text and paths rather than printing them, so
their lossy calls are the most likely to sit on a value path rather than a
message path."* That is reasoning about what a program is *for*, not a
measurement of what its code does, and it picked the two worst candidates on
the list.

Re-measured with the test modules cut off at `#[cfg(test)]` and the remaining
uses classified by whether a diagnostic macro appears within two lines:

| | occurrences |
|---|---|
| raw, as the count above was taken | 131 |
| outside test modules | **46** |
| not feeding a diagnostic | **36** |

* **`realpath` has ZERO outside its tests** — all nine were assertion messages.
* **`sed` has one**, and it formats `can't find label for jump to ...`. Five
  probes confirm `sed` passes byte 0xE9 through `s///`, `y///`, `d`, an anchored
  insert and a wildcard match untouched.
* The actual head of the list was **`diff`** — the file being edited all day —
  with three, two of which were a real defect: a directory walk decoded the
  names it had just listed and then could not open them. Fixed; see the commit
  *"diff: carry paths as paths"*.

Two things this adds to `TD-B-MY-AD-HOC-SEARCHES-OVER-REPORT-BY-AN-ORDER-OF-MAGNITUDE`:
a grep that does not exclude test code over-reports by roughly **3×** on top of
everything else, and ranking candidates by subject-matter intuition is worse
than not ranking them at all, because it moves the genuinely broken one down
the list.

### RESOLVED 2026-09-14 — the instrument exists, and the answer is one defect

`scripts/lossy-decode.py` now does what the section below asked for. Against
the 72 binaries on the image:

| | count |
|---|---|
| raw occurrences of the two names | **131** |
| inside `#[cfg(test)]` (assertion messages) | 85 |
| `#[cfg(not(unix))]` host-only halves | 18 |
| rendered into a diagnostic | 12 |
| **VALUE — could corrupt something** | **6** |

All six were then read, and **all six are benign**: two are
`String::from_utf8_lossy` of an ASCII *constant* (`stat`'s `TERSE_FILE` /
`TERSE_FS`), one sits behind `looks_like_integer` which admits only ASCII
digits and a sign (`expr`), one decodes solely to match an ASCII long-option
name (`od`), and two are bindings used only in messages that the checker's
binding-following rule does not quite reach (`split`, `strings`). `split`'s is
worth naming because it is the shape that looks worst and is right: the
decoded name goes to `println!` while the child's `FILE=` environment variable
gets `os_from_bytes(name)` beside it.

**So the entire image held exactly one real defect of this kind, and it was in
`diff`** — fixed the same day. Not `sed`, not `realpath`, the two this entry
originally named.

The checker's four exclusions were each added because a real site needed it,
and two were added after it produced a false positive on the very file whose
shape most resembled the bug:

* `test.rs` writes `path_of` as a `#[cfg(unix)]` block beside a
  `#[cfg(not(unix))]` block INSIDE one function — the first draft only looked
  at attributes on the function itself and called the second block a defect;
* five of the nine it then reported were `let x = lossy(...)` used in a
  `format!` two statements later, which no fixed window can see.

Both directions are pinned in its 15-case self-test, including that a cfg
block which has already *closed* does not excuse a later call, and that a
binding which reaches real work is still a VALUE even if it is also printed.

**Still to do:** a baseline file so the six audited sites are recorded as
accepted and the checker can be wired into `scripts/hooks/pre-push` as a
ratchet, the way `argv-utf8` and `raced-globals` are. Until then it is a tool
to run, not a gate. Note also that `--all` reports **VALUE 183** across the
whole of `userspace/`, which is a much larger surface than the image and has
not been audited at all.

### The instrument this actually wants

A checker that classifies each `from_utf8_lossy` / `to_string_lossy` by what
happens to its result:

* flows into `==`, `contains`, a `match`, a sort key, a hash → **defect**;
* flows into `fs::write`, `write_all`, a path passed to a syscall → **defect**;
* flows only into `format!`/`diag!`/`eprintln!` → allowed, and should be
  recorded as allowed so the count stops re-alarming whoever greps next.

That is a dataflow question, not a pattern question, which is why the grep
above is the wrong shape and why the number it produced should not be quoted
as a defect count. Written down now so the *measurement* is not lost while the
triage waits; the `diff` fix is the thing that comes first, since it is the one
known to answer wrongly.
