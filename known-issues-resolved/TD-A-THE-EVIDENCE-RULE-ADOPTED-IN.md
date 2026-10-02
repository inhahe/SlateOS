### TD-A-THE-EVIDENCE-RULE-ADOPTED-IN-§240-CANNOT-BE-SATISFIED-BY-ANY-EXISTING-COMMAND (lane A, 2026-08-19)

**Status:** FIXED 2026-08-19 in `scripts/bench-history.py` (`--accel-compare`),
with 13 tests in `scripts/test-bench-history.py`, every one of them
mutation-checked. See "How it was fixed" below — including the one clause of
the spec that the recorded data cannot satisfy on its own, which is now an
explicit, labelled assertion by the reader rather than a silent inference.

(It was blocked until the WHPX layout sweep finished: the fix is in `scripts/**`,
and editing `scripts/**` mid-sweep changes the source digest and aborts the
sweep — see `B-A-A-LAYOUT-SWEEP-IS-VOIDED-BY-ANY-COMMIT-MADE-WHILE-IT-RUNS`.
The sweep landed, so the block cleared.)

**In short:** we just adopted a rule saying that any number quoted to the
operator in `open-questions.md` has to be regenerable by a command they can run
themselves. Both of the tables that rule was written for compare benchmark runs
made under *different emulators*, and no such command exists — the tool has a
mode for comparing code layouts and a mode for listing records, but nothing that
compares two accelerators. So the rule is currently unsatisfiable for exactly
the evidence that motivated it, and both tables had to be re-checked by throwaway
scripts that leave nothing behind for the next reader.

#### What it is

`design-decisions.md` §240 adopted, after the Q53 contamination:

> evidence quoted in `open-questions.md` must be reproducible by a command the
> reader can re-run

`scripts/bench-history.py` offers `--list`, `--layout-bands`, and the record/diff
path. None of them produces an accelerator comparison. The two tables that need
one:

| Table | Claim | How it was produced |
|---|---|---|
| **Q54** "The evidence" | ×3.53 typical, 82/86 faster, `hpet_read` 453 → 13534 ns | hand-built in `design-decisions.md` §237 |
| **Q53** "Update 2026-08-19" | TCG vs WHPX layout-band widths | `build/whpx-band-preview.py`, which monkeypatches `bh`'s grouping key |

Q53's is the better of the two — it reuses `bh.layout_arm_rejection` unmodified,
which is why its WHPX column survived re-derivation intact — but it is a
throwaway script in `build/`, i.e. gitignored, i.e. not a command the operator
has.

#### Why this is debt and not a bug

Nothing is currently *wrong*. Both tables have now been checked by hand: Q53's
was corrected (`41581ca30`), Q54's was confirmed sound with one row amended
(`0d9da98c6`). The defect is that the check is not repeatable — the next person
to update either table, including a future me with no memory of today, gets no
help from the tooling and no warning if they select records the way the Q53
analysis did.

That is precisely the failure §240 describes. Writing the rule and not building
the thing that makes it satisfiable leaves the rule as an intention.

#### What the proper fix looks like

An `--accel-compare` mode on `scripts/bench-history.py`:

- Takes two accelerator names (or `--accel-compare A:B`), a profile, and
  optionally a `kernel_sha` or `src_digest` to pin the source.
- **Selects records through the same predicate the layout code uses**, not a
  fresh enumeration — §240 rule 2. Where the layout predicate does not apply
  (an accelerator comparison is not a band), the selection it *does* make must
  be printed, record by record, so the reader sees which runs went in.
- **Refuses to compare a single TCG arm against a single WHPX run without
  saying so.** The Q54 amendment found the typical speedup is ×3.53–×3.97
  depending on which of six layout arms is chosen, and that the "best case"
  benchmark is a different benchmark in four of the six. If a band exists for
  either side, the tool should report the range, not one arm's number.
- Emits markdown, so the table in `open-questions.md` is pasted output rather
  than transcribed output. Transcription is where a digit changes.

#### How to reproduce the gap

```bash
python scripts/bench-history.py --help    # no accelerator-comparison mode
```

Then try to regenerate Q54's table from the documented inputs. It cannot be done
without writing new code, which is the whole of this entry.

#### How it was fixed

```bash
python scripts/bench-history.py --accel-compare tcg:whpx --profile release     [--assume-missing-accel tcg] [--accel-pin PREFIX] [--markdown]
```

