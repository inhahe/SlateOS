# SlateOS

SlateOS is a from-scratch **microkernel operating system for x86_64 desktops**, written primarily in Rust. It is an experiment in fully AI-assisted OS development: the entire codebase is written, reviewed, and tested by an AI agent (Claude), guided by a detailed design specification and a phased roadmap.

The goal is a coherent, modern desktop OS — designed all-of-a-piece rather than accreted over decades — that is competitive with Linux, Windows, and macOS on the performance-critical paths, with capability-based security from day one.

> **⚠️ Status: early and very far from complete.** SlateOS is an in-progress project, not a usable operating system yet. Substantial kernel and low-level userspace work exists, but large parts of the system — most of the GUI, the desktop, and applications — are unimplemented or in early stages, and it does not yet boot to a usable desktop. Expect things to be missing, incomplete, or in flux. See the roadmap below for what exists and what remains.

## Design principles

These are the non-negotiable architectural decisions the system is built around:

- **Microkernel.** Only the scheduler, memory manager, IPC, capability enforcement, and interrupt routing run in kernel space. Drivers run in userspace (with an optional SPARK-verified + IOMMU-sandboxed in-kernel fast path for the cases where the speedup matters).
- **16 KiB pages** throughout the memory subsystem (not 4 KiB).
- **Capability-based security.** Every kernel object is accessed through unforgeable handles; no ambient authority.
- **Channel IPC** (structured messages + capability transfer) as the primary IPC mechanism — not file descriptors, not Unix signals.
- **Specialized syscalls** (Linux-style, many syscall numbers) with optional io_uring-style batched submission, and **versioned syscall tables** for ABI stability.
- **Linux binary compatibility.** A Linux-compatible syscall ABI (baseline Linux 6.6, reported as `6.6.0-slateos`) lets unmodified Linux executables run alongside native SlateOS programs, in the style of FreeBSD's Linuxulator — implementing advertised syscalls/flags for real rather than silently no-oping them.
- **No Unix signals for process control** — shutdown and similar events are IPC messages. Hardware exceptions surface as language-level exceptions (SEH-style), not signals.
- **Case-sensitive filesystem**, `/` path separator, all bytes allowed except `/` and null. **ext4 first** (port battle-tested code rather than inventing a filesystem).
- **Committed memory by default**, with lazy allocation opt-in (no silent overcommit).
- **YAML** for configuration, **JSON-lines** for structured logs (no binary logs).
- **No AI features in the OS itself** (except speech I/O, and optional on-device AI image indexing for the file finder), and no ads.

