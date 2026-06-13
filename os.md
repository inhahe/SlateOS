# OS Architecture and Design Decisions

**Ambition:** This is intended to be a production-quality OS that can
compete with Linux, macOS, and Windows — not a toy or hobby project.
The design decisions below reflect that goal.

## System architecture

The OS uses a **microkernel architecture** with a hybrid driver model:

| Layer | Language | Scope |
|-------|----------|-------|
| **Applications** | Compiled Python (fastpy) | User applications, scripting, tooling |
| **Userspace services** | Compiled Python (fastpy) | Shell, window manager, file manager, networking stack |
| **Userspace drivers** | Rust | General hardware drivers, running in isolated processes with MMIO-mapped direct hardware access |
| **Kernel-space drivers** | Ada/SPARK | Performance-critical and safety-critical drivers (DMA buffer manager, interrupt handler core, primary storage). Formally verified to not crash, so safe to run in kernel space without process isolation. |
| **Kernel core** | Rust (with inline asm) | Scheduler, memory manager, interrupt dispatch, page tables, IPC, device MMIO mapping |

## Microkernel driver model

Drivers run in userspace by default, each in its own process with its
own address space. This provides:

- **Fault isolation.** A buggy driver crashes only its own process. The
  kernel detects the crash, restarts the driver, and the system continues.
  No reboot, no data corruption beyond what the driver was mid-way through.
- **Direct hardware access without IPC overhead on the data path.** The
  kernel maps device hardware registers into the driver's virtual address
  space via MMIO (Memory-Mapped I/O). This is a one-time page table setup
  at driver startup — the kernel adds a page table entry mapping a
  virtual address in the driver's process to the device's physical register
  address. After that, the driver reads and writes hardware registers with
  normal memory loads and stores. The MMU translates the virtual address to
  the physical device address through the same hardware path as any other
  memory access — no syscall, no IPC, no kernel involvement per access.
  Cost vs. monolithic kernel: zero on the data path. The kernel is only
  involved for the one-time mapping setup (one syscall at driver startup)
  and authorization (controlling which processes can map which physical
  addresses).
- **IPC only for control-path operations:** interrupt delivery (kernel
  notifies driver that its device fired an interrupt), DMA mapping setup,
  resource allocation. These happen infrequently.

## Ada/SPARK kernel-space drivers

For drivers where even the interrupt-delivery IPC overhead matters (a few
percent of system performance), or where formal correctness is critical,
drivers can be written in Ada/SPARK and loaded into kernel space.

**Why SPARK is a natural fit for drivers:**
- **No heap allocation** — drivers pre-allocate DMA buffers, command queues,
  and request pools at initialization. No dynamic allocation during
  operation. This is already best practice for high-performance drivers.
- **No recursion** — driver code is naturally iterative: state machines,
  loops over descriptor rings, linear register-write sequences.
- **No dynamic dispatch** — a driver for specific hardware knows exactly
  what registers it talks to. Everything is statically known.

These restrictions, which are what enable SPARK's formal verification,
align naturally with what well-written driver code already looks like.

**What SPARK proves for a kernel-space driver:**
- No buffer overflows when handling DMA data
- No out-of-bounds register access
- State machine completeness (every state handles every input)
- No integer overflow in size/offset calculations
- No uninitialized memory reads
- Pre/postconditions on every function

The formal proof replaces process isolation as the safety mechanism.
If the code is mathematically proven not to crash, it doesn't need
process isolation, and it gains the performance benefit of running in
kernel space (no context switch on interrupt delivery, direct access
to kernel data structures).

**Candidates for kernel-space SPARK drivers:**
- DMA buffer manager
- Interrupt handler core
- Primary storage driver (where I/O latency matters most)
- Timer and clock management
- Any driver on the critical performance path

## Why Rust for the kernel core and userspace drivers

