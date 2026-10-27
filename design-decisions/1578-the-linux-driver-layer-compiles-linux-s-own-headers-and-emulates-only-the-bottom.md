## 1578. The Linux driver layer compiles Linux's own headers and core code, and emulates only the bottom: one cooperative thread per driver process

**Date:** 2026-10-27 · **Decided by:** Claude (operator-approved scope) -- the operator chose Linux's drivers through a layer (§1577); how the layer is built is Claude's call · **Lane:** A

**In short:** §1577 decided that hardware without a public standard (GPUs,
WiFi, Bluetooth, laptop sound) runs Linux's own drivers through a
compatibility layer. This entry decides how that layer is built. A Linux
driver expects thousands of Linux's internal functions and data structures,
declared in Linux's header files. The layer could rewrite those headers
itself, as FreeBSD does -- or it could use Linux's real headers, and
Linux's real code for everything above the lowest level, and write only the
lowest level: memory, interrupts, timers, and the switching between Linux's
tasks. That second way, Genode's (a microkernel system that has run Linux
drivers as ordinary programs for well over a decade), is chosen: far less code to
write, far less to redo when Linux changes, and fewer places to get Linux's
rules subtly wrong. Each driver runs in its own process, on one thread, with
Linux's tasks taking turns on it -- which makes Linux's locking rules
trivially true rather than emulated.

### The decision

1. **Linux's own headers, configured once.** The driver is compiled against
   an unmodified Linux tree of a pinned release, configured by a committed
   fragment (`linuxdrv/linux.config`, on top of `allnoconfig`) and prepared
   by Linux's own build (`make prepare`, `scripts/linux-tree.sh`); nothing
   it generates is committed. Only the few headers that
   assume kernel mode are *shadowed*, by files earlier on the include path,
   never by editing Linux's: `current`; the interrupt flags (`cli`, `sti` and
   `pushf`/`popf` of IF are privileged in a program, and in this model
   "interrupts off" means nothing, rule 5); port I/O (`in`/`out`, a syscall
   for the few drivers that use it); `hlt` and the other privileged
   instructions.
2. **Linux's own code above the bottom.** Generic library and core files
   (`lib/idr.c`, `lib/radix-tree.c`, `lib/string_helpers.c`, the list and
   bitmap helpers, and so on) are compiled in from the same tree; which ones
   is per driver, listed in its manifest.
