### TD-OILS-TRANSIENT-DUP-SOURCE-IS-NOT-THE-LIST-SO-FAR. A transient `N<&M` cannot copy a descriptor that is open but has no read half — 2026-08-01 — ✅ **RESOLVED 2026-08-01**

**Where:** `userspace/oils/src/interp.rs` — `Shell::resolve_dup_in`, the
`fd >= 3` arms, and `Shell::plan_input_fd`.

**Reproduce** (`target/dvscratch/t3/py.sh`, `pv.sh`):

```sh
{ read -r l <&3; } 3<&1;        echo "rc=$? l=[$l]"   # bash: rc=1 l=[]
{ read -r l <&4; } 3>out 4<&3;  echo "rc=$? l=[$l]"   # bash: rc=1 l=[]
```

bash *makes* both dups — fd 1 and a `3> out` are open descriptors, so
`dup2` succeeds — and lets the failure surface at the read through them:
`read: read error: 0: Bad file descriptor`, naming fd 0 because that is what
`read` was pointed at. osh refused the redirect instead, so the body never
ran and the message named the source.

**What it really was.** Measuring the shape turned up a bigger truth than the
entry was written for: `N<&M` and `N>&M` are *one* operation, `dup2(M, N)`.
The arrow the dup is written with picks nothing — the copy carries whatever
access mode fd M had. So `3<&1` gives fd 3 stdout's *write* end (and
`echo W >&3` through it works), and `4>&3` after a `3< file` gives fd 4 the
file's *read* end, cursor shared. osh's fd table was input-xor-output, so it
was wrong in both directions, not just the one the entry described.

That splits into three separate questions, and the fix keeps them apart:

* **open** decides whether the redirect is made at all — `4<&3` after `3<&-`
  is refused, `4<&3` after `3> out` is not;
* a **read half** decides whether `read <&N` finds bytes or `EBADF`;
* a **write half** decides whether `echo >&N` writes or reports
  `echo: write error: Bad file descriptor`.

**Fixed** by giving every fd ≥ 3 binding *both* halves. A new
`InputSrc::WriteOnly` is the mirror of the existing `WriteFd::ReadOnly`: a
descriptor that is open but has nothing to read. `ExtraFdOp::AliasFd(i32)`
became `AliasFd(i32, InputFd)` so one plan entry can carry both halves
(`install_extra_fds` keys on the fd, so two entries would have dropped one);
`ExtraFdOp::Input` now also records `WriteFd::ReadOnly` and
`ExtraFdOp::OutputFile` a `write_only_input()`. `plan_input_fd` answers
`WriteOnly` where it used to answer `None`, and a new `dup_read_half` gives
both `resolve_dup_in` and `resolve_dup_out` the same answer — because the
arrow does not enter into it.

The `exec` path is a separate walker over raw redirects
(`apply_persistent_dup_in` / `apply_persistent_dup_out`, not the `RedirPlan`
resolvers) and got the same treatment, plus a new `exec_dup_read_half`;
`apply_persistent_dup_in` had to take `out: &Out` to reach
`exec_dup_source`. `exec 3< file` and here-docs now record
`WriteFd::ReadOnly` rather than removing the write entry, and `exec 3> file`
records a `write_only_input()`.

Covered by `tests/corpus/dup-copies-the-descriptor-not-the-arrow.sh` and the
unit tests `a_dup_copies_the_descriptor_and_not_the_arrow` and
`reading_a_standard_write_descriptor_is_an_error_not_end_of_input`. Two
shapes were deliberately left out and tracked separately:
TD-OILS-DUP-ONTO-A-STD-FD-IGNORES-THE-SOURCE-MODE (since resolved) and
TD-OILS-DUP-OF-STDOUT-IS-NOT-THE-LIST-SO-FAR.