The overriding factor is that **the user does not manually review
AI-generated code.** For userspace Python, differential testing against
CPython catches bugs. For kernel code, there is no equivalent test
oracle — kernel bugs manifest as silent memory corruption, mysterious
crashes, and data loss that are extremely difficult to diagnose. The
bugs that kill OS projects are not "the feature doesn't work" bugs
(those are easy to find and fix) but "memory got corrupted somewhere
and now everything is subtly wrong" bugs.

Rust's borrow checker categorically prevents these at compile time:
- Use-after-free
- Double-free
- Buffer overflows
- Data races
- Dangling pointers

Additional reasons:
- **`no_std` mode** is well-supported for bare-metal/kernel development.
- **Active OS dev community.** Redox OS is a full Rust OS. Linux kernel has
  official Rust support. Patterns for tricky kernel constructs (intrusive
  lists via `Pin`, self-referential structures via arenas, interrupt-safe
  data sharing via atomics) are well-documented.
- **Good C interop** via `extern "C"` for bridging to the Python runtime.
- **AI writes good Rust.** Sufficient training data for AI to produce
  quality code, and the borrow checker catches most AI mistakes at
  compile time.
- **Borrow checker friction is manageable.** Self-referential structures,
  intrusive linked lists, and some classic OS patterns are awkward in safe
  Rust and require `unsafe` blocks, but the Rust OS dev community has
  worked out patterns for all the common cases.

## Detailed language comparison for the kernel

| Language | Safety guarantees | AI code quality | Bare-metal support | OS dev ecosystem |
|----------|------------------|----------------|-------------------|-----------------|
| **Rust** | Memory safety (borrow checker) | Good | Excellent (`no_std`) | Large, growing (Redox, Linux) |
| **Zig** | No hidden control flow/allocations, but no borrow checker | Moderate (less training data) | Excellent (designed for it) | Small but real |
| **C** | None | Very good | Excellent | Huge (every OS tutorial) |
| **C++** | RAII only | Good | Good (freestanding is complex) | Large (SerenityOS, Haiku) |
| **Ada/SPARK** | Full formal verification (SPARK subset) | Moderate (niche language) | Good | Small (MUEN kernel, aerospace) |
| **Nim** | GC by default (ARC/ORC available) | Moderate | Immature | Tiny |
| **C#** | GC, type safety | Good | Experimental (NativeAOT) | Tiny (COSMOS project) |

## Why not the other candidates for the kernel

**Assembly:**
Not a language choice — it's ~200-500 lines sprinkled into the Rust
kernel for CPU operations that have no high-level equivalent. Modern
compilers (LLVM) produce better machine code than hand-written assembly
for general-purpose code: register allocation, instruction scheduling,
SIMD vectorization, branch layout optimization. A skilled assembly
programmer who knows a specific CPU's microarchitecture can sometimes
beat the compiler on a small hot loop, but AI is not particularly good
at writing optimally-scheduled assembly — optimal scheduling requires
knowing which instructions can execute in parallel on which execution
ports, latency/throughput per instruction, branch predictor behavior,
and cache line alignment. AI-written assembly would typically be slower
than what LLVM produces from Rust, not faster.

**C++:**
Adds RAII and classes over C, which helps with resource management.
SerenityOS and Haiku are successful C++ OS projects. But C++ doesn't
provide Rust's memory safety guarantees, and the language complexity
in freestanding mode is significant: no exceptions (or complex
unwinding setup), no standard library, vtable layout varies by
compiler, template metaprogramming errors are opaque, and the sheer
size of the language creates more surface area for subtle AI-generated
bugs. It doesn't offer enough over C to justify the complexity, and
it doesn't offer the safety of Rust.

**C:**
The classic OS language. Every OS tutorial, every reference kernel,
every hardware manual's example code is in C. Path of least resistance
for learning OS dev concepts. Universal tooling, tiny runtime. But: no
memory safety at all. With AI-written code that nobody reviews, silent
memory bugs are the most dangerous failure mode — a buffer overflow in
the kernel corrupts data silently and manifests weeks later. C is the
highest-risk choice for this specific workflow.

