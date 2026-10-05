### BUG-BOOTTEST-BOOTOK-SUBSTRING. `boot-test.sh` reported PASSED on a livelocked boot because its `grep -q "BOOT_OK"` matched the substring inside the livelock diagnostic "…still armed 200s after arming (no BOOT_OK)…" — 2026-07-22 — ✅ RESOLVED 2026-07-22

The success marker is printed as a standalone line (`serial_println!("BOOT_OK")`),
but the detector used an UNanchored `grep -q "$WAIT_MARKER"`.  The liveness
watchdog's `BOOT DEADLINE EXCEEDED … (no BOOT_OK)` message contains the substring
`BOOT_OK`, so on a genuine livelock (the fastpy-nice tool once busy-spun at a
raised priority and starved the harness) the harness still printed
"BOOT_OK detected … Boot test PASSED".  Fixed by anchoring all four match sites
to line start (`grep -q "^$WAIT_MARKER"` / `^BOOT_OK`) in `scripts/boot-test.sh`,
so only the standalone marker line counts.  Found while validating the
fastpy-nice (nice→priority) self-test.
