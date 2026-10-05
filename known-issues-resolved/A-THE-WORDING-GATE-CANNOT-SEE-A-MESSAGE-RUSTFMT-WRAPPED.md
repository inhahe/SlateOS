## `A-THE-WORDING-GATE-CANNOT-SEE-A-MESSAGE-RUSTFMT-WRAPPED` (lane A, 2026-08-29) — **FIXED 2026-08-29**, one character

**In short:** a gate checks that every self-test assertion names text the
command under test can actually print, so a rung cannot pass by asserting
wording that no longer exists. To do that it collects the strings each command
prints. It could not see a string that `cargo fmt` had split across two source
lines — 61 of them in `kshell.rs` — so those commands owned none of that
wording as far as the gate was concerned. The visible symptom is the opposite
of silence: it accuses *correct* rungs. The invisible half is worse, and is why
this is filed rather than just fixed.

**Where.** `scripts/check-selftest-wording.py`, `LITERAL` (was line 145):

```python
LITERAL = re.compile(r'"(?:[^"\\]|\\.)*"')     # no re.DOTALL
```

A Rust string may be continued onto the next line with a trailing backslash:

```rust
shell_println!(
    "gpu: {}: unknown colour `{}'; expected red, green, blue, \
     white, black or a 0x-prefixed ARGB value",
    sub, other
);
```

`\\.` is the escape alternative, and without `re.DOTALL` its `.` refuses a
newline. So the scan of that literal dies at the backslash, never reaches the
closing quote, and **the literal is absent from the match entirely** — not
truncated, absent. Worse, `finditer` then resumes at the *closing* quote and
treats it as an opening one, so the literals after it in the same argument list
are misaligned too. The total barely moves (46888 matches against 46889), which
is exactly why nothing noticed: the count was right and the contents were wrong.

**Why only here.** Every other literal regex in the file (`STR`, `BSTR`,
`BYTES_LET`) runs over `statements()` output, which has already collapsed the
newline to a space — `unescape`'s docstring says so, and eats "backslash
followed by whitespace" for that reason. `print_literals` is the one scan that
must run over the **raw** body, because it needs real offsets into it, so it is
the one scan that meets the continuation as written.

**Resolution.** `re.DOTALL` on `LITERAL`, plus a comment saying why it is
load-bearing, plus a fixture case that fails without it: `cmd_vdesktop` gained a
`rename` arm whose refusal is wrapped, and a rung asserting it. It is in the
self-test's *controls* — the list of correct code that must **not** be reported
— so a future edit that drops the flag is caught by `--self-test`, not by a red
boot test two batches later. Verified by reverting the flag: the control fails,
alone.

**How it surfaced, which is the part worth keeping.** Not from the gate's
output, which had been green. It surfaced because batch 38 of the operand-refusal
burn-down added two rungs asserting refusal messages long enough for rustfmt to
wrap, and the gate reported them as naming text the command cannot print. The
messages were right; the gate could not read them. Had I taken the gate's word
for it — its own error text offers an `ALLOWED` list for "genuinely right and
merely underivable" — I would have written two exemptions and left 61 literals
invisible, in a checker whose entire purpose is to notice when an assertion and
a message drift apart.

**This is the third gate found blind for a reason unrelated to the code it
inspects**, and the third in three days:

| Gate | Blind to | Hid |
|---|---|---|
| `check-shell-portability.sh` (§630) | anything, if shellcheck was absent | the whole bug class |
| `check-option-refusal.py` (`…RUSTFMT-USES-FOUR`) | a chain rustfmt wrapped | 466 sites |
| `check-option-refusal.py` (§634) | `from_str_radix` | 19 sites |
| `check-selftest-wording.py` (this) | a message rustfmt wrapped | 61 literals |

Two of the four are the *same* cause: **rustfmt decides the layout, and a regex
written against the layout the author typed is a regex against a coincidence.**
`statements()` exists to fix that class once; this scan could not use it, and
paid the cost separately. Before adding any regex over raw Rust here, ask what
`cargo fmt` is free to do to the construct it matches.

**The half that is still not measured.** A gate whose failure mode is *accusing
correct code* announces itself, and that is how this one was caught. But the
same missing flag also *enlarged* nothing and *shrank* every affected pool,
which means an assertion that should have been reported could equally have been
excused by the witness rule instead — and that failure is silent. There is no
count of how many, because the question a gate cannot answer is how much it did
not see. This is design-decisions.md §634's corollary landing for the third
time: **a gate's own output cannot audit its coverage.** The only method that
has ever worked on this project is the one used both times — name a specific
thing you believe the gate should catch, and check that it agrees.