**Zig:**
The runner-up. Designed explicitly as a "better C" — no hidden control
flow, no hidden allocations, comptime metaprogramming, first-class C
interop (can import C headers directly), built-in cross-compilation,
excellent bare-metal support. Simpler than Rust — no borrow checker,
no lifetimes. Andrew Kelley designed it partly motivated by OS
development needs. Bun (the JS runtime) uses Zig. But: pre-1.0 (the
language is still stabilizing, things change between releases), smaller
ecosystem and community than Rust, less AI training data means lower
AI code quality, and it doesn't have Rust's compile-time memory safety
guarantees.

**Nim:**
Can compile to C, so bare-metal is theoretically possible. Lighter
feel than Rust/C++. But very few people have done OS dev in Nim, the
tooling for freestanding targets is immature, the GC (even ARC/ORC)
is awkward in a kernel context, and the community for this use case
is tiny.

**C# (NativeAOT):**
The COSMOS project demonstrates C# OS dev is possible. NativeAOT can
produce standalone binaries. But: the runtime is large, GC in a kernel
is problematic (pauses during interrupt handling cause data corruption),
bare-metal NativeAOT is experimental, and the community for this use
case is tiny.

**D:**
Has a `betterC` mode that strips out the runtime. Some hobby OS work
has been done in D. But the community is small, bare-metal support is
limited, and it doesn't offer compelling advantages over Rust or Zig
for this use case.

## Assembly

The ~200-500 lines of assembly are written as **inline assembly inside
Rust** using the `core::arch::asm!` macro, assembled by LLVM's backend.
No standalone assembler is needed for this. If a standalone assembler is
ever needed (e.g., a multiboot header, a 16-bit real-mode trampoline),
use **NASM** — it's the OS dev community standard, cross-platform, and
well-documented. The bootloader itself should use an existing bootloader
(Limine or GRUB) rather than a custom one, or target UEFI directly via
the Rust `uefi` crate.

## Development and cross-compilation

All development happens on Windows. No compiler needs to run on the OS
itself during development.

- **Rust cross-compilation:** `--target x86_64-unknown-none` produces
  bare-metal kernel binaries from Windows.
- **fastpy cross-compilation:** compiles Python programs on Windows,
  producing binaries targeting the OS.
- **Testing:** boot the OS in a VM (QEMU, VirtualBox, or Hyper-V) on
  the development machine.
- **Development cycle:** edit on Windows → cross-compile → boot in VM →
  test → repeat.

**Self-hosting roadmap** (the OS compiling its own code):
1. **Current and near-term:** cross-compile everything from Windows.
   Rust uses `--target x86_64-unknown-none` for the bare-metal kernel.
   fastpy cross-compiles Python programs on Windows targeting the OS.
2. **After working userspace + filesystem + process model:** port CPython
   to run on the OS. Users can run Python scripts. Requires implementing
   enough POSIX-like syscalls for CPython's C runtime.
3. **After CPython port:** the fastpy compiler (which is Python) runs on
   the OS under CPython and can compile Python programs natively.
4. **Long-term:** port LLVM and rustc to run on the OS. This makes the
   OS fully self-hosting (can compile its own kernel). Primarily requires
   implementing libc/libstdc++ support on the OS, not modifying LLVM.

**Custom Rust target for the OS:**
Once the OS has a stable syscall interface, define a custom Rust target
(e.g., `x86_64-unknown-yslateos`) so Rust programs can be compiled as
native OS applications rather than bare-metal binaries. This involves:
- A **target specification JSON file** for rustc, defining the executable
  format (ELF, PE, or custom), C ABI, linking conventions, dynamic
  linker, and platform feature flags. Rust supports custom target specs
  natively — this is how Redox, Fuchsia, and other non-mainstream OSes
  integrate with Rust.
- **Porting Rust's `std` library** to call the OS's syscalls. This is
  the main work: `std::fs::File::open()` calls the OS's file-open
  syscall, `std::thread::spawn()` calls the OS's thread-creation
  syscall, `std::net` calls the OS's networking syscalls, etc. The
  closer the OS's syscall interface is to POSIX, the less adaptation
  needed, because Rust's `std` already has POSIX implementations.