Each bullet of the spec above, and what it turned into:

- **Pinning.** Pairing is on `kernel_sha` — the SHA-256 of the ELF that was
  measured — not on `src_digest`, and `--accel-pin` accepts a prefix of either.
  A layout band groups arms built from one *source*, because they are
  deliberately different binaries; an accelerator comparison wants the opposite,
  the *same binary* run twice, and `kernel_sha` settles that by observation
  rather than by argument. It is also the only pin that works on this history:
  the six TCG sweep arms carry no `src_digest` at all and the six WHPX arms
  carry a `full:` one, so no digest match across the two sides can ever exist —
  while their `kernel_sha` values match pad for pad.
- **The shared predicate (§240 rule 2).** The three filters both selections
  agree on — wrong host, wrong profile, deliberately loaded host — now live once,
  in `measurement_mismatch`; `layout_arm_rejection` and `accel_run_rejection`
  both delegate to it and only word the answer for their own reader. The test
  suite asserts the equivalence over a matrix *and* reads the `MISMATCH_*`
  constants off the module, so a fourth filter added without a case fails the
  suite rather than going unexercised.
- **The selection is printed, run by run** — timestamp, commit, `kernel_sha`,
  pad, canary verdict, and whether the accelerator was recorded or assumed —
  followed by a "Not used" block that accounts for every record that did not
  make it, grouped by reason.
- **It never presents one arm as "the" number.** Where several binaries are
  shared, every headline row is a span (`x3.44 - x3.99`), the "benchmarks
  faster" row is a span, and the best-case row *refuses to name a benchmark*
  when the pairs disagree about which one won. Where only one binary is shared
  it prints a warning that names this exact defect and the sweep command that
  would replace it with a range.
- **Markdown.** `--markdown` emits the tables plus a collapsed `<details>`
  selection block, so `open-questions.md` gets pasted output. Both forms are
  ASCII throughout, including the `x` in `x3.53`: this prints to a console whose
  code page is not UTF-8, where a multiplication sign is mojibake.

#### The clause the data cannot satisfy, and what was done instead

The spec asks for Q54's six-arm range to be regenerable. It is not, from the
records alone: **19 otherwise-usable runs — including the entire TCG sweep the
table is built from — predate the `accel` field.** `parse_accel` deliberately
reports those as unknown rather than as TCG, and it is right to: the *first*
WHPX run in this history also predates the field, so "no banner" provably does
not mean "TCG", and a tool that inferred it would be wrong about a real record
here.

So the inference is not made. `--assume-missing-accel NAME` lets the reader
supply the fact the harness failed to record; it is an assertion by whoever
types it, exactly like `--host-load idle`, and every report resting on one says
so in its third line and repeats the flag in its own reproduce command. The
assumption cannot be verified, but it can sometimes be *falsified*, and that
half is enforced: a run whose own `experiment` banner names a different
accelerator is refused individually and listed. The 2026-08-19 16:15 probe is
banner-titled "WHPX vs TCG", so it names both and is refused under either
assumption — the safe direction, since an unusable record costs a pair and a
mislabelled one corrupts every figure its pair feeds.

Both tables were then re-derived against the new command:

| | Q54 as published/amended | `--accel-compare` |
|---|---|---|
| Without the assumption | — | **1 pair** (`1a278561f858d447`), typical **x3.53**, **83 of 86** faster, worst `hpet_read` **x0.03** |
| With `--assume-missing-accel tcg` | ×3.53–×3.97 over six arms, 82–83 of 86, best case a different benchmark in four of six | **7 pairs**, typical **x3.44 – x3.99**, **82–83 of 86**, best case a different benchmark in **4 of 7** — the same four Q54 names |

The unassumed row is worth keeping: it is a *different binary* from the one Q54
used, so its x3.53 is an independent replication of the published headline
rather than a restatement of it. `hpet_read` is the worst case in all seven
pairs, which is the one row placement does not move.

#### What it still cannot do

- It cannot catch a run that used a non-default accelerator and wrote nothing
  about it in its banner. Nothing can; that record did not record what it did.
  The only real fix is the one already in flight — every run recording `accel`,
  which runs from 2026-08-19 onward do.
- `bench/boot-history.jsonl` still records no `accel` at all, so boot timings
  cannot be compared across accelerators by this or any other command. That is
  gated on Q54 and tracked separately.
