## D-POSIX-THE-CONSTANTS-OF-78-HEADERS-WERE-NEVER-COMPARED — `check-libc-abi.py` compared the library's constants with 105 of musl's 183 headers; in the other 78, sixteen of its numbers were not the header's (lane D, 2026-09-30) — **Status: FIXED 2026-09-30**

**In short:** a C program passes the library the numbers its header defines
-- flags, `ioctl` requests, option bits -- so the library's own copies of
those numbers must be the header's. A gate compares them on every push, but
it read a fixed list of headers written on 2026-09-27, and 78 of musl's
headers were not on it. Reading all of them, as it does now, found sixteen
numbers that were wrong, and reading around those found one more that no
musl header has.

| Where | Was | Is | What it meant |
|---|---|---|---|
| `<stropts.h>`: `I_NREAD`, `I_SRDOPT`, `I_GRDOPT`, `I_SETSIG`, `I_GETSIG`, `I_RECVFD`, `I_CANPUT` | seven other numbers, `I_NREAD` being musl's `I_SRDOPT` | musl's, `('S' << 8) \| n`, and the other ten requests and the event, mode and band constants musl has | no code read them (the STREAMS calls are glibc's stubs); wrong for any that would |
| `<stropts.h>`: `RMSGD`, `RMSGN` | swapped | 1, 2 | the same |
| `<fmtmsg.h>`: `MM_RECOVER`, `MM_NRECOV` | 0x10000, 0x20000 | 0x40, 0x80; `MM_APPL`, `MM_UTIL`, `MM_OPSYS`, `MM_NULLMC`, `MM_NULLSEV` added | `fmtmsg` reads only `MM_PRINT` and `MM_CONSOLE` today |
| `<sys/param.h>`: `MAXHOSTNAMELEN` | 256 | 64, `HOST_NAME_MAX` | glibc and musl both say 64 |
| `<sys/user.h>`: `PAGE_MASK` | `PAGE_SIZE - 1`, the offset bits | `~(PAGE_SIZE - 1)`, the page-number bits, as the header defines it | the complement of the header's mask |
| `<resolv.h>`: `RES_DEFAULT` | glibc's, without `RES_NOIP6DOTINT` | musl's, with it (`RES_NOIP6DOTINT` added; accepted, never acted on) | `_res.options` after `res_init` now equals the header's `RES_DEFAULT` |
| `<resolv.h>`: `RES_NOTLDQUERY` (no musl header has it) | 0x0010_0000, which is `RES_USE_EDNS0` in both headers | 0x0100_0000, glibc's | a program asking for EDNS0 got no-TLD-query instead |
| `legacy::class` | `UPPER`, `SPACE` ... | `_ISupper`, `_ISspace` ..., the names `posix/include/ctype.h` declares | `SPACE` met `<scsi/scsi.h>`'s SCSI opcode of that name; the gate now checks the twelve against the overlay |

Three differences are deliberate and join design-decisions §1119's table and
the gate's `KNOWN_DIFFERENT`: `NGROUPS` (65536, as `NGROUPS_MAX`), `NBPG` and
`PAGE_MASK` (16 KiB pages, as `PAGE_SIZE`).

**Fix:** `scripts/check-libc-abi.py` reads its oracle headers out of musl's
include directory and the overlay's at run time -- every header, not a list
-- which all compile together with `_GNU_SOURCE`: 1,816 constants compared
where it was 1,646. **Compared since:** a constant no musl header and no overlay
header defines, like `RES_NOTLDQUERY` above -- against glibc's and the
kernel's headers, through a table (the next entry,
`D-POSIX-CONSTANTS-NO-MUSL-HEADER-NAMES-HAD-NO-ORACLE`).

**Where:** `posix/src/stropts.rs`, `fmtmsg.rs`, `sys_param.rs`, `resolv.rs`,
`legacy.rs`; `scripts/check-libc-abi.py`.
