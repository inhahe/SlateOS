### TD-OILS-A-REDIRECT-LISTS-EARLIER-MEMBERS-ARE-IN-EFFECT-FOR-EVERY-FD-NOT-JUST-FD-2. A later word's expansion cannot see an earlier `3>` or `<` in the same list — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::resolve_redirect_list`, which
installed the partial plan's *stderr* around each step but nothing else.

**What.** bash performs the whole list against the real fd table, so *every*
descriptor an earlier member bound is in force while a later member's word is
expanded — not just fd 2:

```text
$ bash -c 'true 3>f3 > $(echo x >&3; echo /dev/null)'; cat f3
x                                     # bash: fd 3 is already open on f3
$ osh  -c 'true 3>f3 > $(echo x >&3; echo /dev/null)'
osh: line 1: 3: Bad file descriptor    # osh: the plan is not installed yet

$ bash -c 'true <in > $(cat; echo /dev/null)'
bash: line 1: $(cat; echo /dev/null): ambiguous redirect
                                      # bash: `cat` read `in`, so two fields
$ osh  -c 'true <in > $(cat; echo /dev/null)'
                                      # osh: `cat` reads the terminal, and hangs
```

Found while fixing TD-OILS-A-REDIRECT-LIST-IS-NOT-APPLIED-AS-IT-IS-BUILT above,
whose measurement covered fd 2 only.

**Impact.** Confined to a redirect word that *reads* the descriptor table while
being expanded — a command substitution using `>&N`/`<&N`, or one that reads fd
0. Rare in practice, but the fd-0 case could *hang* osh where bash returned
immediately, which is worse than a wrong answer.

**Fixed.** The partial installation grew from "the stderr sink" to "the plan so
far", in three parts, one per descriptor class:

- **fd ≥ 3.** `ExtraFdOp::OutputFile` carried a *path*, so re-installing one
  reopened and re-truncated the file. It now carries the `Arc<File>` the
  resolve-time open produced — the same "open once, keep the handle" treatment
  fd 1 and fd 2 got in the entry above — which makes `install_extra_fds`
  side-effect-free and so safe to call mid-list. `resolve_redirect_list` installs
  `plan.extra_fds` *incrementally* (only the entries the last step appended,
  tracked by a `staged_upto` index) and unwinds once at the end via
  `restore_extra_fds`, because re-installing an earlier entry would rebind a
  descriptor the expansion may still be holding. A repeated fd is saved twice —
  once by the step that first bound it, once by the step that rebound it — and
  the unwind runs in reverse, so the binding the list started with is what comes
  back. Unwinding at the end rather than per step is why the two failure exits
  now record a `RedirFail` and break instead of returning where they are found.
- **fd 0.** New `push_partial_stdin` / `pop_partial_stdin`, the companions of
  `push_partial_stderr`, wrapped around each step. They install the plan's fd 0
  as `exec_stdin`/`exec_stdin_write` — the *ambient* binding — because that is
  how fd 0 reaches an expansion at all: a command substitution runs with
  `StdinSrc::Inherit`, which resolves through it. The source is resolved by the
  existing `plan_stdin_fd`, already designed for mid-list use by the `<&0` dup,
  so the word and the command *share* one cursor exactly as they share one
  descriptor in bash; `<&-` resolves to an explicitly closed input rather than to
  `None`, which as an ambient binding would have meant the real terminal.
- **fd 1** needs no staging: a command substitution's fd 1 is always its capture
  pipe, so no word can observe the list's stdout by writing to it, and a *dup* of
  it (`>f 3>&1`) is answered from the plan by `AliasSink`, not from the
  descriptor table. Verified by measurement, not assumed.

Now matching bash, all previously divergent:

```text
true 3>f3 > $(echo x >&3; echo /dev/null)                  f3 holds `x`
true 3>f3 4>&3 > $(echo y >&4; echo /dev/null)             f3 holds `y`
true <in 3> $(cat >&2; echo /dev/null)                     `in`'s contents
true <in 3> $(read -r a; …) 4> $(read -r b; …)             a=L1, b=L2
{ read -r c; …; } <in 3> $(read -r a; …)                   a=L1, c=L2
true <<< $'H1\nH2' 3> $(read -r a; …)                      a=H1
true <&- 3> $(read -r a; …)                                read error: 0: EBADF
```

and, unchanged, the reversed orders that must *not* see the binding
(`3> $(…) <in`, `> $(echo x >&3; …) 3>f3`), the no-leak-past-the-list cases, and
an outer `exec 3>outer` restored afterwards with its offset shared.

Covered by the same corpus case as the entry above.
