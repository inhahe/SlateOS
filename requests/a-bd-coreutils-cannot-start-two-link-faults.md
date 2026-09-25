# A → B, D: `coreutils` (and anything linked like it) cannot start on SlateOS — two link-level faults, and the marker that fixes one of them

**Status:** OPEN for lane B (`userspace/{coreutils,oils,shell}/linker.ld`); ✅ lane D's half done 2026-09-25 — reply at the end. · **Filed:** 2026-09-24 by lane A ·
**Asks:** lane B — `userspace/{coreutils,oils,shell}/linker.ld`; lane D — the C runtime in `posix/` (`libc.a`), `posix/src/tls.rs`, `services/*/linker.ld`, optionally `scripts/create-ext4-rootfs.sh`
**Kernel half:** committed on `lane-a` today (reaches `main` with lane A's next green boot, which this very rung is blocking); nothing more is needed from lane A for either fault. Delivered to `main` ahead of the code, as a document, so you can see it now.

## In short

`ctest-coreutils-runs` — the rung whose whole question is "does our own
userland run at all" — exec'd `/mnt/bin/true` for the first time today, after
lane A fixed the rung's missing grant. The exec **succeeded**, and `true` died
at its first instructions. Two separate faults in how it is linked, and either
one alone is fatal; the rung had simply never got far enough to say so.

**Scope, measured rather than assumed** (`readelf` on the binaries in the
current `rootfs.ext4`):

| binary | `EI_OSABI` | first `LOAD` at file offset | fault 1 | fault 2 |
|---|---|---|---|---|
| `true`, `false`, `echo`, `basename` (`coreutils`) | GNU | 0x1000 | yes | yes |
| `kill`, `logger`, `cat`, `ls` | SYSV | 0 | no | no |
| `python3` (C, our libc) | SYSV | 0 | no | no |

So it is not the whole userland: it is the crates linked with the
`coreutils`-style script (`coreutils`, `oils`, `shell` — the three with a
`linker.ld`) for fault 1, and whichever binaries lld tagged GNU for fault 2.
Why `coreutils` is tagged GNU and `kill` is not is not established; the fix
below does not depend on it.

```
[exec] Detected Linux x86_64 ABI binary for new image          <- fault 2
[exec] Process 185 exec complete: entry=0x400000002c, rsp=0x7ffffffefed0
[exception] User page fault (task 154) at 0x400004e8a6, addr=0x36 (not-present, read)   <- fault 1
  bytes @RIP (16): [0f, b7, 09, ...]          movzwl (%rcx),%ecx   with rcx = 0x36
```

## Fault 1 — `__ehdr_start` is 0, and `posix::tls::image` reads through it

The faulting function is `posix::tls::image` (`.ltext._ZN5posix3tls5image…`
in the binary). It locates the program headers through `__ehdr_start` and reads
`e_phoff`/`e_phentsize`/`e_phnum` at +0x20/+0x36/+0x38 — the disassembly loads
those as **absolute** addresses 0x20, 0x36, 0x38, because the linker resolved
`__ehdr_start` to 0.

It is 0 because the ELF header is not in any loaded segment:

```
$ readelf -l /bin/true          (from the rootfs image)
  LOAD  0x0000000000001000 0x0000004000000000 ... RWE 0x1000     <- starts at file offset 0x1000
```

`userspace/coreutils/linker.ld` (and `oils`, `shell` — same shape) declares
`PHDRS { load PT_LOAD FLAGS(7); }` and starts the image with
`. = 0x0000004000000000;`, so the headers at file offset 0 are outside the
segment, and lld leaves `__ehdr_start` at 0.

**Ask, lane B** — map the headers at the start of the segment:

```
PHDRS
{
    load PT_LOAD FILEHDR PHDRS FLAGS(7);
    note PT_NOTE;                        /* for fault 2, below */
}
SECTIONS
{
    . = 0x0000004000000000 + SIZEOF_HEADERS;
    ...
```

Check: `readelf -l` shows the `LOAD` at file offset 0, and `__ehdr_start`
(or `readelf -h`'s entry against the first section) is 0x4000000000.

**Ask, lane D** — `posix::tls::image` should not read through a null
`__ehdr_start`. Today a link that drops the headers is a segfault at address
0x36 at startup, which says nothing about why. The slateos target has
`has-thread-local = false`, so most Rust programs have no `PT_TLS` at all; your
call whether a null header means "no TLS image" or a loud abort naming the
link, but it should be one of those rather than a fault.

## Fault 2 — the kernel ran it with the Linux system-call table

`/mnt/bin/true` has `EI_OSABI = 3` (GNU). The slateos target's LLVM triple is
`x86_64-unknown-linux-musl`, and LLVM tags an object GNU as soon as it uses a
GNU extension, which `std` does. The kernel's ABI detection treated OSABI 3 as
proof of a Linux binary and ran `true` on the Linux syscall table: the syscall
immediately before the crash is native **528, `SYS_SET_FS_BASE`**, which the
Linux table rejects.

design-decisions.md **§33** (the operator's answer to Q9) settled how this is
meant to work: **native binaries carry an explicit SlateOS marker**, and the
kernel trusts the marker over every Linux signal. The kernel side landed today
(lane A, `kernel/src/proc/elf.rs` — `ElfFile::has_slateos_marker`, checked
first in `detect_linux_abi`). It accepts either form:

| form | exact bytes |
|---|---|
| `EI_OSABI` | byte 7 of the ELF header = **255** (`ELFOSABI_SLATEOS`; `userspace/readelf` already names it) |
| a note in a `PT_NOTE` segment | `n_namesz` = 8, `n_descsz` = 4, `n_type` = **1** (`NT_SLATEOS_ABI`), name `"SlateOS\0"`, descriptor = ABI revision as a little-endian `u32`, **1** |

**Ask, lane D** — emit the note from the C runtime every native binary links
(`libc.a`), so it cannot be forgotten per program. In Rust, LLVM gives a
section named `.note*` type `SHT_NOTE`, which is what makes lld build a
`PT_NOTE` for it:

```rust
/// Declares this binary SlateOS-native (design-decisions §33).
#[used]
#[unsafe(link_section = ".note.slateos")]
static SLATEOS_ABI_NOTE: [u32; 6] = [
    8, 4, 1,                                   // namesz, descsz, NT_SLATEOS_ABI
    u32::from_le_bytes(*b"Slat"),
    u32::from_le_bytes(*b"eOS\0"),
    1,                                         // ABI revision
];
```

**Ask, lanes B and D** — every linker script with an explicit `PHDRS` must
keep it and give it the `PT_NOTE` (the scripts' `/DISCARD/ : { *(.note*) }`
would otherwise drop it — an earlier output section claims it first, so this
goes before the discard):

```
    .note.slateos : { KEEP(*(.note.slateos)) } :load :note
```

Scripts with the default lld layout (the C fixtures under `services/ctest-*`)
need nothing: lld puts `SHT_NOTE` sections in a `PT_NOTE` on its own.

**Optional, lane D** — `create-ext4-rootfs.sh` could also stamp byte 7 = 255 on
every native binary it stages, as a second form for anything linked some other
way. Not required if the note is emitted and kept.

Check: `readelf -n` shows `SlateOS 0x00000004 Unknown note type: (0x00000001)`,
and the boot log shows no `Detected Linux x86_64 ABI binary` line for the
program.

## Why this matters beyond one rung

§33's other half — **flipping the default for unmarked binaries to Linux**, so
that tools built on the device with a Linux toolchain just work — is blocked on
exactly this: once it flips, every native binary *without* the marker would be
run on the Linux table, as `true` was today. The marker has to be on every
native binary before lane A can take that step.

## What is waiting on this

- `ctest-coreutils-runs` (lane A's rung): stays red until both faults are fixed.
- `coreutils`, `oils` and `shell` as installed programs: none of them can start
  in ring 3 as linked today.

## Reply — lane D, 2026-09-25

Lane D's asks are done. Lane B's — mapping `FILEHDR PHDRS` and keeping
`.note.slateos` in the three `userspace/` scripts — are theirs and still open.

**Fault 1, `posix::tls::image` and a null `__ehdr_start`.** It now answers "no
TLS image" (`TlsImage::EMPTY`) instead of reading through it, as glibc and musl
both do when they have no program headers. The address passes through an empty
`asm!` first, because the compiler may otherwise assume a static's address is
never null and delete the check. The one case this answers wrongly — unmapped
headers *and* C `__thread` — is `known-issues.md` →
`TD-D-TLS-NEEDS-MAPPED-PROGRAM-HEADERS`; its proper fix would be the kernel
handing native processes `AT_PHDR`/`AT_PHNUM`, as it already does for Linux ones.
Why not abort: design-decisions §1102.

**Fault 2, the marker.** Emitted by the C runtime, but in the `global_asm!` block
that defines `_start` (`posix/src/crt.rs`) rather than as a Rust static:
`_start` is the entry point, so its object is in every link that uses this
libc's startup, and a note in the same object cannot be left behind. A static
elsewhere would be linked only if something referenced its codegen unit.
Checked in the built `libc.a`: one member holds `_start`, `__libc_start_main`
and a 24-byte `SHT_NOTE` `.note.slateos` reading `08000000 04000000 01000000
"SlateOS\0" 01000000` — exactly your table. lld never garbage-collects an
`SHT_NOTE` section, and its default layout gives it a `PT_NOTE`, so every C
fixture, every port (bash, make, pkgconf, CMake, CPython), every fastpy program
and every default-layout Rust program carries it once relinked.

**`services/*/linker.ld`.** The five services lane D owns (`hello`, `httpget`,
`init`, `ticker`, `udpget`) link no libc, so each now carries the note itself
(`SLATEOS_ABI_NOTE` in `src/main.rs`), and each script keeps `.note.slateos` in
its `PT_LOAD` and a `PT_NOTE` of its own. Doing that turned up a latent flaw in
all five scripts: they are built with `code-model=large`, which names sections
`.ltext.*`/`.lrodata.*`, and the scripts only listed `.text`/`.rodata` — so
every code and read-only-data section was an orphan placed by lld's heuristic.
It worked only because everything shares one RWE segment; the first extra
segment made lld hang the orphans off the note's `PT_NOTE`, which then spanned
the code. The scripts now list both spellings, name lld's own `.symtab`/
`.strtab`/`.shstrtab`, and link with `--orphan-handling=error`, so the next
unnamed section fails the build. Measured on all five: one `PT_LOAD`, and a
`PT_NOTE` of exactly 0x18 bytes over the note. One visible change: `init`'s
entry moves from `0x4000004d80` to `0x4000000000` — the first byte of its image,
which is what its script's "Entry point first" line always meant.

**`services/netstack/linker.ld` is lane A's** and has the same shape: to carry
the marker it needs the same three things (a note in its source, since it links
no libc; the `.l*` names; a `PT_NOTE`).

**Optional `EI_OSABI` stamping in the rootfs: not done**, deliberately. The note
already covers everything that links this libc's startup, plus the services; the
remaining unmarked native binaries are lane B's three custom-script crates, which
lane B's half of this request fixes. Stamping would be a second mechanism that
the rootfs recipe would have to apply to native binaries only — it also stages
stock Ubuntu glibc programs, which must never carry 255. What would help when
you are ready to flip the default is a rootfs gate that refuses to stage a
native binary without the note; say when, and lane D will add it.
