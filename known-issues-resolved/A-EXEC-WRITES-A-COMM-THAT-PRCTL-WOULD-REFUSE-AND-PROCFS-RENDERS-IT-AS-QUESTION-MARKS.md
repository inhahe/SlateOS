### A-EXEC-WRITES-A-COMM-THAT-PRCTL-WOULD-REFUSE-AND-PROCFS-RENDERS-IT-AS-QUESTION-MARKS (lane A, 2026-09-12) — FIXED

**FIXED 2026-09-12, the ABI surfaces.** `comm_truncate` takes and returns `&[u8]`; its
UTF-8 char-boundary walk was deleted rather than ported, because Linux cuts `comm` at 16
bytes flat so byte truncation is the more faithful behaviour and the function collapsed to
one line. `gen_pid_cmdline` drops its decode entirely, `gen_pid_comm` carries bytes end to
end, `build_pid_status` emits `Name:` straight into the byte buffer with its other 22
writes untouched, and `build_pid_stat` splits around field 2 — which is parenthesised in
the format precisely because it may contain anything.

`PR_SET_NAME` no longer validates UTF-8. Its rejection was sound when written, and the
reason it stopped being sound is worth keeping: procfs decoded to `str`, so refusing beat
storing something that would not read back. procfs no longer decodes. Removing it also
closed the asymmetry that made the check **partly decorative** — `execve` set the same comm
through `set_task_name`, which never validated, so bytes this arm refused could already
arrive by the more common route, and the gate only ever caught the caller who asked
politely.

**AMENDMENT: "all five surfaces" was wrong when written. There are nine, and four remain.**

The fix covers the four procfs surfaces and `PR_GET_NAME`, which are the ABI — what
userspace tools read. It does **not** cover four in-kernel shell task listings:

```
kshell.rs:8948, 9418, 9515, 9612
    for info in &task_list { let name = core::str::from_utf8(...).unwrap_or("?"); ... }
```

Each iterates the scheduler's `task_list` and renders an undecodable comm as the literal
`"?"` — the same collision constant one more time, so two processes with different
unreadable names print identically in the shell's own `ps`-style output.

**Why they were missed, which is the fourth time this specific mistake has appeared in
this entry.** I enumerated the ABI: I grepped procfs, fixed what was there, and wrote
"all five surfaces" from the population I had searched rather than from the population
that has the defect. The kshell sites do not decode via `comm_truncate`, are not in
`procfs.rs`, and use `"?"` rather than `"???"` — so every pattern that found the others
missed these, and the phrase "all five" was an inference from a search, not a count.

**Deliberately not fixed in the same change, and this is a judgement rather than an
oversight.** The ABI surfaces and the shell display are different in kind: `/proc` is read
by programs that match, group and kill by name, which is where a collision does damage;
kshell's listing is read by a developer looking at a screen. Fixing the four shell sites
means converting `shell_println!("{name}")` call sites to byte emission in a
144,000-line file, which is the same restructure the procfs builders needed and deserves
its own change and its own verification rather than riding along on this one.

**What is true after this change**, stated so nobody has to re-derive it:

| surface | status |
|---|---|
| `/proc/<pid>/comm` | bytes |
| `/proc/<pid>/stat` field 2 | bytes |
| `/proc/<pid>/status` `Name:` | bytes |
| `/proc/<pid>/cmdline` | bytes |
| `prctl(PR_GET_NAME)` | bytes (always was) |
| `kshell` task listings (4 sites) | **escaped** (`escape_octal`) — see below |

**The four kshell sites are done too, and the fix is different from the procfs one for a
reason worth recording.** They feed a width-padded column (`{:<12}`), so emitting raw bytes
would break the alignment every other row depends on. They use `fs::escape::escape_octal`
instead: total over any byte sequence, lossless, invertible by `unescape_octal`, and pure
printable ASCII — so distinct names stay distinct *and* the column still lines up.

