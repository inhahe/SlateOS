## D-POSIX-MLOCK-LOCKS-NOTHING — `mlock` and `mlockall` answer success and lock nothing, while the kernel swaps user memory (lane D, 2026-10-06)

**Status:** OPEN -- the fix is lane A's (`requests/d-a-mlock-locks-nothing.md`); what the library answers meanwhile is the operator's (`open-questions/D-Q8.md`).

**In short:** a program "locks" memory to keep it out of swap -- a key that
must never reach a disk, an audio buffer that must never be slow. Neither
of SlateOS's kernel interfaces can lock memory, and the C library answers
success anyway. The kernel compresses rarely-used memory in RAM, and writes
it to a disk when a spare raw one is attached. So a locked key can be
swapped (compressed in RAM on an ordinary install, written to disk with a
raw spare disk), and a locked buffer can be slow to reach.

**Where:** `posix/src/mman.rs` -- `mlock`, `mlock2`, `mlockall`: the
argument checks, `CAP_IPC_LOCK` and `RLIMIT_MEMLOCK` (`check_mlock_caps`),
then 0. The Linux ABI's `sys_mlock` and `sys_mlockall`
(`kernel/src/syscall/linux.rs`) check and answer 0 the same way.

**The proper fix:** a `locked` flag on the kernel's mappings, which the
swap reclaimer passes by, set by a native call that the library sends the
calls to (the request has the shape). Until then, D-Q8 asks whether the
library should go on answering success, or answer `EAGAIN` ("could not be
locked"). The latter is honest; it also makes GnuPG warn, and Tor's
`DisableAllSwap 1` refuse to start.
