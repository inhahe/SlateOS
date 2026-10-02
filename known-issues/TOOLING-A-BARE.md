### [A] TOOLING-A-BARE-`bench-history.py`-RECORDS-A-RUN-AND-MISLABELS-ITS-PROVENANCE — 2026-08-15 — ⚠️ OPEN (low severity, easy to trip)

**Status:** open. Hit while validating `level_shifts()`; the bad record was
caught within the minute and reverted with `git checkout -- bench/history.jsonl`,
so no bad data reached a commit. Recorded because the failure is silent and the
file it damages is the baseline every performance verdict is measured against.

**What happens.** `python scripts/bench-history.py` with no arguments is a
natural thing to type when you want to *read* the analysis — and it **writes**.
It parses whatever is in `build/serial-test.txt` and appends a record, because
recording is the intended behaviour when `boot-test.sh` invokes it after a
`--bench` boot. There is a `--no-record` flag; the default is the destructive
one.

**Why the record is worse than a mere duplicate.** Two fields are taken from
the *invocation*, not from the log being parsed:

- `profile` defaults to `LEGACY_PROFILE` (`debug`), and
- `commit` comes from the current git HEAD.

So the accidental run took release-profile numbers from an older boot and stored
them tagged `profile=debug`, `commit=<today's HEAD>`. Nothing validates that
pairing, and nothing could: the serial log does not record which profile or
commit produced it. Since `comparable_records()` partitions strictly by profile,
a mislabelled record does not merely add noise — it lands in the *wrong
partition*, where it is a baseline for numbers it has no relationship to.

**Reproduce:** with any `build/serial-test.txt` containing a scorecard, run
`python scripts/bench-history.py` and diff `bench/history.jsonl`.

**Proper fix** (not "add a warning"): make the provenance travel with the data
rather than with the command line. Have the kernel print the build profile and
the commit hash into the scorecard header at boot, and have `parse_serial()`
read them; then `--profile` becomes an override that must *agree* with the log,
and a disagreement is an error rather than a silent relabel. Until then the
lesser mitigation is to make recording explicit (`--record`) so the read-only
use is the default, which is the safe direction for a tool whose output is
advisory but whose side effect is permanent.