Deliberately **not** `from_utf8_lossy`: U+FFFD is many-to-one, so it would re-create the
exact collision this entry is about in a different alphabet. That is the same choice lane B
made for the four `/proc/<pid>/comm` consumers, and the same argument as
`A-FAT-8-3-NAMES-DECODE-TO-QUESTION-MARKS`: escaping says which byte it was and is
reversible; replacement is neither.

One visible change for ordinary names: a comm containing a space now prints `\040`,
because `escape_octal` escapes anything non-graphic. For a column-aligned listing that is
the better behaviour — a space in a name would otherwise make the columns unparseable — and
it matches how the tree already renders `/proc/mounts`.

**In short:** every running program has a short name the system shows in process
listings. There is a check that stops a program *asking* for a name the system cannot
display — but the same name is also set automatically when a program starts, and that
path has no check. So a program can end up called `???` in every listing, and **every**
program in that state is called `???`, so they cannot be told apart.

**Trivially reachable, by any process, with no unusual filesystem.** The comm is taken
from `exe_path`, falling back to `argv[0]` — and `argv[0]` is whatever the caller passes
to `execve`. A process that execs itself with `argv[0] = b"\xff"` gets a non-UTF-8 comm.
No non-UTF-8 file need exist. (Since `D-VFS-PATHS-ARE-STR-NOT-BYTES`, a non-UTF-8 *path*
reaches it too, so both routes are open.)

**The two paths, and the asymmetry:**

| path | validates UTF-8? | outcome |
|---|---|---|
| `prctl(PR_SET_NAME)` | **yes** — rejects with `EINVAL` | name unchanged |
| `execve` → `spawn.rs:2386-2391` → `sched::set_task_name` | **no** | bytes stored verbatim |

`sched::set_task_name` (`sched/mod.rs:5509`) copies raw bytes and validates nothing.
`PR_SET_NAME`'s own doc (`syscall/linux.rs:12651`) explains the rejection: *"the procfs
surfaces lossily decode the task name to a `str` (invalid bytes would render as `???`),
so we reject rather than store something that wouldn't read back faithfully."* That
reasoning is right. It is simply not enforced where the name is actually set most often.

**What renders.** Two sites decode identically:

```rust
let full_name =
    core::str::from_utf8(task.name.get(..task.name_len).unwrap_or(&[])).unwrap_or("???");
```

`procfs.rs:2367` (`build_pid_status`, the `Name:` line) and `procfs.rs:2690`
(`build_pid_stat`, field 2). `gen_pid_comm` (`procfs.rs:3613`) is the third surface. So
`/proc/<pid>/comm`, `/proc/<pid>/stat` field 2, `/proc/<pid>/status` `Name:` and
`prctl(PR_GET_NAME)` all report `???`.

**Why the collision is the real defect, not the mangling.** A lossy rendering that
preserved *distinctness* would be a display wart. `"???"` is a constant: two processes
with different unreadable names are reported identically, so anything that groups or
matches by name — `pkill`, `pstree`, `top` — treats them as the same program. That is
lane B's `/proc` finding arriving from the producer end: they fixed consumers that read
`stat` into a `String`; this is the kernel writing a `String`-shaped answer in the first
place, so a byte-correct consumer still gets `???`.

**The docstring that names the principle it is not following.** `set_task_name`'s comment
at `sched/mod.rs:5505-5507` says, of its `task_id == 0` guard: *"Rejecting it here rather
than in the `prctl` handler covers every caller, present and future."* Exactly right —
and the UTF-8 check sits in the `prctl` handler, which is the place that sentence warns
against. One check moved to the shared path, one left at the caller, in the same function,
with the reasoning for moving it written above the one that moved.

**Proper fix — the one already named in `todo.txt:10454-10459`,** whose closing condition
is "procfs to emit comm as raw bytes". Smaller than that entry implies:

1. `comm_truncate(&str) -> &str` becomes `(&[u8]) -> &[u8]`. Its UTF-8 char-boundary walk
   exists **only because the parameter is `&str`** — Linux's `comm` is a 16-byte array cut
   at bytes, so byte truncation is *more* faithful, not less.
