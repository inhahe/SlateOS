### TD-OILS-EXTERNAL-CHILD-HAS-NO-FD-3. A spawned external command never inherits any descriptor above fd 2 — 2026-08-01 — OPEN (target-blocked; unverifiable on the Windows dev host)

**Where:** `userspace/oils/src/interp.rs` — `Shell::run_external` and the
spawn path under it, which map only fd 0, fd 1 and fd 2 onto the child.
`Shell::install_extra_fds` puts the scratch descriptor in the *shell's*
`open_fds` / `open_write_fds`, which is a table the child cannot see.

**Reproduce** (`target/dvscratch/t3/pk.sh`) — all three write `W` into the
file under bash and all three fail under osh with
`sh: line 1: 3: Bad file descriptor`:

```sh
( exec 3>o1; sh -c 'echo W >&3' )      # persistent
( sh -c 'echo W >&3' 3>o2 )            # transient
( sh -c 'echo W >&3' >o3 3>&1 )        # a dup of the list's own fd 1
```

Every fd ≥ 3 osh models is an in-process handle, so `>&3` inside a *builtin*
or a compound body resolves and a `>&3` inside a spawned process does not.
Shell idioms that hand a scratch descriptor to a real program —
`prog 3>log`, `prog >out 3>&1`, and the `exec 3>` + long-running-child
pattern — all silently lose the writes and the child reports `EBADF`.

**Proper fix.** Give the child the descriptor: on Windows, mark the handle
inheritable and pass it in the `STARTUPINFOEX` attribute list (or, more
simply, materialise fd ≥ 3 as an inherited handle at the right numeric slot);
on the SlateOS target, hand the capability across in the spawn message. Either
way `run_external` needs the extra-fd table, which it does not currently take.

**Why not now — and why the host cannot settle it.** fd inheritance above
fd 2 does not survive the Windows process boundary in either direction that
matters here, which was measured rather than assumed
(`target/dvscratch/t3/pn/`):

```sh
# MSYS bash -> native Windows child, on this machine:
bash -c 'python -c "import os; os.write(3, b\"W\")" 3>out'
# OSError: [Errno 9] Bad file descriptor
```

A native child gets fd 3 only through the MSVCRT's `lpReserved2`
`STARTUPINFO` block, which MSYS/Cygwin does not write; a Cygwin child gets it
only from a Cygwin parent, through a shared section osh (a native binary)
cannot produce. So the reference shell itself cannot demonstrate the behaviour
against the childern osh would spawn here, and no corpus case could pin it.

That makes it a **SlateOS-target** feature: there, `run_external`'s spawn
message can carry the capability for each extra fd, and the child's fd table
is ours to populate. Implementing it now would mean writing untestable
platform code on the host and shipping the real half blind. It is logged so
that the target's process-spawn work picks it up, and named as deliberately
absent in the header of
`tests/corpus/dup-of-a-std-fd-copies-the-sink-the-list-installed.sh`.