- **Same for fastpy's C runtime** — port it to call the OS's syscalls
  for memory allocation, file I/O, and process management. After this,
  fastpy-compiled Python programs are native OS applications.
- **Same for Ada/SPARK runtime** — port the Ada runtime to the OS's
  syscalls so SPARK-verified drivers can be compiled natively on the OS.

## Package management and dependency system (future decision)

The OS needs a system for installing, updating, and managing software
and library dependencies. This is a major design decision that affects
users, developers, and the OS update mechanism itself. To be decided
when the OS architecture is further along. Key questions:

- **Package format:** binary packages (like .deb/.rpm), source-based
  (like Gentoo/Nix), or app bundles (like macOS .app / Flatpak)?
- **Dependency model:** shared libraries with version constraints (like
  Linux), bundled dependencies per app (like macOS/Windows), content-
  addressed immutable store (like Nix/Guix), or some combination?
- **Update mechanism:** rolling release, point releases, or atomic
  OS-level updates (like ChromeOS/Fedora Silverblue)?
- **Multi-language support:** the OS has Python, Rust, Ada, and C code.
  Should there be one unified package manager or per-language tools
  (pip, cargo, alire) that integrate with a system-level manager?
- **Compatibility guarantees:** how to handle library version conflicts
  between programs that depend on different versions of the same library?
- **Reproducibility:** can a user rebuild the same package and get the
  same result? (Nix-style reproducible builds vs. traditional packaging)
- **Security:** package signing, update verification, supply chain
  integrity.

Potential models to study: Nix (content-addressed, reproducible, solves
dependency hell), Guix (like Nix but Scheme-based), Alpine apk (simple,
fast), Homebrew (user-friendly), cargo (Rust-specific but well-designed
dependency resolution), and Flatpak/Snap (sandboxed app distribution).

### Content-addressed immutable store (Nix model)

The leading candidate for the dependency model. Every package is stored
at a path derived from a hash of its entire build recipe (source,
compiler, flags, and all dependencies by hash):

```
/store/a1b2c3d4-python-3.14.3/
/store/e5f6a7b8-openssl-3.2.1/
```

**Key properties:**
- **No dependency conflicts, ever.** Same inputs → same hash → same
  path → one copy. Different inputs → different hash → coexist.
- **Atomic upgrades and rollbacks.** "Upgrade" = build new version at
  new store path, swap a symlink in the user's profile. "Rollback" =
  swap the symlink back. No half-upgraded state possible.
- **Reproducibility.** Same build recipe always produces the same hash.
- **Multiple versions coexist.** Different users can have different
  versions active. No conflict.
- **Safe garbage collection.** Any store path not referenced by any
  profile or snapshot manifest can be safely deleted.

**Users see symlinked profiles**, not the store directly:
```
~/.profile/bin/python  →  /store/a1b2c3d4-python-3.14.3/bin/python
```

**Advantage for a new OS:** On existing OSes, Nix fights legacy path
assumptions (/usr/lib, /usr/bin, FHS). On our OS, the store IS the
filesystem's package layer — designed in from the start, no legacy.

### Snapshots and deduplication

Snapshot and deduplication strategy differs by data type:

**Package snapshots (installed software):**
Essentially free in a content-addressed store. A "snapshot" is just a
manifest — a small text file listing which store paths are active.
Rolling back means loading a previous manifest and updating profile
symlinks. Package files are immutable in the store, so nothing is
copied or duplicated. Space cost of keeping N snapshots is only the
old package versions that are no longer in the current manifest but
are referenced by a snapshot manifest. Typically 200-500 MB for 10
snapshots over a month of updates.

**File-level deduplication within the store:**
Files that are identical across different store paths (vendored copies
of the same library, shared resource files like fonts/icons/locales,
identical generated files) are detected by hash and hardlinked to share
disk blocks. This is automatic — every file written to the store is
hashed, and if the hash already exists, it's hardlinked. Saves ~25-35%
on top of package-level content-addressing. The hash table is small
(one entry per unique file, ~16-64 MB for a large system).

