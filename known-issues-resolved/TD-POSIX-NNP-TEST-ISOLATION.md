### TD-POSIX-NNP-TEST-ISOLATION. 29 `NO_NEW_PRIVS` tests shared genuinely-process-global state and raced under the parallel harness — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**Symptom.** With the parallel test harness,
`unistd::tests::test_phase160_repeated_get_no_set_returns_zero`
(`posix/src/unistd.rs:4771`) intermittently failed: `PR_GET_NO_NEW_PRIVS`
returned 1 where the test had never set it.

**Root cause — deliberately *not* the same as TD-POSIX-TEST-PARALLEL.** This
one is not a product thread-safety gap. `NO_NEW_PRIVS` is backed by a single
process-wide `AtomicBool`, and that is *correct*: it mirrors Linux's
`task->no_new_privs`, a per-process (not per-thread) security bit that is
one-way latching. 29 tests in `unistd.rs` mutate and read that one bit, so the
harness running them concurrently lets one test's `PR_SET_NO_NEW_PRIVS` land in
the middle of another's `PR_GET_NO_NEW_PRIVS`. Making the bit per-thread would
have "fixed" the suite by breaking the product.

**✅ RESOLVED 2026-07-30.** Added a test-only `nnp_guard()` returning a
`MutexGuard` over a private `static LOCK: Mutex<()>`, and took it as the first
statement of all 29 affected tests. The guard's doc comment records explicitly
why serialising is the right fix here and why per-thread storage would be
wrong, so a future reader doesn't "improve" it into a bug. Verified by 10
consecutive clean parallel runs of the full posix suite (20013 tests each).

> **⚠️ Superseded 2026-08-12 — the "root cause" above is wrong where it
> matters, and the fix did not work.** The premise that serialising is
> sufficient is false: the mutex lives in `unistd`'s *own* test module, so it
> can only serialise `unistd`'s 29 tests, while `linux_seccomp` and
> `linux_landlock` drive the same bit through `_test_reset_no_new_privs`
> without ever taking it. A 40-run hunt duly failed on
> `test_seccomp_phase186_filter_with_nnp_no_cap_reaches_enosys`. The claim that
> "making the bit per-thread would have fixed the suite by breaking the
> product" is also wrong: the *host* build can store it per-thread while the
> target keeps the `AtomicBool` untouched, so the product semantics are
> unchanged. The guard's doc comment — written to stop a future reader
> "improving" it — is what kept the real bug alive for two weeks. Both the
> guard and the two `NnpGuard` copies are gone; see
> TD-POSIX-TEST-SHARED-STATICS-REMAINING-TIER.
