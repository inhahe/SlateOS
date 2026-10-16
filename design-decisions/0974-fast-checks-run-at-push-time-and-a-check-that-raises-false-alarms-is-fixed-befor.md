## 974. Fast checks run at push time, and a check that raises false alarms is fixed before it is moved there

**Date:** 2026-09-27 · **Decided by:** Operator (Claude recommended this option) · **Lane:** A

Answering A-Q13. Relayed by lane F, 2026-09-27; verbatim:

> The first, and see if you can "fix the checks" too.

**In short:** when one agent pushed something broken, the fault was found only
by another agent's full test run. That run was 20 to 40 minutes when the
question was filed and is over three hours now. The operator chose to move the
quick checks into the check every push must pass, so the agent who wrote a
fault sees it within minutes. The checks themselves are to be fixed wherever
they raise false alarms, since a check that cries wolf teaches agents to
ignore it.

**What it obliges.**
1. **An inventory.** It lists every gate the boot test runs that the push hook
   does not (72 against 48 on 2026-09-27). For each it gives:
   - its measured cost, from the per-run gate-timing files;
   - whether it judges the commit being pushed or the working tree.
2. **Each fast, deterministic gate moves into the hook**, scoped to the paths a
   push touches, the way the hook's other gates are. The slow ones stay in the
   boot and are what the gate cache (C-Q11 idea 2) is for.
3. **"Fix the checks."**
   - Every recorded false alarm is found in `known-issues.md` and the boot
     history, and confirmed fixed. A false alarm is a gate refusing a correct
     tree.
   - A gate moves into the hook only with a self-test that proves it refuses
     what it should and passes what it should. The hook already runs those
     self-tests before it trusts a verdict.
4. **Cost against catches, as the operator's C-Q11 answer asked.** For each
   gate, record its cost per run against the faults it has actually caught, so
   a gate that costs much and catches nothing is visible rather than assumed
   useful.

**Measured, 2026-09-27**, with `scripts/gate-cost-report.py`. It reads
every boot's per-gate timing file in all six worktrees, and it counts only
the boot test's own gates, so fixture rows are excluded.
- **Total.** Across 395 boots, the gates `run_checker` times cost 133.9
  gate-hours.
- **Never refused.** 98 of the 131 gates never refused once, and they cost
  73.4 of those hours (55%).
- **The most expensive.** `check-live-counter-reads` cost 11.2 hours over 335
  boots, and `check-selftest-reach` 9.3 hours over 267. Neither ever refused.

Two limits on what that says:
- **A refusal is a catch or a false alarm.** The report lists the dates so a
  person can tell which.
- **Zero refusals is not zero value.** A gate can prevent faults its authors
  fixed before any boot.

So this retires nothing. It says where the gate cache (C-Q11 idea 2) earns
most: these gates' inputs rarely change between boots. The tooling suites,
about 5800 s a boot, do not run through `run_checker` and are not in these
numbers.

**Done 2026-09-27: the first half.**
- 22 of the 53 boot-only gates, those with a median of 15 s or less that judge
  source, are push gates 53-74 (52-73 until main's binary-collision
  gate took 52 the same week).
- Each runs only where the working tree is the push.
