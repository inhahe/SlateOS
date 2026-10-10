## D-Q8 — [D] Until the kernel can lock memory, should `mlock` keep saying it did, or say it cannot? — Status: OPEN (raised 2026-10-06)

**In short:** a program can ask for some of its memory to be "locked":
kept in RAM, never moved out to swap. Password managers, GnuPG and
ssh-agent do this for keys, so a key never lands on a disk. Tor does it
with `DisableAllSwap 1`. Audio programs do it so a sound buffer is never
slow to reach. SlateOS's kernel cannot lock memory yet. The C library
answers "done" all the same, and has always done so. Meanwhile the kernel
compresses rarely-used memory in RAM, and writes it to a disk when a spare
raw disk is attached. So a program told its key is locked may have it
compressed, or written to that disk; and an audio program told its buffer
is locked may wait for it to be decompressed. The choice: keep answering
"done" until the kernel can lock memory (lane A is asked), or answer "could
not lock" now.

**Terms.** *Swap*: memory the kernel moves out of RAM when RAM runs short;
here, first compressed into RAM, and onto a disk only when a spare raw one
is attached (`kernel/src/main.rs`, step 20e). *`EAGAIN`*: `mlock`'s
documented "some or all of it could not be locked".

| Option | *What changes:* |
|---|---|
| **A. Keep saying "done"** (today) | Nothing changes. Programs go on believing memory is locked. Keys could reach swap -- compressed RAM always, a disk only with a spare raw one attached. |
| **B. Say "could not lock" (`EAGAIN`) until the kernel can** | GnuPG prints "WARNING: using insecure memory!", and carries on. Tor with `DisableAllSwap 1` refuses to start, as it would on a system that cannot lock. Audio servers say real-time memory is unavailable, and carry on. Nothing is told something false. |
| C. Say "could not lock" only when a disk swap is attached | Keeps keys off disks without warnings where swap stays in RAM. But the library would have to ask the kernel what swap is attached at every call, and audio's latency problem would stay hidden either way. |

- **A** costs nothing today, and the risk is narrow: on an ordinary install
  swap is compressed RAM, which is lost at power-off. But it is a false
  "done". This library has refused other false successes in favour of honest
  errors -- `madvise`, real-time scheduling, `mmap` of files, all on
  2026-10-06 -- and a key reaching a disk is the case `mlock` exists to
  prevent.
- **B** is honest and consistent with those, but visible: some programs warn,
  and one hardening option (Tor's) stops working until the kernel can lock.
- My recommendation: **B**, because the promise is about security. It
  could be done in minutes.

**Where it bites:** `posix/src/mman.rs` -- `mlock`, `mlock2`, `mlockall`
(validation, then success). The Linux ABI's `mlock` (`kernel/src/syscall/linux.rs`)
does the same, which is lane A's, in the same request.

**If never answered:** nothing breaks and nothing is blocked; the gap stays
as described, and closes by itself when the kernel can lock memory
(`requests/d-a-mlock-locks-nothing.md`).