2. Three call sites (`procfs.rs:2369`, `2692`, `3632`) drop the `from_utf8(...)` and use
   `task.name.get(..task.name_len)` directly. `TaskInfo.name` is already `[u8; 32]` with
   `name_len`, and `sched::copy_task_name` already returns bytes — the byte path exists at
   both ends and only procfs's middle forces `str`.
3. `PR_SET_NAME` then drops its UTF-8 validation, closing the tracked limitation, and the
   two paths agree.

**SCOPE CORRECTED TWICE MORE, same day, and the second one is a real widening of the
bug rather than bookkeeping.** Three estimates of one task, each wrong in the same way,
recorded in full because the pattern is worth more than the number.

| revision | claim | population measured | verdict |
|---|---|---|---|
| original | "three call sites and one helper" | that the *inputs* are already bytes | right conclusion, reasoning never reached the output |
| correction 1 | "23 `write!` calls — a restructure" | *all* writes in `build_pid_status` | wrong: only one write carries the name, and it is the first |
| correction 2 | this table | the decode itself (`unwrap_or("???")`) | four sites, not three — see below |

**The widening: `/proc/<pid>/cmdline` is a fifth affected surface and this entry missed
it.** `gen_pid_cmdline` decodes the same task name with the same
`from_utf8(..).unwrap_or("???")` and emits `name.as_bytes()` with a NUL. So a process with
a non-UTF-8 `argv[0]` reports `???` from `cmdline` too — and `cmdline` is what `ps` reads
for the command line, which makes it the most-read of the five, not the least.

**Why the enumeration missed it, which is the same defect as the estimates.** I listed the
sites by grepping for `comm_truncate`, which finds three. The decode is a different
population: `unwrap_or("???")` finds four. `gen_pid_cmdline` does not truncate — it has no
reason to call the helper — so a search keyed on the helper could not contain it. I picked
the population that matched my mental model of "the comm path" rather than the one that
matched the defect.

**What each site actually requires**, stated as mechanism so a reader can check the size
rather than trust it:

- `comm_truncate(&str) -> &str` becomes `(&[u8]) -> &[u8]`. Its char-boundary walk is
  deleted, not ported: Linux cuts `comm` at 16 bytes flat, so byte truncation is *more*
  faithful.
- `gen_pid_cmdline` drops the decode entirely and copies `task.name.get(..task.name_len)`
  before the NUL. It already returns `Vec<u8>`; this is two lines and needs no helper.
- `gen_pid_comm` already builds a `Vec<u8>`; it drops the decode and pushes bytes.
- `build_pid_status` carries the name in `writeln!(s, "Name:\t{name}")`, the **first** of 23
  writes into a `String`, and returns `s.into_bytes()`. The name bytes are emitted first and
  the untouched `String` appended after. **The other 22 writes do not change.**
- `build_pid_stat` is a **single `format!`** with `name` as one argument inside `({})`,
  returning `text.into_bytes()`. It splits into a prefix ending `" ("`, the name bytes, and
  a suffix beginning `") "`.
- `PR_SET_NAME` then drops its UTF-8 validation, and the exec path and the prctl path stop
  disagreeing about what a name may contain.

**The lesson, which is about corrections rather than about procfs.** A correction inherits
the authority of having been checked, so a wrong one is harder to dislodge than the error
it replaces — and mine turned a right-by-accident estimate into a wrong-by-measurement one
that *read* as more rigorous because it carried a number. Every one of the three revisions
measured a population adjacent to the question: inputs instead of the path, all writes
instead of the writes carrying the name, the helper instead of the decode. None of the
numbers was false.

**Describing the mechanism instead of sizing it is the form that cannot fail this way.** A
reader who doubts "split one `format!` into two" can check it in one command; a reader who
doubts "23 writes" has to reconstruct which question it answered.

**Interim behaviour is safe**, which is why this is debt and not an emergency: nothing is
corrupted on disk, no privilege is involved, and the name is cosmetic to the kernel. What
it costs is that monitoring tools cannot distinguish such processes, and that a process
can *choose* to be indistinguishable by passing a non-UTF-8 `argv[0]`.