3. **Only the bottom is written here, in two halves that never meet.** The
   *Linux-facing* half is C that includes Linux headers and implements
   Linux's functions (`__kmalloc`, `request_irq`, `dma_alloc_attrs`,
   `schedule`, `mod_timer`, ...) in terms of a small C interface, `slx_*`.
   The *system-facing* half is Rust, never sees a Linux header, and
   implements `slx_*` on SlateOS: the `udriver` syscalls (MMIO, DMA through
   the device's IOMMU domain, §1576; IRQ wait), the scheduler of Linux tasks,
   timers, and IPC to the native service the driver serves (`netring` to the
   netstack for a NIC). This is Genode's split between `lx_emul` and
   `lx_kit`.
4. **Everything else is a generated stop.** A script lists the symbols the
   link leaves undefined and generates, for each function, a definition that
   prints its name and a backtrace and stops the driver. A driver whose
   working path reaches one names the next thing to implement; one that never
   does needs nothing more. The stops are defined in a file that includes no
   Linux header, so they need no signatures: C links by name, and a function
   that never returns has no calling convention to get wrong. Data symbols
   are written by hand.
5. **One thread per driver process, Linux's tasks cooperative on it.** The
   Linux tree is configured uniprocessor (`CONFIG_SMP=n`) and
   non-preemptive (`CONFIG_PREEMPT_NONE`). Kernel threads, workqueue workers,
   interrupt handlers and timer callbacks are coroutines on the process's one
   thread, switched only where Linux code calls `schedule()` or returns. With
   no second CPU and no preemption, a spinlock is correct as Linux itself
   compiles it for that configuration, per-CPU data is a plain global, and
   the barriers that order one CPU against another compile to compiler
   barriers, which is all one thread needs. (The barriers that order the CPU
   against the *device* -- `wmb()` before a doorbell, `dma_rmb()` -- stay
   Linux's own instructions, which work unchanged in a program.) The class
   of bug §1577 named as the layer's main risk ("emulating Linux's rules for
   sleeping, deferred work and memory ordering slightly wrongly") then has
   no emulation to be wrong in. The process's main loop waits on the
   device's interrupt, its IPC channel and the next timer at once, and runs
   whatever became runnable.
6. **A pinned long-term release, fetched by hash, never vendored.** The
   tree is `linux-6.18.y` (the newest long-term series on 2026-10-27),
   pinned by version and SHA-256 in the repository (`linuxdrv/linux.pin`)
   and fetched from kernel.org into WSL's own filesystem
   (`~/slateos-build/linux`, outside the repository: the tree has file
   names that differ only in case, which a Windows folder holds only with
   admin rights). Linux's
   files are never edited; a change Linux needs is a patch file applied by
   the fetch script, visible in review. Moving the pin is one commit that
   changes the version and the hash, and the boot
   test says whether every driver still works.
7. **The GPL boundary.** Linux's files and everything generated from them --
   the prepared headers, the generated stops -- are GPL-2.0, and none is
   committed: they are made at build time. The two halves
   written here are MIT, compatible with linking into a GPL program. Each
   driver *executable* is GPL-2.0 as a whole; the kernel and the native
   services stay MIT and reach it only by IPC.

### The alternatives

| Alternative | For | Against |
|---|---|---|
| **Linux's headers and core code, emulate the bottom (chosen)** | The bulk of what a driver calls is Linux's own code, correct by construction; a new Linux release is mostly a re-fetch and a re-prepare; the hand-written part is small and stable (memory, IRQs, timers, tasks); the generated stops make "what is missing" a list, not a hunt | Linux's headers assume kernel mode, so a few must be shadowed and the shadows checked on every pin move; preparing the tree needs Linux's build tools once per pin (a Linux or WSL host); the per-driver list of compiled core files is found by trial |
| Rewrite the headers (FreeBSD's LinuxKPI) | No dependency on Linux's build; the headers can be MIT | Thousands of declarations, macros and inline functions rewritten by hand, each a chance to differ subtly from Linux, and all of them to re-check against every new release -- the work FreeBSD's graphics port spends most of its effort on, and the reason Genode abandoned this approach in 2021 |
| Preemptive threads per Linux task, `CONFIG_SMP=y` | Uses more than one core per driver | Linux's spinlocks, RCU and barriers would have to be emulated faithfully on SlateOS's primitives under real concurrency, the exact risk §1577 named; per-CPU data and `current` need a real per-thread scheme |
| Vendor the Linux files into the repository | Builds offline with no fetch | GPL text mixed into an MIT tree file by file; `amdgpu` alone is several million lines, mostly generated register headers; local edits to Linux's files become possible and invisible |

### What it costs

- One driver uses one core. For WiFi, Bluetooth, audio DSPs and display
  modesetting this is not a limit; for a GPU's command submission it may be,
  and is measured when `i915` runs. Linux's own `CONFIG_SMP=y` is the way
  out if it is, at the price the "preemptive threads" row above names, for
  that driver alone.
- A driver that busy-waits on hardware for long (`udelay` in a loop) holds
  its whole process while it does. That is what Linux does too, on one CPU.

### How it is checked

The first driver is `e1000e`, beside the native `kernel/src/e1000.rs`: the
boot test gets a second QEMU e1000e, the Linux driver binds it in its own
process, frames cross between the two NICs, and the overhead against the
native driver is measured (roadmap 2.1).

### Where it lands

- `roadmap.md` 2.1, the layer's task: this is its first item, "Shape first".
- `roadmap-detailed.md` §2.1a.
- References: Genode release notes 21.08 ("DDE Linux re-imagined"), the
  genodians.org articles on `create_dummies` (2021-04-08) and on trimming
  driver ports (2022-05-03); FreeBSD `sys/compat/linuxkpi`.
