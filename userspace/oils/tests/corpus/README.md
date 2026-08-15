# osh↔bash differential corpus — **scope-capped, read this first**

Cases here are run by `scripts/osh-bash-diff.py` through both `osh` and a
reference bash 5.2.37, comparing stdout, stderr and exit status byte for byte.

## ⛔ This corpus does not grow for its own sake

**design-decisions.md §305 (operator decision, 2026-08-14) froze osh's
bash-fidelity scope.** The cases already here are a **regression floor to
protect, not a number to raise.**

The reason: **GNU bash 5.2 itself cross-compiles and runs on SlateOS** — it has
since 2026-07-22 (`scripts/bash-spike/`, proven each boot by
`kernel/src/proc/spawn.rs::self_test_bash_on_slateos_libc`). The premise that
made byte-for-byte reimplementation parity worth chasing — that we could not
have real bash — has been false for almost the whole time this corpus has
existed. §305 records that history in full; it is worth reading once.

## When to add a case

Add one only if the divergence it pins meets at least one of:

- **something we ship or run actually hits it** — a real script, service, init
  file, build step, package recipe or interactive session, not a hypothetical;
- **it is a bug on its own terms** — crash, hang, data loss, security, or a
  wrong exit status that propagates;
- **it is a regression** against a case already green here.

## When *not* to add a case

- **Diagnostic wording**, spelling, or the exact substring a message echoes.
- **Artifacts of bash being a 40-year-old C program.** The canonical example is
  already in this directory: `OPTIND=4294967297` wraps to the first argument
  because bash stores it in a C `int`. That is a fact about `int`.
- **Constructs reachable only by adversarial or nonsense input** whose only
  observable difference is which error text appears.

If you found a real divergence that fails the criterion, annotate its
`TD-OILS-*` entry in `known-issues.md` with `SCOPE: out of frozen scope (§305)`
and move on. If something truly needs exact bash semantics, **run bash.**

## What this corpus is still for

Proving you have not **broken** anything. Run the full sweep after any change to
`userspace/oils`:

```sh
python scripts/run-timeout.py --poll 60 2400 python scripts/osh-bash-diff.py
```

A green sweep is the contract. Use it to protect the floor — not to hunt for new
differences to fix.

## Magic comments recognised by the harness

    # STDIN: <text>          feed <text> plus a newline to the shell's stdin
    # EXPECT-DIFF: <reason>  known divergence; fails if it stops differing
    # TIMEOUT: <seconds>     raise this case's 20s default budget

## A hazard worth knowing

bash's parse-level `${ … }` scan (`parse_matched_pair`, `P_DOLBRACE`) **does**
treat `'` as a quote opener. A corpus line like `echo "[${arr['x${m']}]"` must
therefore contain an **even** number of `'` inside the `${…}`, or the run spills
into the following line and the whole script dies with
``unexpected EOF while looking for matching `''``.
