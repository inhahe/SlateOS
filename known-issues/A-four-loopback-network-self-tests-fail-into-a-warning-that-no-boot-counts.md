### [A] Four loopback network self-tests fail into a WARNING that no boot counts -- 2026-09-26

**Status:** OPEN — lane A's; to be converted after the next boot that reaches them (trigger below).

**In short:** four of the boot's network self-tests run entirely inside the
machine, over the network daemon's software loopback, so an error from any of
them is a real bug rather than a network hiccup. Each still ends in a
`WARNING:` line, and the boot test does not count a `WARNING:` as a failure --
so a regression in any of them would pass the boot, printed where nobody
looks.

**Where.** `kernel/src/proc/spawn.rs`, `run_persistent_netstack`: the `Err`
arms of `net::socket::self_test_server` (the object-layer server socket),
`netstack_client::self_test_connect6`, `self_test_udp6_loopback` and
`self_test_udp_connect`. `scripts/boot-test.sh` reds a run only on
`self-test failed` (`check_selftest_failures`).

**Why they were not converted with listen/accept (2026-09-26).** The
listen/accept check was converted in the change that fixed lane F's
listener-`poll` bug, because that change is what it tests. These four have
not been seen to run lately: `run_persistent_netstack` is the last thing
before `BOOT_OK`, and no serial log on this machine from the last four days
(lanes A, C, D, E) gets that far -- every one of those boots was starved or
stuck earlier. Converting them blind would put four unseen results in the way
of the integration boot that carries the scheduler fix; the boot that reaches
them shows whether each passes first.

**The proper fix.** Each `Err` arm prints a `FAIL:` line and calls
`crate::selftest::dispatch_debug(<name>, Severity::Diagnostic, Err(e))`, as
the head-of-line witness and listen/accept now do. The head-of-line comment's
distinction -- "a warning is right for a parity check that can be defeated by
the environment (no lease, no IPv6 peer)" -- is already honoured by their
`Ok(None)` skip arms; an `Err` from a loopback check is not environmental.

**Trigger.** The first boot whose serial log contains
`[spawn]   persistent netstack listen/accept:`. If any of the four printed a
`WARNING: ... error` there, that is a bug to fix before converting it.
