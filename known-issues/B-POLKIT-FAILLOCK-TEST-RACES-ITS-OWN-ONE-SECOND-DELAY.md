## B-POLKIT-FAILLOCK-TEST-RACES-ITS-OWN-ONE-SECOND-DELAY (found by lane C, 2026-08-22 — lane B's crates)

**In short:** One test in `userspace/polkit` earns a rate-limit delay of exactly
one second and then does a deliberately-slow password hash inside that second
before checking the delay is still in force. On a busy machine the hash outlasts
the delay and the test fails. It passes every time when run alone.

**Where.** `userspace/polkit/src/main.rs:1806`,
`tests::polkit_honours_a_delay_earned_at_another_prompt`. The delay arithmetic is
`authlib::delay_for` (`userspace/authlib/src/lib.rs:413`) and the clock is
`wall_clock_secs` (`:429`), both lane B's.

**Why one second.** The test's loop is
`while elsewhere.rate_limited("alice").is_none() { elsewhere.note_failure("alice") }`,
which stops at the *first* iteration that produces a delay — `FREE_ATTEMPTS + 1`
failures, so `delay_for` computes `1 << 0` = 1 s. The clock has whole-second
granularity, so the real window is 0–1 s depending on where in the current second
the loop landed. `admin_with_password` → `set_password_with_salt` then runs a real
KDF inside it.

**How it was found, and why it mattered to lane C.** `cargo test --workspace`
stops at the first failing binary, so this failure meant `apps/settings` — the
crate under test in the task that ran the gate — was never run at all. See
`TD-C-A-TEST-BINARY-CAN-BE-BROKEN-WITHOUT-ANYONE-NOTICING`; this is a second,
independent way for a workspace gate to report nothing and look green-ish.

**The same bug is in `ftpd`.** `ftpd::tests::repeated_guesses_are_rate_limited`
(`userspace/ftpd/src/main.rs:3230`) writes the same count out longhand —
`for _ in 0..=authlib::FREE_ATTEMPTS` — and so also earns exactly one second,
then spends it on `FREE_ATTEMPTS + 1` real shadow verifications before asserting
the limit is still in force. It failed on the run where `polkit` passed, which is
how the family was identified rather than the instance.

**Filed to the owning lane** as
`requests/c-b-three-flaky-tests-fail-the-workspace-gate.md`, with three suggested
fixes — the right one being to inject a frozen clock, since the property under
test in both crates ("a delay earned at another prompt is honoured here", "the
limit is daemon-wide and survives reconnecting") has nothing to do with wall
time. Lane C has not touched either file.

**Also noted there, not the cause:** the test's scratch directory is a *fixed*
path, `temp_dir()/polkit-faillock-share-test`, which it opens by deleting. Two
concurrent `polkit` runs would delete each other's fixture mid-test.

**Workaround until fixed:** run the gate as
`cargo test --workspace --no-fail-fast --target x86_64-pc-windows-gnu`.
