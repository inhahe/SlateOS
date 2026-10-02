### [B] D-POSIX-TIMED-WAITS-DID-NOT-VALIDATE-TV-NSEC — ✅ FIXED 2026-08-14

`pthread_cond_timedwait`, `pthread_mutex_timedlock` and `sem_timedwait`
accepted any `timespec` whatsoever. A `tv_nsec` of `1_000_000_000` or `-1` —
the classic result of adding a nanosecond offset without carrying into
`tv_sec` — should be `EINVAL` (glibc `valid_nanoseconds`, `include/time.h:517`);
instead it fell through to the deadline comparison, where a too-large
`tv_nsec` silently extended the wait by up to a second and a negative one made
the call return `ETIMEDOUT` immediately. Both are wrong in the direction that
hides the caller's bug. Separately, `mqueue::deadline_from_timespec` checked
`tv_nsec` but not `tv_sec < 0`, which the kernel's `timespec64_valid` rejects.

Fixed by adding `time::valid_nanoseconds` (glibc's predicate, verbatim) and
calling it from each site **at the position its own upstream uses** — eagerly
in `pthread_cond_timedwait` and `sem_timedwait`, lazily (contended branch
only) in `pthread_mutex_timedlock` — plus the missing `tv_sec` half in
`mqueue`. See the ninth-pass write-up under
`D-POSIX-NULL-POINTER-ERRNO-NEEDS-A-PER-FUNCTION-AUDIT` for why the three
placements differ and why the mqueue predicate is not the same predicate.

Seven tests pin the distinctions, including the two that would silently pass
under a naive "check it at the top of every function" fix:
`test_pthread_mutex_timedlock_uncontended_ignores_a_bad_deadline` and
`test_sem_timedwait_checks_the_deadline_before_the_fast_path`.

**Not fixed, because we do not have them:** `pthread_cond_clockwait`,
`sem_clockwait` and the `pthread_rwlock_{timed,clock}{rd,wr}lock` family are
unimplemented. When they are added they need the same predicate plus
`futex_abstimed_supported_clockid`, and the rwlocks check **eagerly** — see
the comment at `pthread_rwlock_common.c:286-291`.
