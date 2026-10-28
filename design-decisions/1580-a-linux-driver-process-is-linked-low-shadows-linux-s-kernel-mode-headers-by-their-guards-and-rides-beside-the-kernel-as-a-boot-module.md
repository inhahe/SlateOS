## 1580. A Linux driver process is built by Linux's own make for the small code model, shadows Linux's kernel-mode headers by their include guards, and reaches the kernel as a boot module

**Date:** 2026-10-28 · **Decided by:** Claude (operator-approved scope) -- the operator chose Linux's drivers through a layer (§1577); how its processes are built and delivered is Claude's call, under §1578 · **Lane:** A

**In short:** §1578 decided that a Linux driver runs in a process of its
own, compiled from Linux's own source with Linux's own headers, with only
the bottom layer written here. Three practical questions followed, each
with more than one workable answer. How is Linux code compiled to run as an
ordinary program, when Linux's build makes code that only works at the top
of the address space? How are the few Linux headers that use instructions a
program may not run (`cli`, `sti`) replaced without editing Linux's files?
And how does the finished program, which is GPL-licensed, reach a kernel
that is MIT-licensed, without being built into it? The answers: Linux's own
build with one flag overridden; a forced include that gets in ahead of each
header; and a file the bootloader loads beside the kernel.

### The decision

1. **Linux's own make, with the code model overridden.** Linux's objects
   are built by Linux's build (`make <object>` in the prepared tree), with
   `KCFLAGS="-mcmodel=small -include <shadow.h>"`. Linux compiles for
   `-mcmodel=kernel` (code and data in the top 2 GiB); KCFLAGS comes last on
   the command line, so `-mcmodel=small` wins. The program is linked at
   4 MiB, below 2 GiB, where the small model's 32-bit absolute relocations
   reach -- Linux's code uses them (`current_task` is reached through an
   `R_X86_64_32S`), so a position-independent build was not an option. The
   layer's own C files are compiled with the very flags Linux's build used
   for the driver's first object (read from its saved `.cmd`), so they see
   Linux's declarations exactly as Linux's code does.
2. **Shadowing by include guard.** A header to replace is replaced by a
   file of the layer's that defines the *include guard* of Linux's header
   and then what that header declares; `linuxdrv/shadow/shadow.h` includes
   them all and is forced in before every C file (`-include`). When Linux
   code later includes the original, its guard is already defined and it
   adds nothing. Each shadow names its Linux header and guard on a
   `SHADOWS` line, and `scripts/linux-tree.sh` checks the guard against the
   pinned tree on every build -- a pin move that renames a guard fails the
   build instead of letting both versions in. The first is
   `<asm/irqflags.h>`: interrupts "off" is a flag the layer keeps, and no
   handler runs while it is clear (§1578 rule 5). The second is
   `<asm/bug.h>`: Linux's WARN() and BUG() execute `ud2` for the kernel's
   invalid-opcode handler to report, which in a program would kill the
   driver at its first warning; here they call the layer, which says a
   warning (once, for WARN_ONCE) and goes on, and ends the process on a
   BUG().
3. **Generated stops for functions, hand-written data.** Every symbol the
   link would leave undefined and that is only ever *called* (only
   `R_X86_64_PLT32` relocations) becomes a generated stop: a function that
   prints its own name and its caller's address (for the symbol table the
   driver's file keeps) and ends the process with exit code 125, which
   nothing else exits with. A symbol
   referenced any other way is data, which a function cannot stand in for:
   the build refuses until it is written by hand (the first three were
   vsprintf's flag-name tables).
4. **A boot module, not embedded, not on the root filesystem.** The program
   is a file on the boot medium, loaded by Limine beside the kernel
   (`module_path:` and `module_cmdline:` in `limine.conf`), and found by the
   kernel by name (`boot::module`). Its memory is the bootloader's
   "executable and modules" kind, which the frame allocator never takes.
5. **The build runs in WSL, the Rust half on Windows.** Linux's build needs
   its tree, which lives in WSL's filesystem (§1578 rule 6); the layer's
   Rust half (`linuxdrv/kit`, a workspace member) is built by cargo as a
   static library and linked there by `linux-tree.sh driver <name>`. The
   boot test builds both, and a driver that no longer builds stops the run;
   a host that cannot build them at all (no WSL, no toolchain, offline on
   the first fetch: the script's exit 3) runs the boot test without them,
   saying so, since the boot test must run on a host with only Rust and
   QEMU. A boot whose medium carried one must show it ran.

### The alternatives

| Question | Chosen | Alternative | Why not |
|---|---|---|---|
| Compiling Linux code as a program | Linux's make, `-mcmodel=small` via KCFLAGS, linked at 4 MiB | `-fPIE` | Linux's code and inline assembly assume absolute addressing (32-bit absolute relocations to data); the kernel's own PIE support is partial |
| | | The layer's own compile flags | They would drift from Linux's with every pin move; Linux's flags are what its headers were written for |
| Replacing a kernel-mode header | A forced include defining the header's guard first | `-I` ahead of Linux's include paths | Linux's build puts KCFLAGS last, after its own `-I`s, so the layer would have to rebuild Linux's whole include list (LINUXINCLUDE) and keep it current |
| | | Editing or patching Linux's header | §1578 rule 6: Linux's files are never edited |
| Delivering the program | A Limine boot module | Embedded in the kernel (`include_bytes!`) | Puts a GPL program's bytes inside the MIT kernel image, and every driver change rebuilds the kernel |
| | | On the root filesystem | The rootfs is lane D's recipe, is not mounted in every boot (it was not in lane A's own boot test), and a disk or network driver needed to mount it could not itself come from it |

### What it costs

- A kernel started by kexec has no modules: Limine is not there to load
  them, and kexec passes none on. Its driver processes would have to come
  from elsewhere -- a filesystem, once one is mounted -- and the kernel's
  self-test records the skip.
- Every boot medium that runs a Linux driver must stage it beside the
  kernel and name it in `limine.conf`: the boot test does; the USB image
  builder will when the first real driver runs on the test PC.
- The tree and its objects are one per machine, shared by every lane's
  worktree, so builds of it take a lock and run one at a time; and lanes
  whose `linux.config` differ rebuild it in turn.
- Each shadow is a promise to match a Linux header's interface by hand;
  the guard check catches a renamed header, not a changed function in it.
  Shadows are few, and each is checked against its original when the pin
  moves.

### Where it lands

- `linuxdrv/`: `shadow/`, `emul/` (the C half), `kit/` (the Rust half),
  `hello/` (the first process, no driver), `link.ld`.
- `scripts/linux-tree.sh driver <name>`; `scripts/boot-test.sh` builds and
  stages the processes; `kernel/src/boot.rs` (`module`),
  `kernel/src/linuxdrv.rs` (the self-test).
- roadmap.md 2.1, "The core shim"; §1577, §1578.
