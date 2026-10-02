## §272 — `/dev` grows subdirectories and a character-device entry type, because "openable but invisible" is not a state a real client survives

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** the kernel has working keyboard, mouse, graphics and sound device
files at paths like `/dev/input/event0`. Until now they could be opened only by
a program that already knew the exact path — nothing that *looked* in `/dev`
could see them, because our `/dev` had no folders at all, and anything that
asked "what kind of file is this?" was told "an ordinary file" rather than "a
device". Both answers are ones the standard Linux libraries treat as *no
hardware present*: libinput lists the folder and gives up, and it also rejects
any entry that does not report itself as a device. So a machine whose keyboard
demonstrably works would have reported no keyboard. This change gives `/dev`
real subdirectories and adds a "character device" file type, so the libraries
find what is actually there.

### What changed

* `fs::vfs::EntryType` gains a `CharDevice` variant. It is produced by devfs
  alone; `stat` maps it to `S_IFCHR`, `getdents64` to `DT_CHR`.
* devfs stops being a flat list of twelve names and becomes a table of 22 nodes
  whose paths may contain a `/`. `readdir` of a directory node lists its
  children; `readdir` of a file is `NotADirectory`; `readdir` of a missing path
  is `NotFound` (it used to be `NotADirectory` for both, which is the wrong
  signal — `NotADirectory` is how a scanning client learns to stop descending).
* `/dev/input`, `/dev/dri` and `/dev/snd` and their seven device nodes are now
  visible to `readdir`, `stat` and `metadata`.

### The nodes are listed by a filesystem that does not serve them

This is the part with a genuine tradeoff. `open("/dev/input/event0")` never
reaches devfs — it is intercepted in `syscall/linux.rs`, which mints a device
handle instead of a VFS file. So devfs now advertises paths whose contents come
from somewhere else, and the table here has to be kept in step with the
interception table there by hand: adding a device in one place and not the other
gives either an invisible device (as before) or a phantom one.

The alternative was to have the syscall layer own the namespace too — devfs
would ask it what exists. That removes the duplication, and it was rejected
because it inverts the dependency: the filesystem would have to call into the
syscall layer, which is the layer above it, to answer `readdir`. A pseudo-fs
whose contents are decided by its caller is a much harder thing to reason about
than two lists that must agree, and the lists are short, fixed and covered by a
self-test that walks every directory and stats every entry it finds.

The duplication is bounded by that self-test, not by discipline: it asserts the
count in each directory and that every entry stats back as a character device,
so a node added to one table and not the other fails the boot test rather than
failing silently in a library six months later.

### Reading a device node through the VFS is `NotSupported`, not `NotFound`

`stat` now says `/dev/input/event0` is there, so a read of it that answers "no
such file" contradicts the `stat` that just succeeded and sends whoever hits it
looking for a missing entry that is sitting right in front of them.
`NotSupported` — "not that way" — is the honest answer, and directories get
`IsADirectory` for the same reason.

### No `BlockDevice` sibling

Deliberate. This kernel has no block device nodes to name: storage is reached
through the VFS, not through `/dev/sdaN`. A variant with no producer is one that
every `match` in the tree must answer for, and its presence would tell the next
reader that block device nodes exist somewhere in this system. They do not.