**Mutable data snapshots (user files, databases, app state):**
Mutable data is NOT in the content-addressed store — it lives in the
regular filesystem. Snapshots of mutable data use **copy-on-write at
the filesystem level**: when a file is modified after a snapshot, old
blocks stay (referenced by the snapshot) and new blocks are written
elsewhere. Only changed blocks are stored twice. The block size for
CoW is a tunable tradeoff:

| Block size | Hashes for 1 TB | Memory | Dedup granularity |
|-----------|-----------------|--------|-------------------|
| 4 KB | 256 million | ~8 GB | Maximum (catches small changes) |
| 64 KB | 16 million | ~512 MB | Good (misses sub-64KB partial overlaps) |
| 1 MB | 1 million | ~32 MB | Moderate (misses sub-1MB changes) |

Larger blocks reduce memory overhead dramatically while catching most
real-world duplication. For snapshots specifically, the CoW mechanism
means only changed blocks are duplicated regardless of block size —
the block size only affects how much extra data is written when a
change falls within a block. A 1-byte change in a 1 MB block
duplicates 1 MB; in a 4 KB block, only 4 KB. For typical workloads
(documents, configs, code), 64 KB blocks are a good balance.

**Summary of dedup by data type:**

| Data type | Snapshot mechanism | Dedup mechanism |
|-----------|-------------------|-----------------|
| Installed packages | Store manifests (free, instant) | Content-addressing + file-level hardlinks |
| System configuration | Version-controlled config store or CoW | File-level or block-level |
| User files / app data | CoW filesystem snapshots | Block-level (tunable block size) |
| Databases | App-level journaling/WAL + filesystem CoW | Block-level |

## Hardware support strategy

Rather than replicating Linux's or Windows' driver API (which would lock
the OS into their architectural decisions), the OS implements hardware
support through standards-based mechanisms:

- **Device Tree (DTS/DTB) parser.** Parse the same device tree files used
  by the Linux community. These describe hardware topology and properties
  in a standard format (not Linux-specific). Gives hardware discovery for
  free on ARM and many embedded platforms, reusing thousands of board
  descriptions maintained by the Linux community.
- **ACPI support.** For x86 hardware discovery and configuration.
- **USB class drivers.** USB device classes (HID, mass storage, audio,
  video, CDC/networking) are defined by USB standards. Implement the class
  specifications directly to handle most USB devices.
- **PCI enumeration.** PCI device discovery is a hardware standard.
  Enumerate devices, read vendor/product IDs, match to drivers.
- **Generic device-class handlers.** For common device classes (HID, mass
  storage, I2C/SPI sensors, standard audio codecs), a generic handler plus
  device tree/ACPI configuration data handles most hardware without
  per-device driver code.
- **Per-device driver porting.** For specific hardware that needs a custom
  driver (GPU, specific NIC, specific storage controller), port the logic
  from Linux's driver. The hardware-specific knowledge (register maps,
  initialization sequences, quirks) is in the Linux driver, typically
  2,000-5,000 lines per device. Porting means understanding the Linux
  driver and rewriting it against our driver API.

## Why not replicate the Linux or Windows driver API

If the OS implemented Linux's driver API (or enough of it), it could
potentially load Linux kernel modules (.ko files) directly — thousands
of drivers instantly. Similarly for Windows driver API (WDM/WDF). This
was considered and rejected.

### What Linux driver API compatibility would require

The Linux driver API is not a clean, stable interface. It's tightly
coupled to Linux's internal architecture:

- **Scheduler.** Drivers assume Linux's specific scheduler behavior —
  they sleep, get preempted, run on multiple CPUs simultaneously, and
  interact with a scheduler that has specific fairness properties.
