## `A-KSHELL-A-DIAGNOSTIC-CAN-NAME-A-COMMAND-IT-DID-NOT-COME-FROM` (lane A, 2026-08-29) — **fixed, and gated**

**In short:** the shell's error messages start with the name of the command
that produced them — `epollstat: pid: 'x' is not a pid`. That name is typed
into the source by hand, next to the code that uses it, and nothing checks it
against the command it is actually written in. Copy a few lines from one
command into another and the copy keeps saying the *first* command's name. The
message is then fluent, specific, correct in every other respect, and about the
wrong program — which sends whoever reads it to the wrong file. It has now been
made impossible: a build-time check compares each name against the shell's own
dispatch table.

**How it happened, which is the interesting half.** Batch 40 of the §600
guess-a-value burn-down converted twenty functions in one sitting. Several were
done with a whole-file search-and-replace of an identical statement:

```rust
let pid = parts.get(1).copied().unwrap_or("0").parse::<u32>().unwrap_or(0);
```

That statement was **not unique to the function being edited**. Seven arms
across `cmd_filelock`, `cmd_netsock`, `cmd_pipestat`, `cmd_schedclass`,
`cmd_taskstats`, `cmd_hdrdisplay` and `cmd_dpiscaling` were rewritten too. The
rewrite was *correct in substance* — each of those arms documents its operand
as required and each was guessing at it — so the conversions were kept. What
was wrong was one string: they announced themselves as `epollstat` and
`displayarrange`.

**Nothing downstream noticed, and it is worth being precise about why:**

| Check | Verdict | Why it could not see it |
|---|---|---|
| `cargo build` | clean | the literal is a `&str`; every `&str` is a valid `&str` |
| `cargo fmt` | clean | formatting is unaffected by a string's contents |
| `check-option-refusal.py` | **counted them as fixed** | by its own measure they *were* fixed — the `unwrap_or` was gone |
| self-test rungs | silent | no rung asserted on those arms' text |

So the single visible trace was in the wording of a message that no test reads,
in code that every gate had just scored as an improvement.

**Why this is the same defect one level up.** The whole point of the operand
helpers is to stop the shell answering a question it could not read with a
confident, specific, invented answer. A diagnostic that names the wrong command
*is* a confident, specific, invented answer — about provenance instead of about
a value. The fix reproduced the bug it was fixing, inside its own machinery.

**The gate: `scripts/check-shell-message-names.py`.** Decidable rather than
heuristic, because the shell already writes down the answer. `kshell.rs`
dispatches on the typed word:

```rust
"webcam" | "cam" => cmd_webcam(args),
```

so the set of names a function may legitimately call itself is exactly the set
of literals in its own dispatch arm. That is also why the obvious rule — "the
literal must equal the `cmd_` suffix" — is wrong: `cmd_vdesktop` correctly says
`vd` and `cmd_webcam` correctly says `cam`, because those are the short names
their usage lines use, and both are in the arm.

It checks `required_num`, `optional_num`, `readable_num`, `readable_hex` and
`end_help_arm`, and it **starts at zero** — 523 name-bearing calls across 749
dispatch entries, no mismatch once the seven were corrected. Per
design-decisions.md §635 that is the bar for a new gate; had it needed a
baseline, it would have been narrowed until it did not. A `cmd_` function with
no dispatch arm is reported too: a name-bearing call there has no set of
legitimate names to be checked against, and was almost certainly copied in.

Wired into `scripts/boot-test.sh` fixture-first, like its siblings — the four
fixtures (a clean tree with aliases, a name copied from another command, a name
that dispatches to nothing, a function with no dispatch arm) are graded by
`--self-test` before the real file is inspected, so a collapsed checker cannot
report a clean tree in the same words as a clean tree.

**The lesson that generalises past this file.** A whole-file `replace_all` is
safe only if the text is unique to the target, and *the way to find out is to
count the matches first* — which is cheap, and which was done for four of the
six replacements in that batch and skipped for two. The two that were skipped
are the two that leaked. More generally: when a mechanical edit carries a
*name* along with the code, the name is the part that will be wrong, because it
is the only part the compiler is not reading.

Self-test rung 104 asserts the corrected case in the serial log as well
(`filelock pid 1O` must name `filelock` and must not contain `epollstat`), so
the invariant is pinned both statically and at runtime.
