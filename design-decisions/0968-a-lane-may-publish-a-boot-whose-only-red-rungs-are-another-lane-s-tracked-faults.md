## 968. A lane may publish a boot whose only red rungs are another lane's tracked faults that `main` already has

**Date:** 2026-09-26 &middot; **Decided by:** Claude (operator-approved scope) -- A-Q20 put the question to the operator; on 2026-09-26 the operator left it to Claude ("I'll let you do what you think is best"), for lane A's case in hand &middot; **Lane:** A

**In short:** a lane may only copy its work into the shared main line after
its full test run passes. Lane A's runs have been failing for days on tests
that fail because of bugs in another lane's code -- bugs the shared main line
already has -- so nothing lane A did in a week, including fixes the other
lanes are waiting for, reached anyone. A lane now publishes in that one
situation, under three conditions it must check and write down, and in no
other.

**What was decided.** A lane may publish a tested commit whose boot is red only
when all three hold, checked and stated in the publishing note:

1. **Every red rung is another lane's fault, and tracked.** Its cause is in
   code the publishing lane does not own, with an open request to the owner.
2. **`main` already has it.** The rung fails on `main` too for the same image
   contents, so publishing introduces no failure `main` does not already have.
3. **The lane's own failures are zero.** Anything of its own that was red is
   fixed, and the boot being published shows it fixed.

Everything else about publishing stays as it was: merge `origin/main`, boot the
merge, fast-forward push of the sha that was booted (§538).

**The case that decided it.** Lane A's integration boot of `3fd70ae1d` was red
on four counts. Two were lane A's own -- a VFS lock-order inversion that a
brittle kshell rung turned into a kernel panic, and a ring-3 ABI probe left
behind by a struct that grew -- and are fixed. Two are lane D's libc, and fail
on `main` for any image that carries Python and CMake:

| rung | cause (lane D's code) | on `main` |
|---|---|---|
| `ctest-python-repl` | `execl` starts programs with an empty environment; fixed on `lane-d`, not published | red: exit 8, since lane A's grant fix for the rung is only on `lane-a` |
| Path-Z real CMake | `__cxa_atexit` drops the object pointer, so static destructors run on `NULL` | red: the stub is on `main` |

The first is also a circular wait: lane D's `execv` fix cannot pass lane D's
boot until lane A's grant fix is on `main`, and lane A's boot cannot pass until
lane D's fix is.

**Alternatives considered.**

| option | why not |
|---|---|
| Keep the strict rule | lane A's week of work -- including hang fixes other lanes' boots need -- waits on lane D's libc, while lane D's fix waits on lane A |
| Quarantine the rungs: skip them, or boot an image without Python and CMake | hides the failures; the report would say less than the truth, and `main` would look greener than it is |
| Cherry-pick lane D's unpublished fix | takes another lane's untested work in through a side door, outside the publish protocol |

**How to reverse.** Return to the strict rule; nothing structural depends on
this. Revisit if a publish under it ever turns out to have introduced a
failure -- condition 2 exists to make that impossible, so one would mean the
check was done wrong.

**Where this bites:** `CLAUDE.md` "Branch Strategy" and `roadmap.md` rule 6
still state the strict rule; this entry is the exception, cited from each
publish that uses it.

**Publishes under this rule.**

| date | published | boot | red rungs (owner, tracked by) | on `main` too? | lane A's own |
|---|---|---|---|---|---|
| 2026-09-26 | `6fe46eec2` (`d3741b26b` + its boot-history row), `main` fast-forwarded from `d2f434288` | integration, debug, BOOT_OK after 1212 s | `ctest-python-repl` exit 4 (lane D: `execl` passes no environment, `requests/a-d-execv-execvp-execl-execlp-start-the-new-program-with-no-environment.md`); Path-Z CMake exit -8 (lane D: `__cxa_atexit`, `requests/a-d-cxa-atexit-drops-the-object-so-static-destructors-run-on-null.md`) | yes: the REPL rung is worse there (exit 8, no grant), and the `__cxa_atexit` stub is on `main` | 0 -- lockdep 0 real, kshell self_test PASSED (rung 21 included), 21 spawn_ex2 probes OK, no panics |

That push also carried `ALLOW_STALE_RELEASE=1`, and it is recorded here so that
it is not a silent waiver. The release-staleness gate counted 101
"kernel-touching" commits against a threshold of 100. Its 101st was the
boot-history row of the very boot being published: the gate counted `bench/`,
and 29 of the 101 were boot records. Only 72 touched `kernel/`. The gate now
excludes records (`6ff91aa8a`), and lane E's release-profile boot of `main` is
the one that resets the baseline.
