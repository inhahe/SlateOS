### TD-OILS-DUP-ONTO-A-NON-STD-FD-IS-WRONG. `N<&M` never checks its source, `N>&M` retargets stdout, `N>&N` fails — ✅ RESOLVED — 2026-08-01

**Where:** `userspace/oils/src/interp.rs`, `resolve_dup_in` (the
`DupWord::Fd(n)` arm, which acts only `if fd == 0 && n >= 3`) and
`resolve_dup_out` (its final `else if let Some(n) = target_num` arm, which
validates `n` unconditionally and then writes `plan.stdout_to_fd` for every
redirector that is not 2).

**Reproduce** (fd 9 closed, `exec 5>f5` first):

```sh
read -r l 7<&9    # bash: 9: Bad file descriptor    osh: silent, rc=0
read -r l 7<&"9"  # bash: 7: Bad file descriptor    osh: silent, rc=0
read -r l 7<&7    # bash: silent, rc=0              osh: silent, rc=0  (agrees)
echo hi 7>&007    # bash: prints hi, rc=0           osh: 7: Bad file descriptor
echo hi 7>&5      # bash: prints hi, f5 empty       osh: prints nothing, f5=hi
```

**Three separate faults, all in the same corner — a dup whose *redirector* is
not the operator's own standard descriptor:**

1. **The input side does not check.** `resolve_dup_in` only looks at the source
   when the redirector is fd 0, so `7<&9` is silently accepted where bash
   reports `EBADF` and abandons the command. (The message it should print is
   already covered: `Shell::dup_error_subject(r, fd, 0)` gives bash's text.)
2. **A self-dup is checked when it should not be.** bash skips the `dup2`
   entirely when the redirector equals the source, so `7>&7` and `7<&7` succeed
   even though fd 7 is closed — there is nothing to duplicate. osh validates the
   source first and so rejects `7>&007`.
3. **The output side redirects the wrong descriptor.** `plan.stdout_to_fd` is
   set for any redirector other than 2, so `7>&5` reroutes *stdout* to fd 5
   instead of aliasing fd 7. The `AliasStd` arm just above handles `3>&1` and
   friends correctly; this arm has no equivalent for a non-standard source.

**Impact.** Fault 1 loses a diagnostic and lets a doomed command run; fault 3
actively corrupts a redirect, though only for a shape (`N>&M`, both ≥ 3, outside
`exec`) that is rare. Fault 2 is a spurious failure. None is exercised by the
corpus — `tests/corpus/dup-word-that-is-not-a-descriptor.sh` documents in its
header why it stays away from these shapes.

**Proper fix.** Give the output arm the same `fd >= 3` treatment the `AliasStd`
arm has, extending `ExtraFdOp` so a user-space write descriptor can alias
another user-space one; give the input arm a matching `ExtraFdOp` for input
aliases so `resolve_dup_in` can validate and record `N<&M` for `N >= 1`; and
skip validation in both when `n == fd`. Then extend the corpus case with the
rows its header currently excludes.

**Fixed** (2026-08-01), along those lines but with one simplification the plan
did not anticipate: no new `ExtraFdOp` variant was needed on either side.

* `ExtraFdOp::AliasStd(i32)` became `ExtraFdOp::AliasFd(i32)` — the variant only
  ever named a *source* descriptor, and nothing about it was specific to 0/1/2.
  `Shell::alias_write_fd` (was `alias_std_write_fd`) grew one arm that answers a
  source of 3 or more from `open_write_fds`, so `7>&5` shares fd 5's handle the
  way `dup2` shares it.
* The input side reuses `ExtraFdOp::Input`, handing over the very same `InputFd`
  the source holds. That is exactly the cursor-sharing a dup gives, which is why
  no aliasing variant was called for.
* `resolve_dup_out`'s two numeric arms collapsed into one that dispatches on the
  *redirector*: 2 and 1 keep `stderr_to_fd`/`stdout_to_fd`, 3-and-up push an
  `AliasFd`, and 0 does nothing (`0>&N` dups onto stdin, which osh has no write
  model for — the source is still validated). Both sides gained an `n != fd`
  guard, which is bash's skipped `dup2` rather than an optimisation.
* `resolve_dup_in` now validates for every redirector instead of only fd 0.

Covered by a new corpus case,
`tests/corpus/dup-onto-a-non-standard-descriptor.sh`: a missing source under
four redirectors, both self-dups, a scoped alias proving stdout is not diverted
and that the binding dies with the scope, the same alias made to persist by
`exec`, and two names for one input sharing a cursor. The naming case gained the
`7<&"9"` / `7<&9` rows its header used to exclude.

**Follow-up.** The `n != fd` guard skipped the source *check* as well as the
dup's effect, unconditionally — and `resolve_special_redirect` funnels
`/dev/fd/N` through the same resolvers, so `8>/dev/fd/8` with fd 8 closed
started succeeding silently. Fixed under `TD-OILS-EXEC-DUP-WORD-MISCLASSIFIES`
by separating the two: the effect is always skipped (a descriptor duplicated
onto itself is unchanged), while the check is skipped only for a *word*
(`Shell::dup_needs_no_source` / `DupOrigin`), since a special filename is an
`open` that can fail where a dup bash never makes cannot.