- **Memory allocator.** Drivers call `kmalloc`, `vmalloc`, `kzalloc`,
  `dma_alloc_coherent`, page allocation functions — all of which are
  Linux-specific with Linux-specific semantics (GFP flags, NUMA
  awareness, slab allocator internals).
- **Locking primitives.** Drivers use spinlocks, mutexes, RCU (read-
  copy-update), seqlocks, rwlocks, semaphores — Linux-specific
  implementations with Linux-specific ordering guarantees.
- **Interrupt model.** Drivers register interrupt handlers via
  `request_irq()`, use tasklets, softirqs, and workqueues for deferred
  processing — all Linux-specific scheduling mechanisms.
- **Process model.** Drivers assume they can create kernel threads,
  use `wait_queue`s, call `schedule()`, and interact with Linux's
  specific process/thread abstraction.
- **Filesystem and device model.** Drivers expose devices via `/dev/`
  nodes, `sysfs` attributes, `procfs` entries, and `ioctl` commands.
  They assume the existence of `udev`, `devtmpfs`, and the device
  model hierarchy (buses, devices, drivers).
- **Networking stack.** Network drivers interact with Linux's specific
  `sk_buff` abstraction, `netdev` operations structure, NAPI polling
  mechanism, and traffic control infrastructure.
- **Power management.** Drivers implement Linux-specific PM callbacks
  (suspend, resume, runtime PM) that integrate with Linux's power
  management framework.

Implementing all of this means reimplementing large chunks of Linux's
kernel. At that point, you've adopted Linux's architecture — the API
IS the architecture.

### API instability

Linux explicitly does NOT guarantee a stable kernel driver API. The
internal API changes between releases: functions are renamed, signatures
change, entire subsystems are restructured. A driver written for kernel
6.1 may not compile against 6.8 headers. Maintaining compatibility
means tracking a moving target across every kernel release.

### GPL licensing concern

Linux kernel code is GPLv2. If the OS reimplements enough of Linux's
internal API to load Linux drivers, it may be creating a derivative
work, which would require the kernel to also be GPLv2. This is a legal
gray area that has never been definitively tested in court, but it's a
real concern for a non-GPL OS.

### Windows driver API has similar problems

Windows driver API (WDM/WDF/KMDF/UMDF) is similarly coupled to Windows
internals: the Hardware Abstraction Layer (HAL), I/O manager, Plug and
Play manager, power manager, Windows-specific IRP (I/O Request Packet)
model, and the Windows Driver Model's specific layering and filtering
architecture. Much of it is proprietary and undocumented. Implementing
it would mean reverse-engineering Windows internals.

### How it would constrain OS design

Committing to either API means committing to that OS's:
- **Driver discovery and binding model** — no room for capability-based
  or proof-based alternatives
- **Memory model** — must implement their specific allocator interfaces
- **Concurrency model** — must implement their specific locking and
  scheduling primitives (can't use message-passing, event loops, or
  other models)
- **Device exposure model** — must implement /dev/, sysfs, ioctl (Linux)
  or device nodes, registry, DeviceIoControl (Windows)

In short: adopting their driver API means building their OS with a
different name.

### What we do instead

Design our own clean driver API that fits our microkernel architecture,
and get hardware support through standards-based mechanisms (see above)
plus per-device driver porting from Linux when needed. The per-device
porting work is bounded: a typical device driver is 2,000-5,000 lines,
and the porting effort is "read the Linux driver, understand the
hardware interaction, rewrite against our API." The hardware-specific
knowledge (register maps, initialization sequences, device quirks) is
the valuable part of a Linux driver, and it ports cleanly — it's the
Linux framework integration code that doesn't.

## Boundary between layers

The kernel exposes a syscall interface to userspace. Compiled Python
programs call into the kernel via syscalls. The fastpy C runtime
implements its OS-facing operations (file I/O, memory allocation,
networking) via these syscalls. Userspace Rust drivers communicate with
the kernel via IPC for control operations and access hardware directly
via MMIO mappings. Kernel-space SPARK drivers call kernel internal APIs
directly. All layers are separate compilation units linked or loaded
independently.