Language policy: **Rust** for the kernel, drivers, compositor, and performance-critical services; **Python compiled ahead-of-time via [fastpy](https://github.com/inhahe/fastpy)** for userspace tools and applications (native speed, faster iteration); **C** only for porting existing code (ext4, Mesa, etc.); **Ada/SPARK** for safety-critical driver logic.

## Roadmap & status

Development is organized into phases (Phase 0 foundation → Phase 5 advanced features/ecosystem). Phases 0–1 (project foundation and kernel core) and much of Phase 2 (basic userspace) are substantially implemented, including SMP boot, a buddy allocator over 16 KiB pages, demand paging, copy-on-write, compressed swap, the scheduler, syscall dispatch, and channel IPC.

- **[roadmap-detailed.md](roadmap-detailed.md) — the fine-grained feature inventory.** Every actionable feature derived from `design.txt`, tracked as a checkbox. Start here for a comprehensive view of what exists and what remains.
- **[roadmap.md](roadmap.md)** — the higher-level phased task list and the live source of truth for task status/ordering and dependencies.

### Design mockup

**[Aero Desktop (offline).html](Aero%20Desktop%20%28offline%29.html)** is a self-contained HTML mockup of how parts of the shell and desktop are envisioned to look and behave. It is a visual/UX target — not the implemented UI — meant to convey the intended look and feel of the future desktop. (Download the file and open it in a browser to view it; GitHub shows `.html` as source rather than rendering it.)

## Repository layout

| Path | Contents |
|------|----------|
| `kernel/` | The microkernel: boot, GDT/IDT, interrupts, memory manager, scheduler, syscalls, IPC, capabilities — plus in-tree filesystem (`kernel/src/fs/`, incl. the ext4 port) and driver framework (`kernel/src/drm/`, etc.) |
| `net/` | TCP/IP network stack and sockets |
| `posix/` | POSIX compatibility layer |
| `init/`, `services/` | Service manager, init, and bare-metal startup binaries |
| `gui/` | Compositor, GPU/2D drawing, widget toolkit, and desktop (window manager, taskbar, etc.) |
| `apps/` | Desktop applications (file explorer, settings, text editor, …) |
| `userspace/` | Shell, coreutils, terminal, and CLI tools |
| `bench/` | Benchmarks and performance baselines (`bench/baselines.toml`) |
| `toolchain/` | Custom `x86_64-slateos` target spec, sysroot stubs, and `fastpy/` — the fastpy compiler as a git submodule, pinned to the commit the OS is tested with |
| `scripts/` | Build, disk-image, and QEMU boot-test scripts |
| `esp/` | EFI System Partition staging (Limine bootloader + config) |

## Building & running

SlateOS is developed on Windows (edit + cross-compile) and booted in **QEMU**. The kernel builds against a custom target (`toolchain/x86_64-slateos.json`) on a Rust **nightly** toolchain; some host-side crates with C dependencies require the Visual Studio Build Tools. See [`CLAUDE.md`](CLAUDE.md) for the exact toolchain, compiler paths, and environment setup.

Clone with `git clone --recursive`, or run `git submodule update --init` in an existing clone: the programs written in Python are compiled by the fastpy in `toolchain/fastpy`, the commit this tree pins. `./scripts/bootstrap-worktree.sh` checks it out along with everything else a fresh checkout lacks. To build with another fastpy checkout — one you are changing — set `FASTPY_DIR` to it. The rootfs is built under WSL, which sees a Windows variable only when told to: `WSLENV=FASTPY_DIR/p` passes it, path translated, so `/bin/fastpy` comes from the same checkout as the programs.

Typical loops:

```bash
# Build the kernel (custom target configured via .cargo/config.toml)
cargo build

# Boot the kernel in QEMU and check for the serial success marker.
# This is the only supported boot test, on every platform: it derives its own
# worktree, stages the rootfs, and takes the cross-worktree lock that keeps
# three lanes from running QEMU at once.
./scripts/boot-test.sh

# Interactive QEMU run (PowerShell). Note it does NOT take the boot lock, so
# don't start one while a lane's boot test is running.
powershell ./scripts/run-qemu.ps1
```

```bash
# Run unit/integration tests for a crate
cargo test -p <crate-name>

# Run benchmarks for a performance-critical subsystem
cargo bench -p <crate-name>
```

## Development model

This project is written and maintained by an AI agent working autonomously against the roadmap. The conventions, coding standards, self-review checklist, performance targets, and testing requirements that govern that work are documented in **[`CLAUDE.md`](CLAUDE.md)**. The design rationale lives in `design.txt` and the accompanying design notes (`scheduler.txt`, `ipc.txt`, `memory management.txt`, `design-decisions.md`, `open-questions.md`, `known-issues.md`).

Because the AI is the developer, reviewer, and tester, correctness and testing are held to a high bar: every subsystem ships with unit, integration, and (where relevant) stress and boot tests, and performance-critical code is benchmarked against concrete baselines.


## License

SlateOS's own code is MIT - see [LICENSE](LICENSE). Free to use, modify and
redistribute; provided as-is, with no warranty.

Components ported from or built on other projects keep those projects'
licences, and the MIT licence does not cover them. These are the upstreams
whose licence is, or offers, a copyleft one:

| Upstream | Licence | Notice kept in |
|---|---|---|
| GNU coreutils 9.4, diffutils 3.10, findutils 4.9.0, patch 2.7.6, time 1.9, binutils 2.42; gawk 5.2.1 | GPL-3.0-or-later | `userspace/coreutils` |
| gnulib | GPL-3.0-or-later AND LGPL-3.0-or-later AND LGPL-2.1-or-later | `userspace/coreutils` |
| GNU bash 5.2.37 | GPL-3.0-or-later | `userspace/oils` |
| GNU sharutils 4.15.2 | GPL-3.0-or-later | `userspace/autoopts` |
| AutoOpts 41.1 | LGPL-3.0-or-later OR BSD-3-Clause | `userspace/autoopts` |
| glibc 2.39 | LGPL-2.1-or-later AND GPL-2.0-or-later | `userspace/localtime` |
| util-linux 2.39.3 | GPL-2.0-or-later AND LGPL-2.1-or-later, with BSD and MIT parts | `userspace/ulclosestream` |
| procps-ng 4.0.4 | GPL-2.0-or-later AND LGPL-2.1-or-later | `userspace/coreutils` |
| psmisc 23.7, sysvinit 3.08, Debian hostname 3.23 | GPL-2.0-or-later | `userspace/coreutils` |
| systemd 255 | LGPL-2.1-or-later | `userspace/coreutils` |
| procmail 3.24 | GPL-2.0-or-later OR Artistic-1.0 | `userspace/lockfile` |
| vim 9.1.0016 (`xxd`) | MIT OR GPL-2.0-only | `userspace/coreutils` |
| FFmpeg n8.1.3, and FFmpeg 9b7439c31b (2026-03) | LGPL-2.1-or-later | `gui/video/ffbits`, `gui/video/mp4` |

An upstream's code is often in more crates than the one keeping its notice:
that crate's `licenses/notices.yaml` names them (sharutils, for one, is
`userspace/uuencode` and `userspace/uudecode`; util-linux is `lsblk`, `blkid`,
`login` and the rest of its ports). Everything else is under permissive
licences, each keeping its own notice. `python scripts/gather-notices.py
--list` prints every third-party component with its licence and the crate
whose notice covers it, and the OS image carries those notices under
`/usr/share/licenses`; the fonts keep their licences beside them under
`/usr/share/fonts`, and Oils' spec tests in `/usr/share/oils-spec`.
