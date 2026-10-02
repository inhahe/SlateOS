### TD-OILS-EXTERNAL-ARGV0-IS-THE-RESOLVED-PATH. A child is told it is `/usr/bin/sh`, not `sh` — 2026-08-06 — OPEN (host-blocked)

**Where:** `userspace/oils/src/interp.rs` — the spawn path, which builds
`PCommand::new(resolved_path)`.

**What.** bash `execve`s the resolved path but passes the *word* as `argv[0]`,
so a child names itself the way the script wrote it. osh passes the resolved
path, and every diagnostic a child emits about itself differs:

```text
$ sh -c 'echo argv0=$0'
bash: argv0=sh
osh : argv0=/usr/bin/sh

$ sh -c 'echo W' 1<in        # the child's own write error
bash: sh: line 1: echo: write error: Bad file descriptor
osh : /usr/bin/sh: line 1: echo: write error: Bad file descriptor
```

**Why not now.** Windows has no `argv[0]` separate from the program: a process's
command line *is* its argv, and `std::process::Command` writes the program path
into it. Unix's `CommandExt::arg0` has no Windows counterpart, and
`raw_arg` appends arguments rather than replacing the zeroth. Setting it would
mean building the command line by hand and spawning through `CreateProcessW`
with a separate `lpApplicationName` — real platform code for a host that is not
the target.

**Proper fix.** On the SlateOS target, the spawn message carries argv itself and
`argv[0]` is simply the word. If the host spelling starts blocking corpus cases
before then, the Windows path is `lpApplicationName` = resolved path,
`lpCommandLine` = the word plus the arguments, quoted per `CommandLineToArgvW`.

**Found by** extending `a-std-fd-bound-to-a-read-only-source.sh` to external
commands: the statuses matched but the children's diagnostics did not, so every
external probe there discards fd 2 and compares the status.
