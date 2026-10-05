### [A] BLKDISCARD: the fd-to-device step is solved, and partitions fail closed -- 2026-09-21
**Status:** DESIGN COMPLETE, implementation deliberately not started. Supersedes the open step in the BLKDISCARD scoping entry above.

**The step I filed as unsolved is solved, and needed no new code.** `sys_ioctl`
receives an fd; `blkdev::with_device` looks up by name. The chain between them
already exists, spread across three modules:

```
  fd  -> pcb::linux_fd_lookup(pid, fd)   -> FdEntry
      -> FdEntry.raw_handle
      -> fs::handle::handle_path(handle) -> PathBuf
      -> strip the `/dev/` prefix        -> short name
      -> blkdev::with_device(name, ...)
```

**The safety property holds by construction, which is the part worth
recording.** I flagged that `/dev/sda1` must not silently discard `/dev/sda`,
because a partition-vs-whole-disk confusion in a discard arm destroys the
wrong extent. `blkdev::register` takes short whole-device names (`"sda"`,
`"vda"`), so a lookup of `"sda1"` simply **fails** and the ioctl returns an
error. Failing closed is the correct default for a destructive operation, and
it needs no special handling -- only a comment saying why the obvious future
improvement, stripping a trailing partition digit to be helpful, would be
catastrophic.

**One byte-handling note for whoever writes it.** Paths are bytes, not UTF-8
(CLAUDE.md: never force UTF-8 on filesystem paths). Strip `/dev/` from the
path's bytes, then convert only the remainder, and treat a non-UTF-8
remainder as no-match rather than an error worth reporting -- it cannot name a
registered device either way.

**Why it is not implemented here.** This is a data-destroying syscall, and
today's session has a measured error rate on mechanical edits: thirteen escape
collapses, five assertions written over text the replacement itself supplied,
two entry points missed until the compiler objected, and a grep that
under-counted call sites by two. Every one was caught -- by a compiler or an
assertion, on code whose worst case is a red boot. The worst case for a
wrongly-keyed discard arm is a destroyed extent, and no gate in this tree
would catch it.

The design above is complete enough that writing it is mechanical: three
constants, one fd-to-name helper, three match arms, one of which
(`BLKSECDISCARD`) must refuse. That is a better first task for a fresh session
than a last one for a long session.
