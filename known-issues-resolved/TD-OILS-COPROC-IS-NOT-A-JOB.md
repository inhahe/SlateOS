### TD-OILS-COPROC-IS-NOT-A-JOB. A coproc cannot be waited for, and its endpoints are not real descriptors — ✅ RESOLVED 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — `exec_coproc`. It spawned the body on
a thread and published `NAME[0]`/`NAME[1]`/`NAME_PID`, but registered no entry in
`self.jobs`, and the two array elements are osh-internal fd numbers rather than
descriptors the rest of the shell can dup.

**Reproduce:**

```sh
coproc C { :; }
wait "$C_PID"; echo "wait=$?"
coproc D { sleep 0; }
exec {w}<&"${D[0]}"
```

bash: `wait=0`, and the `exec` is silent. osh, before the fix:

```
wait: pid 900000 is not a child of this shell
wait=127
12: Bad file descriptor
```

**✅ The job half is fixed.** The body now goes into the job table under the
synthetic pid that `NAME_PID` publishes, carrying the `coproc …` command text
that `jobs` prints, so `wait "$NAME_PID"` reaps it and answers its status, a
bare `wait` waits for it, `jobs` lists it while it runs, and `kill` finds it.
The thread returns `last_status` (and fires the body's EXIT trap first) the way
a `&` job's does, and the write-only `coproc_jobs` handle list it used to be
parked in is gone. Covered by `tests/corpus/coproc-is-a-job.sh`, which prints no
raw fd number — bash allocates the endpoints near the process's fd limit (63,
62, …) and osh from 10 up, so the numbers are a property of the host.

**✅ The endpoint half is fixed too.** `${NAME[0]}` lives in `coproc_read_fds`,
which the `read`/`<&` *input* path consulted but the `exec` builtin's fd-alias
path did not, so `exec {v}<&"${NAME[0]}"` was "Bad file descriptor". The alias
path now looks there first — and a dup names one open file description, so the
two numbers had to share the *reader*, not just the descriptor: a buffered
reader has already pulled bytes off the pipe that neither number has handed
out, and a `try_clone` would have stranded them under the original. Hence
`coproc_read_fds` holds `Arc<Mutex<BufReader<File>>>` (the new `CoprocRead`)
rather than `RefCell<…>`, and the alias is an `Arc::clone`. Every path that
rebinds or closes a number now clears any coproc entry it had, so a descriptor
is one thing at a time. The write end was already an ordinary `open_write_fds`
entry and needed nothing. Covered by
`tests/corpus/coproc-read-end-can-be-duped.sh`.

**Left out deliberately:** `exec 0<&"${NAME[0]}"`. fd 0's binding is an
`InputFd` (`Arc<Mutex<InputSrc>>`), which cannot alias a `CoprocRead`; joining
the two would mean one input representation for pipes and byte snapshots alike.
Nothing in the corpus does it, and `read <&"${NAME[0]}"` — the way a script
actually reads a coproc — has always worked.
