### [D] B-D-USED-STATIC-TAGGED-THE-SERVICES-GNU — 2026-09-25 — FIXED

**Status:** FIXED 2026-09-25, the day it was introduced — by e6d9cee96, the
commit that gave the five bare-metal services their SlateOS ABI note.

**In short:** marking the five small built-in programs as "SlateOS-native" made
them look like Linux programs to any kernel that has not learned to read the
mark. Such a kernel runs them with Linux's system-call numbers, which mean
different things, so they never finish. On lane D's boot of 2026-09-25 the
kernel's container self-test, which runs `hello`, waited forever behind it.

**What happened:** e6d9cee96 carried the note in each service as a `#[used]`
static in section `.note.slateos`. On ELF, Rust's `#[used]` gives the section
`SHF_GNU_RETAIN`, and LLVM tags every object that uses a GNU extension
`ELFOSABI_GNU`, so the linked services read `OS/ABI: UNIX - GNU` where they had
read System V (checked with `readelf -h` against lanes A and C's builds). The
kernel on `main` takes `ELFOSABI_GNU` as its first Linux signal
(`ElfFile::detect_linux_abi`) and ran `hello` on the Linux table, where its
`SYS_EXIT` (1) is Linux's `write`. The liveness monitor reported `SUSPECTED
LIVELOCK` behind `/bin/hello` (task 428, zero context switches) from about 3,000
s into the boot; the run could not progress and was stopped. Saved serial log:
`build/serial-run5-livelock.txt` in lane D's worktree.

Lane A's kernel ranks the note above every Linux signal (`has_slateos_marker`
first in `detect_linux_abi`), so the tag is harmless there. But it only has to be
harmless on the kernel the binary happens to meet, and that depends on merge
order.

**Fix:** each service assembles its note with `global_asm!`
(`.pushsection .note.slateos, "a", @note`), as posix's crt0 does. That sets no
GNU flag, and `KEEP` in each `linker.ld` keeps the section. All five read
System V again, with the note in a PT_NOTE of their own (`readelf -h`, `-l`,
`-n`).

**Lesson:** a `#[used]` static on an ELF target changes the object's OS/ABI
byte, which this kernel reads as an ABI decision. Emit marker notes with
assembler directives, never with `#[used]`.
