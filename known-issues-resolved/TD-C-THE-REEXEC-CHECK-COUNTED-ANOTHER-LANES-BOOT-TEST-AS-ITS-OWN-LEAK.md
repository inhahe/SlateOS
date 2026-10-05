## TD-C-THE-REEXEC-CHECK-COUNTED-ANOTHER-LANES-BOOT-TEST-AS-ITS-OWN-LEAK (lane C, 2026-09-25) -- FIXED the same day

**In short:** every boot test first checks that its own start-up trick -- run
from a copy of the script, so an edit made mid-run cannot corrupt it -- still
works, and that the copy is deleted afterwards. The "deleted afterwards" half
counted the copies in the machine's shared temporary folder before and after,
and blamed any extra one on itself. Six lanes run boot tests, so when another
one started during those few seconds, its copy was counted, and the boot test
refused to build: lane C's boot of `02bc887ba` stopped at 7 minutes with "1
snapshot(s) left behind; the trap did not fire". The copy belonged to a boot
test another session had started at 22:41:15, while the check ran.

**Where:** `scripts/check-boot-test-reexec.sh`, the leak check at its end.

**Fixed:** the check's trial run gets a temporary folder of its own
(`TMPDIR`, which the preamble's `mktemp -t` honours), and only that folder is
counted -- nothing else writes there, so no other run can be miscounted. To
keep the check from passing for a preamble that ignored `TMPDIR`, it also
records that the copy *was* made there while the trial ran. Checked three
ways: clean; with a snapshot made in the shared folder mid-check (the race,
now passes); and with each half broken -- no `TMPDIR`, and a copy left behind
-- both refused.

**The general shape**, since it will recur: a check that measures shared state
before and after an action attributes to the action whatever else changed
that state in between. With six lanes on one machine that is not rare; the
fix is to give the action state no one else touches, not to compare harder.
