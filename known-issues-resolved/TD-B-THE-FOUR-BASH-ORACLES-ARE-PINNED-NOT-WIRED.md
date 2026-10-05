## TD-B-THE-FOUR-BASH-ORACLES-ARE-PINNED-NOT-WIRED (lane B, 2026-09-03) -- RESOLVED

**RESOLVED 2026-09-03.** All four steps of the proper fix below landed, in the
order it prescribed, and the four `PINNED` entries were deleted in the same
commit that wired the gates. `boot-test.sh` now runs them as
`check_bash_oracles`: the **gates** carry `--may-skip`, so a host without WSL
skips them loudly instead of failing the build, and the **self-tests do not**,
because a self-test reads only fixtures the checker carries in its own source
and therefore needs no WSL by construction. That asymmetry is the point — on a
WSL-less host these four still check their own tables, their floors, the port
of `shellquote.rs` and the transcription of the rung literals. Only the half
that genuinely requires bash skips.

Two things were learned doing it that were not in the plan:

- **A floor breach must not be skippable, and is not.** `--may-skip` only
  accepts exit 2; a gutted table raises `SystemExit` and exits 1, so it still
  aborts the build with the flag present. Verified for all four.
- **The decline must be the checker's *first* line of output**, because
  `run_checker` takes that line as the reason it skipped. Two of the four
  printed a success line ("port verified against shellquote.rs") before the
  transport check, which would have made the reason a gate did not run read
  like a pass. It survived by accident — stdout block-buffers into the log and
  stderr does not — which is the kind of accident that ends the day someone
  adds `-u`. Both now print their preconditions' success *after* the transport
  check, while still *running* them before it, so a drifted port is still a
  finding on a host that cannot ask bash anything.

Kept rather than deleted because the reasoning below is what a future pin
should be argued against, and because the "three defects" list is the record of
why an unrun gate was preferred to a wrongly-wired one for as long as it was.

**In short:** four checkers that verify kshell's quoting rules against *real
bash* are not run by anything. They are now pinned as deliberately-unwired so
the build is honest about it, but pinned is not the destination — they should
run wherever WSL exists. Two specific defects block wiring them.

`scripts/check-ansic-quoting-vs-bash.py`, `check-kshell-pipeline-vs-bash.py`,
`check-kshell-rungs-vs-bash.py`, `check-shellquote-vs-bash.py`, all via
`scripts/bashprobe.py`.

**How this surfaced.** They shipped unwired, and on 2026-09-03 lane B's own
wiring ratchet (`check-gates-are-wired.py`) started reporting them — which
turned `main` red for all three lanes, because that ratchet runs before the
boot test builds anything. Lane C filed
`requests/c-b-four-of-your-new-shell-gates-are-unwired-and-main-is-red.md`
rather than pinning them itself, on the correct grounds that a pin whose reason
is "nobody has looked at this" is precisely what the ratchet exists to prevent,
and only lane B had the real reason. The pin landed with that reason; this
entry is the other half of it.

**Why they cannot simply be wired.** Three defects, in the order they bite:

1. ~~**An absent WSL is reported as a finding.**~~ **DONE 2026-09-03.**
   `bashprobe` now raises `NoBash` and declines via exit 2 with a spoken
   reason that does not begin `usage:`. Original text follows.
   `bashprobe.assert_transport_is_faithful()`
   leaves via `raise SystemExit(msg)`, which exits **1**. So "this host has no
   WSL, I could not ask bash" arrives in the same channel as "bash disagrees
   with kshell" — a machine with no WSL is told its shell quoting is wrong. This
   is exactly the no-verdict-vs-finding confusion that gates 2, 3, 4, 6 and 11
   were converted away from (`TD-B-PRE-PUSH-GATES-2-6-8-11-JUDGE-THE-WORKING-TREE-NOT-THE-PUSH`),
   and it is the first thing to fix regardless of wiring, because it is wrong
   even when the gate is run by hand. It must exit **2**.
2. ~~**`run_checker` aborts the build on any exit but 0 or 1.**~~ **DONE
   2026-09-03.** `run_checker --may-skip <label> …` is in, with group 9 of
   `scripts/test-pre-push-run-checker.py` behind it and design-decisions.md
   §753 recording the tradeoff. An unflagged exit 2 still aborts, which is what
   lane A asked to keep. Note for step (1): the skip arm rejects a decline whose
   output carries a `usage:` banner, because **argparse also exits 2** — so
   whatever bashprobe prints on a missing WSL must not begin `usage:`, or the
   gate will abort rather than skip.
3. ~~**None of the four has a `--self-test`.**~~ **DONE 2026-09-03.** All four
   have one, each with a true-positive fixture and a floor; the shellquote
   one found a real hole while being mutation-tested — the file pinned
   `DQ_ESCAPABLE` against the Rust and nothing checked that the scanner ever
   *read* it. Original text follows.
   Lane C's request flags this from
   direct experience: three of the five gates it wired the same day shipped an
   unrun `--self-test`, and a scanner that has stopped scanning reports zero
   findings exactly as a clean tree does. These four scan `kernel/src/kshell.rs`
   and `kernel/src/shellquote.rs` by regex for literals, which is the shape most
   prone to silently matching nothing.

**Proper fix**, in that order: (1) bashprobe exits 2 with a "no verdict"
message when WSL is absent or the transport is broken; (2) `--may-skip` in
`run-checker.sh`, with cases in `test-pre-push-run-checker.py`; (3) a
`--self-test` per gate, each with a true-positive fixture proving it can still
refuse, plus a floor so a gate that inspected nothing says so; (4) wire all
four into `boot-test.sh` and delete the four `PINNED` entries in the same
commit, which is what that dict is for.

**If it is never fixed:** kshell's quoting rules are verified against bash only
when someone remembers to ask by hand, on a machine that happens to have WSL.
The verdicts are carried forward into kshell's self-test rungs, so the evidence
does survive — but only the evidence gathered on the day the rule was written.
A later edit to `shellquote.rs` that changes behaviour is caught by nothing.
