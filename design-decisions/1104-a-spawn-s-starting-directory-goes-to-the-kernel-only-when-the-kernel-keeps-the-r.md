## 1104. A spawn's starting directory goes to the kernel only when the kernel keeps the record; an older kernel gets the old behaviour, not a failure

**Date:** 2026-09-25
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** a program can ask for the program it starts to begin in a
particular folder. Rust does this for every `Command` with a `current_dir`, and
the Oils shell does it for every command it runs. Since §960 the kernel can
start a child in a folder, and the C library now tells it which. A kernel from
before that change does not know the new field, and it refuses the whole
request rather than ignore it. So the library asks the kernel once, cheaply,
whether it keeps a folder record. If it does not, the library leaves the field
out and the child starts in its parent's folder, as every child did before.

**The alternatives.**

| | on a kernel with the record | on a kernel without it |
|---|---|---|
| **Probe, then send or not (chosen)** | the child starts where it was asked to | the child starts in its parent's folder: the old, wrong behaviour, and silent |
| Always send the directory | same | every spawn with a `chdir` action fails with `EINVAL`, which stops every external command Oils runs |
| Refuse `addchdir_np` on an old kernel (`ENOSYS`) | same | as above, one step earlier and with a clearer errno |

**Why the silent option, when this project usually prefers loud failure.** The
choice only matters in the window before §960's kernel half reaches every tree,
which is days. In that window the wrong-folder behaviour is exactly what every
tree already has, so choosing it adds no new failure. The loud option would
turn a known, tracked defect (`TD-D-CWD-AND-UMASK-DO-NOT-SURVIVE-EXEC-OR-SPAWN`)
into "the shell cannot run anything", on every boot of every lane that merges
this before the kernel half. The same reasoning is why that entry originally
declined to make `addchdir_np` fail.

**Cost.** The probe is one `SYS_PROCESS_GET_CWD` with a one-byte buffer, made
once per process and cached (`posix/src/unistd.rs` `kernel_keeps_cwd`). The
`chdir` action itself is still carried out in full on either kernel: resolved
against where the earlier actions left the child, and checked to be a
directory. A spawn naming a folder that does not exist fails on both.

**Revisit when** no supported kernel lacks syscalls 1077-1079. The probe and
`child_start_dir`'s condition can then go, and the directory is always sent.
