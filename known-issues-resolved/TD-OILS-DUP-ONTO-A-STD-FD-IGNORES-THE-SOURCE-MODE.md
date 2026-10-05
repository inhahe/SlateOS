### TD-OILS-DUP-ONTO-A-STD-FD-IGNORES-THE-SOURCE-MODE. `1<&0` / `2<&0` keeps the standard descriptor's write half instead of the read-only source's — 2026-08-01 — ✅ **RESOLVED 2026-08-01**

**Where:** `userspace/oils/src/interp.rs` — `Shell::resolve_dup_in`, the
comment at the end that read "`1<&N` / `2<&N` …", and the `fd <= 2` arm of
`Shell::apply_persistent_dup_in`.

**Reproduce** (`target/dvscratch/t3/pc.sh`, `pn.sh`):

```sh
printf 'one\n' > in
( exec 0<in; { echo W >&2; } 2<&0 )   # bash: echo: write error: Bad file descriptor
                                      # osh was:  W, on the real stderr
( exec 0<in; { read -r l <&2; } 2<&0; echo "l=[$l]" )   # bash: l=[one]  osh was: EBADF
( { echo W; } 1<in )                  # bash: echo: write error — osh was: W
( nosuchcmd 2<in )                    # bash: silent, 127 — osh was: the diagnostic
```

A dup *onto* fd 1 or fd 2 from a read-only source has to leave that fd with
the source's access mode — readable, and not writable — and the whole family
of shapes that bind a std fd to a source (`1< file`, `2<&3`, `1>&0`,
`1<> file`, a here-doc on fd 1) has to answer alike. osh modelled fds 0, 1
and 2 as the shell's own streams rather than as entries in `open_fds` /
`open_write_fds`, so a dup onto one of them had nowhere to put the source's
mode: the write half stayed whatever the standard stream was, and the read
half was not installed at all.

**Fixed** by giving `RedirPlan` the missing slot. New fields `stdout_read` /
`stderr_read` hold the read half a redirect gave fd 1 / fd 2, and the derived
predicates `stdout_is_read_only()` / `stderr_is_read_only()` say when that
binding has displaced the write half. The read half is installed for a body's
duration by the new `Shell::install_std_reads` (into `open_fds` keyed 1 / 2,
beside every other descriptor's, which is where `clone_input_fd` already
looked) and permanently by the new `set_std_read_half` / `set_std_write_half`
pair; the write half is expressed with the existing `WriteFd::ReadOnly`, which
every consumer already handled — `as_stderr_target()` gives `Discard`,
`write_to_write_fd` gives `EBADF`, and a `1>&N` naming it is refused.

`ReadOnly` is deliberately *not* folded into `Closed`: a write to either fails
identically, but `1<in 3>&1` must **make** the dup (and fail at the write)
where `1>&- 3>&1` must refuse it.

Every shape that binds a std fd to a source now goes through that slot, not
just the dups: `1< file` / `2< file` (the `RedirectOp::Read` arm's `fd == 0 ||
fd >= 3` guard is gone), `1<> file`, and a here-document or here-string on
fd 1 / fd 2 — the last via the new `Shell::plan_here_bytes`, which replaces
three copies of "fd 0 or fd ≥ 3, otherwise silently drop it". `2>&1` after a
`1< file` follows fd 1 into having no write half, sharing its read half, which
is the `plan.stdout.is_some()` test in `resolve_dup_out`'s `2>&1` branch
gaining a read-only sibling.

Four call sites had to learn the state beyond the plan itself:

* `RedirPlan::needs_scope` — a function invocation (`myfunc 1< file`) binds
  fd 1 body-wide, exactly as `>&-` does, so it must run inside a redirect
  scope. Without this the body wrote to the ambient stdout.
* `Shell::exec_with_redirects` — a `1>&N` whose source turns out to have no
  write half is now *made* (local `stdout_read_only`) rather than refused;
  only a `Closed` source is still refused, which is what bash does. This
  resolves TD-OILS-FD0-WRITE's last residue, where `{ echo W; } 1>&0 < f`
  reported `0: Bad file descriptor` instead of running the body and failing
  the write.
* `Shell::bemit_cmd_stderr` — the shell's own command-level diagnostics
  (`command not found`, spawn failures) are dropped when the command's fd 2 is
  closed or read-only, rather than falling back to the *shell's* fd 2. This
  was already wrong for the plain `nosuchcmd 2>&-`.

Covered by `tests/corpus/a-std-fd-bound-to-a-read-only-source.sh` and the unit
tests `a_std_fd_bound_to_a_read_only_source_keeps_only_its_read_half`,
`a_read_only_fd2_drops_the_diagnostics_it_cannot_be_told` and
`a_dup_onto_a_std_fd_carries_the_sources_access_mode`.

**What is left.** Two residues, both already tracked elsewhere:

* an *external* child never sees the read-only std fd — `sh -c 'echo W' 1<in`
  writes to the ambient stdout under osh and is `EBADF` under bash. Same cause
  as TD-OILS-EXTERNAL-CHILD-HAS-NO-FD-3: `run_external` maps only the shell's
  own three streams onto the child, and does so from the ambient handles.
* `1>&0 <>rw` and `1>&0 0<&-`, where the entry *after* the dup changes what
  fd 0 is — the ordering residue of
  TD-OILS-DUP-OF-STDOUT-IS-NOT-THE-LIST-SO-FAR, recorded there.

A third, found while closing this one, was
TD-OILS-RW-ON-A-STD-WRITE-FD-APPENDS below (since resolved): `1<> file` gained
a read half, but the *write* half was still a reopened path, so the two did not
share an offset.
