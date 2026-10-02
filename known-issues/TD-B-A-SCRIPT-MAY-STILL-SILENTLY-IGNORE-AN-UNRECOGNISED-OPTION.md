## TD-B-A-SCRIPT-MAY-STILL-SILENTLY-IGNORE-AN-UNRECOGNISED-OPTION (lane B, 2026-09-10)

**In short:** Gate 22 now guarantees that `--selftest` and `--self-test` both
reach a script's self-test. It does NOT guarantee that a script refuses an
option it does not recognise. A mistyped flag can still be dropped on the floor
while the script runs its default action and exits 0.

**Why it is the same defect one size up.** The sixteen scripts fixed today were
an instance, not the category. The category is: an unrecognised option falls
through to the default action, which succeeds, so the command reports success
having done something other than what was asked. The next one will not be a
spelling of self-test -- it will be `--dry-run` on a script that only knows
`--dry`, and that run will do the real thing and say it went fine.

**Why gate 22 does not cover it.** Detecting "falls through to the default
action" statically means understanding each script's argument handling. A gate
that can only be approximated is one that gets argued with, and then bypassed.
Running all 58 scripts twice and diffing -- which is how today's instances were
actually confirmed, and how four false findings were caught -- takes minutes,
against under a second for the static check.

**The proper fix.** `selftestflag.unknown_options(argv, known=(...))` exists for
exactly this and is one line per script:

    unknown = selftestflag.unknown_options(sys.argv[1:], known=("--check",))
    if unknown:
        print(f"{NAME}: unrecognised option {unknown[0]!r}", file=sys.stderr)
        return 2

Three scripts use it today -- rustscan, check-recursive-locks and
check-selftest-flag-spellings. Converting the rest is mechanical but needs each
script's real option list, which is why it is not a sweep. **Trigger to do it:
the first time an unrecognised option is observed being ignored by any script
other than the ones already fixed.** Until then this is a known gap with a
named tool, not an open question.

Argparse-based scripts are already immune: argparse exits 2 on an unknown
option. The exposure is the ~74 scripts that parse `sys.argv` by hand.
