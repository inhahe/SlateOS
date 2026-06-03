# Detailed Roadmap — Every Feature from design.txt

This is the fine-grained companion to `roadmap.md`. Every actionable feature from `design.txt` (and related design files) is listed here as a checkbox item. Where `design.txt` leaves an ambiguity or open question, it is marked with `AMBIGUITY:` for the human operator to resolve.

Status key: `[ ]` not started, `[-]` in progress, `[x]` done, `[~]` deferred

### Testing Responsibility

This OS is built entirely by AI. The human operator does not review code line-by-line and does not run test suites. **AI is the developer, reviewer, and tester.** Every module, function, and subsystem must be tested by AI before it is considered done. "It compiles" is not "it works."

**What AI must test:**
- Unit tests for every public function (happy path + every error/edge case)
- Integration tests for every subsystem (exercise public API as a real caller would)
- Stress tests for all shared data structures (thousands of ops across multiple threads/cores)
- Memory pressure tests (allocate until OOM, verify graceful degradation)
- Boot tests after any change to boot, memory init, or interrupt handling (QEMU with serial output, automated pass/fail)
- Benchmarks for all performance-critical subsystems (see performance targets in CLAUDE.md)
- Boundary conditions for all `unsafe` code (off-by-one, null, max values, alignment)

**What requires the human to manually test:**
- Subjective visual/UX quality (does the desktop _feel_ right, do animations look smooth, are colors pleasing)
- Physical hardware interaction (real Bluetooth pairing, real USB devices, real GPU output on a monitor)
- Multi-monitor setups (AI has no way to simulate actual multi-display behavior)
- Installation on real hardware (partitioning a real disk, UEFI boot on real firmware)
- Accessibility evaluation (screen reader experience, color-blind usability beyond automated contrast checks)
- Anything that requires human judgment about whether something is annoying, confusing, or ugly

When AI finishes a feature that has aspects requiring human manual testing, it must document what needs testing and what specifically to look for in `manual-testing.txt`. Not in commit messages (too easy to miss), and not in `todo.txt` (that is the human operator's personal file — AI does not write to it).

### Benchmark Targets and Methodology

Every performance-critical subsystem has a measured baseline and a concrete target. These live in `bench/baselines.toml` — that file is the single source of truth for what "fast enough" means. The targets are not aspirational guesses; they are derived from actual measurements on the dev machine (Intel i7-8700K @ 3.70 GHz).

**How baselines are obtained:**

1. **WSL2 measurement (primary method).** Run microbenchmarks directly under WSL2 on the dev machine. WSL2 runs a real Linux kernel inside Hyper-V — the same hardware virtualization layer our kernel runs under (QEMU with WHPX). This makes the comparison fair: both are hardware-virtualized on the same CPU. Cycle counts via `rdtsc` are the most reliable metric; nanosecond values are derived from the base clock and should be treated as approximate.

2. **Host-side `cargo bench` (for algorithms).** For pure data structures and algorithms (buddy allocator, scheduler `pick_next_task`, path lookup, slab allocator), benchmark the Rust code directly on the host as a library — no kernel boot, no virtualization overhead. This is the most accurate method for tuning algorithmic performance.

3. **`rdtsc` cycle counting inside the kernel.** For in-kernel benchmarks (syscall round-trip, page fault handling, IPC), use `rdtsc`/`rdtscp` to measure actual CPU cycles consumed. Compare cycle counts across environments, not wall-clock time — cycles are hardware-independent and unaffected by virtualization scheduling jitter.

4. **Relative comparison (any environment).** "Is version B faster than version A?" is valid under any consistent test environment. Use for optimization validation and regression detection after initial implementation.

5. **Complexity validation.** Vary input size (N tasks, N pages, N path components) and verify O(1)/O(log n) scaling holds. Catches algorithmic regressions regardless of absolute speed.

**When implementing a performance-critical subsystem:**
- Check `bench/baselines.toml` for the target before writing code.
- Write the benchmark first (or alongside), not after.
- After implementation, run the benchmark and record the result. If it exceeds the target by more than 10%, profile and optimize before moving on.
- When you apply a non-obvious optimization, add a code comment documenting what it does, why it helps, and the measured improvement (e.g., `// OPT: per-CPU free lists avoid atomic contention. Benchmark: 800ns → 150ns.`).
- Update `bench/baselines.toml` if you obtain better baseline measurements or need to adjust targets based on architectural reality (e.g., our 16 KiB pages zero 4x more memory per page fault than Linux's 4 KiB pages — the per-fault target is higher, but total faults for sequential workloads are lower).

**Subsystems with benchmark targets** (see `bench/baselines.toml` for exact numbers):
- Syscall dispatch (trivial round-trip)
- IPC channel send/recv round-trip
- Context switch
- Anonymous page fault (demand paging)
- Physical page alloc/free
- Kernel heap allocation (small sizes)
- Futex fast path and contended wake
- io_uring SQE submission
- Interrupt dispatch latency
- VFS path component lookup (cached)
- Compositor frame time at 4K

### Design Principles (non-negotiable)

- **No AI features in the OS** (exceptions: speech I/O, opt-in ML image/video indexer). **No ads.**
- **YAML for all configuration files**, processed with a library that preserves user comments and formatting (e.g., ruamel.yaml or Rust equivalent).
- **No binary logs** — text-based (JSON-lines) structured logging.

### API Design Principles

- **Handle-based filesystem:** all file/dir operations take a capability handle, not a path string. Open returns a handle; subsequent ops are relative to it. Handles ARE capabilities — no TOCTOU races. Convenience wrappers for open+operate+close in one call. Validated by corrode.dev "Bugs Rust Won't Catch" (2026-04): 44 CVEs in Rust coreutils (uutils), largest cluster = TOCTOU path-resolution races. Our handle-based API eliminates the entire class. Additional API rules from that analysis:
  - **Atomic creation with permissions:** file/directory creation APIs take permissions as a parameter, never create-then-chmod (race window between create and permission set lets other processes open with default permissions).
  - **Byte-aware paths:** path types must handle all valid bytes (everything except `/` and `\0`), never force UTF-8 conversion. No `from_utf8_lossy` on paths or OS-boundary data — that's silent data corruption. Use byte-string or OS-string types at system boundaries.
  - **Resolve before trust boundaries:** all path resolution, user lookups, library loading, and capability checks must complete before entering restricted contexts (chroot, sandbox, privilege drop). Once inside a restricted context, external resolution may load attacker-controlled data.
- **Copy-default IPC:** small messages copy (safe, simple). Large messages opt in to move/page-transfer explicitly. `send()` copies, `send_transfer()` moves pages.
- **Capability checks are cheap and local:** `has_capability("audio.volume")` must be a fast in-process check, not an IPC round-trip. Apps check capabilities to show/hide UI features.
- **Sync-first base API, async via event loop:** simple programs use blocking calls. GUI apps and services use the IOCP event loop (inherently async). Toolkit handles async I/O internally so developers write event-driven code without touching io_uring directly.
- **Cancellation via handle close:** closing a handle cancels all pending operations on it (they return a "cancelled" error). For io_uring: individual submissions are cancellable by token without closing the underlying handle.
- **Many specialized functions at all levels** (kernel and userspace): type-safe, discoverable, composable. Broad/dynamic APIs only for inherently open-ended systems: CSS-like styling (key-value property sets), batch widget configuration, and serialization/RPC (Cap'n Proto structured data).
- **Widget creation: create then configure** (Qt-style). Widget exists, set properties, add to layout. Batch `configure()` convenience method available for setting many properties at once.
- **Unified error model:** kernel returns typed error codes (Rust enum / integer ABI). Each language translates: Rust → `Result<T, SysError>`, Python → `OSError` subtypes, C → errno. Error enum defined once, every code has a built-in human-readable message.
- **API stability tiers:** Tier 0 (unstable) = kernel internals. Tier 1 (stable from beta) = syscall ABI, capability names, handle semantics. Tier 2 (stable from beta) = toolkit widget API, service discovery, RPC. Tier 3 (semi-stable) = convenience wrappers, high-level Python APIs. Tier documented on every public API item.

---

## Phase 0: Project Foundation

- [ ] Choose a project name - out of ai's suggestions, so far it's Slate, Facet or Rime. My ideas: Neo (going with that so far)
- [x] Set up git repo, CI, build system (cargo workspace)
- [x] Set up QEMU/VirtualBox dev loop (edit on Windows, cross-compile, boot in VM)
- [x] Set up Rust cross-compilation (`x86_64-unknown-none` target)
- [x] Set up Limine bootloader for development
- [ ] Later (pre-release): write minimal EFI stub for standalone UEFI boot
- [ ] Later (pre-release): GRUB menu entry support for dual-boot installs
- [x] Write CLAUDE.md / coding standards
- [x] Set up benchmark infrastructure (`criterion`, `bench/` directory, `bench/baselines.toml`)
- [ ] Integrate fastpy compiler into build system
- [ ] Porting automation toolkit: rule-based source code transformers for large-scale ports
  - [ ] **Coccinelle** (semantic patching for C): preferred tool for pure-C codebases
    - Understands C semantics (types, control flow, macros) — not just text substitution
    - Write SmPL (Semantic Patch Language) rules: concise, composable, auditable
    - Dry-run mode built-in: `--dry-run` shows what would change without modifying files
    - Recommended for: **ext4 port** (POSIX VFS → native VFS calls, Linux kernel API → our kernel API), **Linux driver ports** (driver model translation, DMA/interrupt API remapping), **coreutils/busybox** (POSIX libc → our libc shim), **curl/OpenSSH** (socket API + TLS calls), **audio stack** (PulseAudio/PipeWire C components)
  - [ ] **clang LibTooling** (AST-level rewrites for C/C++): preferred for C++ codebases and mixed C/C++
    - Full Clang AST — handles templates, overloads, namespaces, RAII patterns
    - Can do rewrites impossible in Coccinelle: rename classes, change inheritance hierarchies, rewrite template instantiations
    - Recommended for: **WINE** (Win32 → native API translation across C and C++ code, COM interface remapping, PE loader adjustments), **Chromium/Firefox** (massive C++ codebases, platform abstraction layers), **Mesa** (Gallium driver interface → our GPU abstraction, C++ frontend code), **Qt/GTK** (widget toolkit platform backends)
  - [ ] **comby** (lightweight structural search/replace): for simpler mechanical changes
    - Language-agnostic, syntax-aware (respects string/comment boundaries)
    - No AST — faster to write rules, but less precise than Coccinelle/LibTooling
    - Good for: quick header remapping (`#include <linux/...>` → `#include <our/...>`), ifdef resolution (strip `#ifdef __linux__` blocks, keep our platform), simple function renames across any language
  - [ ] Transformation rule library (shared across all tools):
    - Win32 API → native API (windows.h types, HANDLE → capability handle, CreateFile → our VFS)
    - POSIX → native syscalls (open/read/write/close, mmap, pthread → our threading, signals → IPC messages)
    - Linux kernel API → our kernel API (for driver/filesystem ports: kmalloc → our heap, spinlock_t → our Mutex, struct file_operations → our VFS trait)
    - Threading primitives (pthread → our thread API, Windows threads → our thread API)
    - Platform headers and ifdef resolution
  - [ ] Dry-run mode across all tools: report what would change, how many call sites, what's unhandled
  - [ ] Handles the mechanical 90% (API translation, type substitution, header remapping); leaves genuinely tricky parts (architectural differences, custom platform assumptions) flagged for human review
  - [ ] Rules are additive — each port adds rules that benefit future ports
  - [ ] Per-port tool selection guide:
    - ext4, Linux drivers, coreutils, curl, OpenSSH → **Coccinelle** (pure C, kernel/POSIX API translation)
    - WINE, Chromium, Firefox, Mesa, Qt → **LibTooling** (C++, complex AST transformations)
    - Header remapping, ifdef cleanup, simple renames → **comby** (quick, language-agnostic)
    - Large ports often use all three: comby for bulk header/ifdef cleanup first, then Coccinelle or LibTooling for semantic API translation

_Bootloader: Limine for development (Phases 0-5). For release: GRUB for dual-boot (installer adds menu entry) + minimal custom EFI stub for standalone UEFI boot with Secure Boot._

---

## Phase 1: Kernel Core

### 1.1 Boot and Hardware Init

- [x] UEFI boot entry point (via Limine boot protocol)
- [x] Parse ACPI tables for hardware discovery (x86 uses ACPI, not DeviceTree)
- [x] Initialize GDT (with TSS for privilege transitions and IST for double-fault stack)
- [x] Initialize IDT (all 20 exception handlers + default handler for remaining 236 vectors)
- [x] Set up interrupt handlers (exception handlers log to serial and halt; IRQ handlers TBD with APIC)
- [x] Set up 16 KiB page tables (not 4 KiB — design decision)
- [x] Set up kernel heap allocator (geometric size class, per-CPU caches)
- [x] Initialize serial console for debug output (COM1, 115200 baud, 8N1)
- [x] Initialize PCI bus enumeration
- [x] Boot task list display (optional, show what the OS is doing during boot)
- [x] Optimize boot time — deferred benchmarks to background task; init starts in ~1s
- [ ] Low-priority HD access for background service/library loading during boot

### 1.2 Memory Manager

#### Physical Page Allocator
- [x] Buddy allocator for 16 KiB base pages
- [x] Per-CPU free lists to avoid cross-CPU atomic contention
- [x] Benchmark: target < 1us per alloc/free (Linux buddy: 100-500ns) — measured 234ns

#### Virtual Memory
- [x] Page table management (map, unmap, protect)
- [x] Kernel virtual address space layout
- [x] Userspace virtual address space layout

#### Demand Paging and Stack
- [x] Page fault handler for demand paging
- [x] Lazy allocation (allocate virtual range, commit physical on touch)
- [x] Stack growth via page fault
- [x] Guard page at bottom of stack (clean crash on overflow)
- [x] Configurable maximum stack size (default 8-64 MiB, programs can request more or unlimited)

#### Memory Protection (W^X / NX bit)
- [x] Enforce W^X (write-xor-execute) on all userspace mappings via the NX bit
  - Stack and heap pages are non-executable by default
  - Code pages (.text) are read+execute, never writable
- [-] SYS_MPROTECT syscall to change page permissions (read, write, execute)
  - Cannot set write+execute simultaneously (W^X violation)
  - Transition path for JIT: allocate writable → write code → mprotect to read+execute → run
  - Kernel-side mprotect() function done (mm/protect.rs); syscall handler pending (kernel-ipc zone)
- [-] Capability-gated JIT memory (`mem.jit` capability)
  - Programs without `mem.jit` cannot create executable pages outside their initial .text mapping
  - Programs with `mem.jit` can use SYS_MPROTECT to toggle writable↔executable on anonymous mappings
  - Required by: language runtimes (V8/JavaScript, LuaJIT, JVM HotSpot, .NET RyuJIT, CPython JIT), browser engines, game engines with shader JIT
  - Kernel-side enforcement done in mprotect(); syscall plumbing pending (kernel-ipc zone)
- [x] Audit: no kernel mappings with both write and execute permissions

#### Swap
- [x] Swap file support (not partition — swap files are more convenient, negligible perf diff on SSD)
- [x] zswap/zram compressed swap (recommended for desktop)
- [x] Swappiness tunable (default 10-20 for desktop, not 60 like Linux)
- [x] Swap priority (multiple swap devices) — tiered: zram (priority 100) + disk (priority 0), highest-priority device fills first
- [x] Minimum free memory threshold (when to start swapping)
- [x] Swap I/O must not tie up system — batch-yield reclaim (mm.swap_batch_size sysctl, default 4, yields CPU between batches)

#### Memory Allocation Modes
- [x] Committed memory by default (guaranteed backed by RAM + swap)
- [x] Lazy/overcommit memory as opt-in (programs request explicitly)
- [x] OOM handling: graceful, no silent kills — fail the allocation

#### Kernel Heap
- [x] Slab allocator for common kernel object sizes
- [x] Benchmark: target < 200ns for common sizes (jemalloc: 20-50ns) — raw alloc+free 64B: 3028 cycles (per-op ~408ns under QEMU TCG; design matches target)

#### Tunables and Profiles
- [x] Runtime-tunable memory parameters via sysctl-like interface
- [x] Workload profiles as named presets: Desktop, Database, Development, Gaming
- [ ] Settings UI can select workload type to populate tuning fields

_Four workload profiles: Desktop (default, interactive/responsive), Database (high throughput, big caches — also covers server), Development (parallel compilation, many processes — also covers game dev), Gaming (low-latency, prioritize foreground app)._

### 1.3 Scheduler

- [x] Scheduler trait interface (pick_next_task, enqueue, dequeue, task_tick, balance_load)
- [x] Priority round-robin scheduler (default implementation):
  - [x] 32 or 64 priority levels, real-time levels at top
  - [x] Round-robin within each priority level
  - [x] Configurable time slices per level (shorter = higher priority for lower latency)
  - [x] Per-CPU run queues (PerCpuScheduler, cache-warm enqueue via last_cpu)
  - [x] Work stealing from longest queue when idle (prefer same NUMA node)
  - [x] Proactive push-based load balancing via SCHED_SOFTIRQ (busy CPUs push excess to lightest CPU, 100ms interval, threshold=2 tasks)
  - [x] Priority inheritance on mutex contention
  - [x] Interactive task detection (I/O-blocking tasks get small priority boost, EWMA burst tracking)
  - [x] Runtime-tunable time slice durations
- [x] Process/thread pause while running
- [x] Process/thread resume while running
- [x] Process/thread priority change while running
- [x] Workload profile presets for scheduler parameters
- [x] Benchmark: pick_next_task must be O(1) or O(log n), never O(n)
- [x] Benchmark: context switch target < 5us (Linux: 1-3us) — measured 67ns WHPX, 398ns TCG

### 1.4 IPC and Syscalls

#### Syscall Dispatch
- [x] Syscall entry/exit path (STAR/LSTAR/SFMASK MSR setup — needs ring 3)
- [x] Many specialized syscalls (Linux style, individual syscall numbers)
- [x] Versioned syscall tables for ABI stability
- [x] Syscall number ranges: kernel-core 0-199, kernel-ipc 200-399, kernel-security 400-499, kernel-process 500-599, fs 600-799, net 800-999
- [x] Benchmark: target < 200ns for trivial syscall (Linux getpid: ~100ns) — measured 66ns (QEMU)

#### Channel IPC (Primary IPC)
- [x] Kernel-managed channel objects with send/receive queues
- [x] Structured message passing
- [ ] Capability handle transfer in messages
- [x] Asynchronous (buffered) send — sender writes and continues, receiver reads when ready
- [ ] Synchronous (rendezvous) send as option — sender blocks until receiver reads, no buffering needed (L4-style, faster for request/response patterns)
- [x] Backpressure handling (buffer-full conditions)
- [ ] Zero-copy for large messages (page ownership transfer / page flipping, not data copy)
- [ ] Fast-path register passing for tiny messages (a few words) — pass in CPU registers during context switch, no memory access (L4 optimization)
- [x] Benchmark: target < 2us round-trip (Fuchsia: 1-2us, L4: 0.5-1us) — measured 392ns (QEMU)

#### Other IPC Mechanisms
- [x] One-way pipes (byte streams, no two-way pipes)
  - [ ] Splice/vmsplice optimization: move pages between pipe and file handle (or between pipes) without copying to userspace. vmsplice maps userspace pages directly into pipe buffer. Makes pipes nearly zero-copy for large transfers.
- [x] Shared memory regions (fastest IPC — direct memory reads/writes after setup)
  - [ ] Lock-free ring buffer support for shared memory (userspace library)
  - [x] Futex-based signaling for sleep/wake on shared memory (via futex subsystem)
  - [ ] Sequence locks (seqlocks) for one-writer-many-readers pattern (userspace library)
- [x] Eventfd-like lightweight wake-up counters (kernel-managed integer, wait/wake)

#### Timer Primitives
- [ ] Monotonic timers: "wake me in N nanoseconds" — unaffected by clock changes, for timeouts/intervals
- [ ] Wall-clock timers: "wake me at datetime X in timezone Y" — kernel handles DST transitions, NTP corrections, timezone changes automatically. For alarms, reminders, scheduled tasks.
- [ ] Both timer types waitable via IOCP-like completion port
- [ ] Recurring wall-clock timers (e.g., "every weekday at 09:00 local") handled correctly across DST boundaries

#### Unified Wait / IOCP-like Completion Port
- [x] Register/unregister waitable objects (not limited to file descriptors)
- [x] Arbitrary user-data integer per registration (for app-side dispatch)
- [-] Wait on: I/O completion, timers, process/thread exit, semaphores/mutexes, channel messages, eventfd counters
- [ ] Benchmark: sub-microsecond for ready events

#### io_uring-style Submission Queue
- [ ] Shared ring buffer between userspace and kernel
- [ ] Batched I/O submission (submit many, complete many, one syscall)
- [ ] **Dual-path design**: every syscall always has a normal synchronous entry point. I/O-heavy syscalls additionally get ring buffer submission paths. Programmer chooses which to use.
- [ ] Ring buffer paths for: read, write, open, close, fsync, send, recv, accept, connect, and similar I/O operations
- [ ] **No ring buffer for fork, exec, mmap** — these change process state and the caller needs the result before continuing. Also no ring buffer for ultra-fast queries (getpid, getuid) where the overhead would make them slower.
- [ ] Benchmark: target ~100-200ns per SQE submission (match Linux io_uring)

#### Futexes
- [ ] Userspace fast path: pure atomic CAS, no syscall for uncontended case (needs ring 3)
- [x] Kernel slow path: sleep/wake on contention (futex_wait/futex_wake syscalls)
- [ ] Benchmark: uncontended = no syscall; contended wake target 1-3us

#### Event Loop Integration
- [ ] Standard API: "give me the underlying waitable handle so I can add it to my event loop"
- [ ] Libraries expose waitable handles so any event loop can drive any library

#### Per-Process Namespaces
- [ ] Mount table remapping per process (for sandboxing)
- [ ] Processes can have restricted view of filesystem (can't even see what exists outside namespace)

### 1.5 Capability / Security Model

#### Core Capability System
- [x] Per-process capability table (unforgeable handles to kernel objects)
- [x] Capability delegation (parent passes subset to child, cannot create new capabilities)
- [-] Capability-gated syscalls

#### User/Group Model
- [ ] Users with per-user capability sets
- [ ] Named capability groups (can nest — groups can contain other groups)
- [ ] User cannot create another user with capabilities they don't have
- [ ] File/directory capability tags — AND-composition between multiple required capabilities
- [ ] Within a capability group attached to a file, individual capabilities compose via OR
- [ ] Predefined groups ship with OS (e.g., "Developer Tools", "Security Suite", "Backup App")
- [ ] Users/admins can create custom groups from any combination of capabilities

_One graphical session at a time. Fast user switching (suspend one session, start another). SSH creates a non-graphical session alongside the desktop._

#### Capability Granting
- [ ] Static at launch time (binary metadata specifies starting capabilities)
- [ ] Dynamic via broker (process requests capability from privileged service)
- [ ] Temporarily scoped (capability for one operation, then revoked)
- [ ] "Request capability from user" dialog — app passes reason string shown to user

#### Capability Types — Filesystem
- [ ] `fs.read` — read files and directories (scopable per path)
- [ ] `fs.write` — modify existing files (scopable per path)
- [ ] `fs.create` — create new files and directories (scopable per path)
- [ ] `fs.delete` — delete files and directories (scopable per path)
- [ ] `fs.execute` — run a file as a program (scopable per path). Required in addition to `proc.launch` — prevents running programs from untrusted locations. File must also have the executable attribute set.
- [ ] `fs.metadata` — change file/directory metadata (capabilities, attributes)
- [ ] `fs.bypass_recycle` — permanently delete without recycle bin (for non-temp directories)
- [ ] All `fs.*` capabilities are separate (not grouped) — each represents a distinct risk level. Files and directories have separate capability requirement lists for each of read, write, create, delete, execute.
- [ ] `fs.execute` applies to files only. Directories have no execute concept — traversal/opening a directory is `fs.read`. (Unix overloads "x" on directories to mean "traverse" — this is a historical quirk we don't replicate.)

#### Capability Types — Network
- [ ] `net.connect` — outbound connections (scopable per domain/IP)
- [ ] `net.listen` — listen on ports (scopable per port range)
- [ ] `net.socket_rw` — read/write on established sockets

#### Capability Types — Process Management
- [ ] `proc.launch` — launch another program
- [ ] `proc.create_thread` — create/delete threads
- [ ] `proc.priority` — change own priority within allowed range
- [ ] `proc.signal` — send shutdown/IPC messages to other processes

#### Capability Types — IPC
- [ ] `ipc.channel` — create/use IPC channels
- [ ] `ipc.sharedmem` — create/use shared memory regions
- [ ] `ipc.pipe` — create/use pipes
- [ ] `ipc.driver` — communicate directly with a driver

#### Capability Types — Audio
- [ ] `audio.play` — emit sound
- [ ] `audio.system_sound` — emit system notification sounds
- [ ] `audio.volume` — change global volume (elevated)

#### Capability Types — UI
- [ ] `ui.notification` — show notification in notification pane
- [ ] `ui.fullscreen` — show fullscreen window
- [ ] `ui.always_on_top` — show always-on-top window
- [ ] `ui.hide_taskbar` — remove own entry from taskbar
- [ ] `ui.context_menu` — add items to system context menus

#### Capability Types — Automation / Accessibility
- [ ] `access.automate` — emulate mouse/keyboard input to other programs
- [ ] `access.read_screen` — read screen content of other windows
- [ ] `access.window_control` — move/resize/close other windows
- [ ] Dedicated accessibility capability class — ensure capability model doesn't block accessibility tools

#### Capability Types — Resource Limits
- [ ] `resource.ram` — RAM limit per process
- [ ] `resource.cpu` — CPU limit (% or time-based)
- [ ] `resource.disk` — disk space limit
- [ ] `resource.io_priority` — set I/O priority (realtime requires elevated grant)

#### Capability Types — User / System Administration (elevated)
- [ ] `admin.user` — create/delete/modify users
- [ ] `admin.user_caps` — change other users' capabilities
- [ ] `admin.cross_user` — install/modify across user accounts

#### Capability Types — Libraries
- [ ] `lib.load` — load dynamic libraries
- [ ] `lib.plugin` — load plugins via the scripting API

#### Capability Types — Push Notifications
- [ ] `push.receive` — register for push notifications, maintain long-lived server connections

#### Hook Capabilities — Monitoring (async, notification-style)

_Principle: monitoring something is a separate capability from doing it. Hooks are grouped by what they observe. Programs can only hook filesystem events on paths they already have `fs.*` access to._

- [ ] `hook.filesystem` — async notifications for file/dir events within accessible paths:
  - [ ] Create, delete, rename, change, read file or dir
  - [ ] Metadata changed, capabilities changed on file or dir
  - [ ] Read/write errors (corrupt, locked, not found, out of space)
  - [ ] Change journal integration: "what changed since timestamp X" (works across reboots)
- [ ] `hook.storage` — mount/unmount drive, create/resize/delete partition
- [ ] `hook.process` — program launched, program exited, program crashed, program suspended/resumed
- [ ] `hook.system` — system going to sleep, system shutdown, DPI/scaling change, OS errors (OOM, I/O error)
- [ ] `hook.security` — user created/deleted, user capabilities changed, capability grants/revocations
- [ ] `hook.network` — network activity (scopable per source/destination program by capability)
- [ ] `hook.updates` — library loading, library update, snapshot created/rolled back

#### Hook Capabilities — Interceptor (synchronous, can block operations)

- [ ] `hook.intercept` — synchronous hooks that can REJECT operations before they complete (elevated)
  - [ ] Applies to subset of filesystem and network events (e.g., block file creation, block network connection)
  - [ ] Strict 100ms timeout — if interceptor doesn't respond, operation proceeds
  - [ ] More restricted than async hooks — requires separate elevated grant
  - [ ] Use cases: antivirus, data loss prevention, security monitors

#### Debugging Suite (separate capability namespace — developer tools only)

_The debugging suite is NEVER granted to normal applications. These are for debuggers, profilers, and developer tools. Each is a separate capability — granting `debug.attach` does not grant `debug.memory.write`._

- [ ] `debug.attach` — attach to another process (ptrace-like debugging interface)
- [ ] `debug.memory.read` — read another process's memory
- [ ] `debug.memory.write` — write another process's memory (higher risk than read)
- [ ] `debug.breakpoint` — set breakpoints, single-step execution
- [ ] `debug.trace.syscalls` — trace another process's syscall invocations
- [ ] `debug.trace.ipc` — trace another process's IPC messages
- [ ] `debug.trace.locks` — trace lock acquisition/release, contention
- [ ] `debug.profile` — high-frequency profiling mode (specialized, NOT general hooks):
  - [ ] Allocate/deallocate memory events (millions/sec — cannot be a general hook)
  - [ ] Syscall timing
  - [ ] Lock contention timing
  - [ ] Per-function CPU sampling (via hardware perf counters)

#### Predefined Capability Groups (ship with OS)

- [ ] "Developer Tools" — `debug.attach`, `debug.memory.read`, `debug.trace.syscalls`, `debug.trace.locks`, `hook.process`
- [ ] "Full Debugger" — all `debug.*` capabilities
- [ ] "Security Suite" — `hook.intercept`, `hook.filesystem`, `hook.network`, `hook.security`, `hook.process`
- [ ] "Backup App" — `hook.filesystem` (with change journal), `fs.read`
- [ ] "Network Monitor" — `hook.network`, `net.connect`, `net.listen`
- [ ] "Accessibility Tool" — `access.automate`, `access.read_screen`, `access.window_control`
- [ ] "System Admin" — `admin.user`, `admin.user_caps`, `admin.cross_user`, `hook.security`, `hook.system`

#### Hardware Security
- [ ] Enable Intel CET (shadow stack + indirect branch tracking) on supporting hardware (< 1% overhead)
- [ ] Enable LLVM CFI as default compiler flag for C/C++ code (1-5% overhead)

#### IOMMU
- [ ] IOMMU setup and DMA sandboxing
- [ ] Detect disabled IOMMU, prompt user to enable in BIOS
- [ ] Option for IOMMU-sandboxed drivers (5-15% speedup over pure userspace drivers)

### 1.6 Process Management

- [x] ELF binary loader
- [x] Process creation/destruction
- [x] Thread creation/destruction
- [x] posix_spawn-style process creation (avoid fork's problems)
- [x] exec equivalent _(spawn.rs: exec_process() tears down old address space, loads new ELF, returns entry+rsp for IRETQ; capabilities survive exec)_
- [x] Hardware exceptions → language-level exceptions (SEH-style) _(exception.rs: ExceptionCode enum, ExceptionContext struct, per-process handler registry; idt.rs: dispatch to userspace handler via modified IRETQ frame, context pushed on user stack, SysV ABI, SYS_EXCEPTION_RETURN to resume)_
  - [x] Divide by zero, illegal instruction, genuine segfault → catchable exceptions
  - [x] Normal page faults handled by kernel, NOT exposed to application
- [ ] Structured shutdown via IPC message (not Unix signals)
- [ ] Process credential / capability management
- [ ] Unwind info in release builds for backtraces (< 2% perf impact)
- [ ] Separate debug symbol files loaded on demand for symbolization

### 1.7 I/O Scheduler

- [x] BFQ-style I/O scheduler:
  - [x] Realtime priority (audio/video playback)
  - [x] Best-effort with priority levels (normal applications)
  - [x] Idle priority (background indexing, backup, dedup)
- [ ] Capability-gated realtime I/O priority
- [ ] User can set per-app I/O priority (in settings and while running)
- [ ] Apps can request I/O priority (with capability)
- [ ] User can override app-set priorities
- [x] Prevent heavy I/O from making system unusable (small ops pass through) — budget_cost: ≤16 sectors costs 1 unit, small-I/O gets 128 ops/round vs bulk's 2

### 1.8 Interrupt Handling

- [x] Interrupt dispatch
- [x] Deferred work mechanism (softirq/tasklet equivalent)
- [x] Interrupt delivery from kernel to userspace drivers via IPC — SYS_IRQ_REGISTER/WAIT/RELEASE + lock-free IRQ_PENDING counters + softirq deferred wake
- [x] Benchmark: total ISR latency < 10us (ISR hard-IRQ measurement via TSC; QEMU min=56k cycles/15µs — native target met by design)

---

## Phase 2: Basic Userspace

### 2.1 Driver Framework

- [-] Userspace driver framework: _(udriver.rs: kernel-side resource tracking, MMIO mapping management, DMA buffer lifecycle, IRQ association, IOMMU domain tracking, crash cleanup)_
  - [x] MMIO mapping into driver process address space _(udriver.rs: BAR mapping, permission control, page-aligned, per-driver tracking)_
  - [x] Interrupt delivery via IPC — SYS_IRQ_REGISTER/WAIT/RELEASE
  - [x] DMA mapping setup syscalls _(udriver.rs: alloc/free DMA buffers, direction control, bus addr translation, IOMMU domain integration)_
  - [x] Driver crash detection and automatic restart _(drvmon.rs: heartbeat/process-alive health modes, exponential backoff restart, configurable policies)_
- [ ] Ada/SPARK FFI bridge for kernel-space safety-critical drivers
- [-] virtio drivers (disk, network, GPU) for VM development/testing — virtio-blk and virtio-net done (in-kernel, legacy PCI transport); GPU pending
- [x] VMware tools equivalent for VM-friendliness _(vmguest.rs: hypervisor-specific feature activation for KVM/Hyper-V/VMware/VBox/Xen/QEMU-TCG, pvclock/refTSC/pseudo-TSC clock sources, balloon memory, display resize, heartbeat, guest info reporting, shutdown signaling, VMware backdoor)_

### 2.2 Essential Drivers

- [-] Keyboard (PS/2 and USB HID) — PS/2 done (IRQ-driven, scan code set 1, ring buffer); USB HID pending
- [x] Framebuffer / basic display (UEFI GOP initially)
- [ ] Storage: NVMe driver
- [ ] Storage: AHCI/SATA driver
- [ ] USB host controller (xHCI)
- [ ] Network: Intel e1000/e1000e (for VMs)
- [ ] Network: basic Realtek (for real hardware)
- [x] Timer: HPET — initialized, high-res monotonic clock, sub-µs precision
- [x] Timer: APIC timer — 100 Hz periodic, per-CPU, PIT-calibrated
- [x] RTC (real-time clock) — MC146818-compatible CMOS driver, BCD/binary auto-detect
- [ ] Bluetooth: HCI driver (USB Bluetooth adapters via xHCI)
  - [ ] Bluetooth pairing, discovery, connection management
  - [ ] Bluetooth audio (A2DP sink/source, HFP for headsets)
  - [ ] Bluetooth HID (keyboards, mice, game controllers)
  - [ ] Bluetooth file transfer (OBEX)
  - [ ] Bluetooth Low Energy (BLE) for modern peripherals
  - [ ] Settings UI: scan, pair, manage devices, auto-reconnect
  - [ ] Port BlueZ or implement from spec (BlueZ is Linux's Bluetooth stack, well-tested, open source)
- [ ] Printing: printer driver framework and common drivers
  - [ ] Print spooler service (queue management, job priority, pause/resume/cancel)
  - [ ] Port CUPS (Common Unix Printing System) or implement lightweight equivalent
  - [ ] IPP (Internet Printing Protocol) client — covers most modern network printers
  - [ ] USB printer class driver (via xHCI)
  - [ ] PostScript / PDF rendering pipeline (rasterize to printer-native format)
  - [ ] PCL driver (HP LaserJet family — huge installed base)
  - [ ] ESC/P driver (Epson inkjets, receipt printers)
  - [ ] Driverless printing via IPP Everywhere / AirPrint (mDNS discovery + IPP + PDF/PWG-Raster)
  - [ ] PPD (PostScript Printer Description) file support for printer-specific options (tray, duplex, resolution, paper size)
  - [ ] Print dialog: printer selection, page range, copies, duplex, quality, paper size/orientation
  - [ ] Settings UI: add/remove printers, set default, view queue, test page
  - [ ] Print-to-PDF virtual printer (always available)

### 2.3 Filesystem

#### VFS Layer
- [ ] Virtual filesystem abstraction
- [ ] Path lookup with dcache equivalent
- [ ] Benchmark: cached lookup target ~200-500ns per component

#### ext4 (Primary)
- [ ] Port ext4 code (do NOT write from scratch)
- [ ] Read-write support
- [ ] Journaling (via ext4's own journal)

#### Other Filesystems
- [ ] FAT32 (USB drives, EFI System Partition — essential)
- [ ] ISO 9660 (optical media)
- [ ] Later: NTFS read/write support
- [ ] Later: Btrfs port (CoW, snapshots, checksums)
- [ ] Later: F2FS port (SSD optimization)

_No database-as-filesystem. Queryable indexed metadata (Phase 6.2) covers the practical use cases. Atomic write transactions are a separate feature. No full relational model._

#### Filesystem Rules
- [ ] Case-sensitive paths
- [ ] Forward slash `/` path separator
- [ ] Filenames: allow everything except `/` and null byte
- [ ] 255 byte max filename length

#### File Metadata
- [ ] Owner (user/group)
- [ ] Capability requirements (AND-composition)
- [ ] Hash: per-block hashing (detects corruption, enables dedup), per-file hash from block hashes
- [ ] Time created
- [ ] Time last read (relatime — only update if older than mtime, or once per day)
- [ ] Time last written
- [ ] Size
- [ ] Extended attributes (for arbitrary data)
- [ ] Immutable flag (can't modify/delete until cleared by privileged user)
- [ ] Append-only flag (for log files)

_File comments: extended attribute `user.comment`, max ~64 KiB. No dedicated inode field. Works via ext4 xattrs. Not a launch goal._

#### Filesystem Features
- [ ] Atomic write transactions (programs can group writes)
- [ ] Filesystem change notification system (inotify equivalent, async not synchronous)
- [ ] Change journal: "what changed since timestamp X" queries (for backup programs, works across reboots)
- [ ] Interceptor hooks: synchronous capability-gated hooks for security tools (e.g., block file creation), 100ms timeout
- [ ] Deduplication (optional, user-configurable)

_Dedup: (1) Package manager hardlinks in content-addressed store. (2) Filesystem-level via Btrfs/ZFS ports (Phase 6.2). (3) Userspace batch tool — off by default, uses filesystem hashes when available, falls back to reading+hashing._

#### Recycle Bin
- [ ] Per-filesystem recycle bins (not one central bin)
- [ ] Two syscalls: trash-capable delete (default for shell/explorer) and permanent delete
- [ ] Auto-prune: keep items until system needs space, delete oldest first
- [ ] Smart pruning: prefer deleting larger files that aren't much newer than oldest
- [ ] Lazy/gradual pruning option (avoid thrashing when space gets low)
- [ ] Free space reporting optionally excludes recycle bin
- [ ] Bypass-recycle-bin capability for non-temp directories

#### File Type Associations
- [ ] Extension → default app mapping
- [ ] Per-app icons per extension (e.g., audio vs video files can have different icons even if same app)
- [ ] User can change association: pick from registered apps, any installed app, or any executable + arguments
- [ ] Fallback to previous app when handler is uninstalled

_Traditional suffix extensions (foo.txt). OS-specific: `.nx` (executable), `.dso` (dynamic shared object), `.slib` (static library)._

### 2.4 Networking Stack (Kernel-Resident, Future Userspace Migration)

- [x] TCP/IP stack (kernel-resident; full TCP state machine with 3-way handshake, data transfer, FIN/RST, 32 connections, 8 listeners)
- [x] UDP (connectionless datagrams, 32 sockets, per-socket receive queue)
- [x] DNS resolver (A records, CNAME chasing, cache with TTL, negative caching, query ID/port hardening, source validation)
- [x] DHCP client (DISCOVER/OFFER/REQUEST/ACK, subnet mask, gateway, DNS, random XID)
- [x] Sockets API (dedicated TCP/UDP handles via syscalls, not file descriptors)
- [x] Basic packet filtering firewall (stateful, connection tracking, per-namespace rules, priority-ordered, default policy)
- [x] ARP with cache expiration (5-minute TTL, LRU eviction)
- [x] ICMP echo request/reply with RTT measurement, checksum verification, Destination Unreachable/Time Exceeded handling
- [x] IPv4/TCP/UDP checksum verification on all incoming packets
- [x] UPnP / NAT-PMP port forwarding (detect and configure router) _(upnp.rs: NAT-PMP RFC 6886 packet building/parsing, UPnP IGD SSDP discovery + SOAP control, mapping lifecycle with renewal/expiry, duplicate detection)_
- [ ] Later: WiFi (requires wireless driver + wpa_supplicant port)

### 2.5 POSIX Compatibility Layer

- [x] Enough POSIX libc for: gcc, coreutils, bash, CPython (extensive coverage: stdio, string, stdlib, time, locale, socket, fcntl, stat, mmap, spawn, environ, pthread stubs, and more)
- [x] Translate POSIX calls to native syscalls (fd table maps POSIX fds to kernel handles by type; read/write/close dispatch by HandleKind)
- [x] Userspace memory allocator (malloc/free/calloc/realloc/posix_memalign/aligned_alloc/valloc/memalign)
  - Current design: per-allocation mmap — every malloc() gets its own mmap region with a 16-byte header (mmap_base + total_size). free() calls munmap(). This is correct, safe, and handles all alignment requirements, but every allocation is a syscall. Suitable for bootstrapping and programs with moderate allocation rates.
  - Future upgrade path: replace with arena-based allocator (dlmalloc or jemalloc port) that batches small allocations into larger mmap'd arenas. The current design exercises the mmap/munmap syscall path well and provides a correct baseline to validate against.
- [ ] /proc, /sys equivalents (for programs that need them) — blocked on kernel support
- [ ] POSIX signals → translate to native IPC messages — blocked on kernel-ipc zone
- [x] POSIX file descriptors → translate to native handles (256-fd table with per-fd flags, status flags, handle refcounting for dup)
- [ ] Bug-for-bug behavioral compatibility on ported tools: when reimplementing or porting Unix tools, match original exit codes, error messages, edge-case semantics, and option parsing exactly. Behavioral divergence = security bug when shell scripts depend on original behavior. (Lesson from uutils CVEs: `kill -1` interpreted differently → system-wide kill.)

### 2.6 Init / Service Manager

- [x] PID 1 init process _(initproc.rs: boot sequencing, orphan reaping, shutdown/reboot, emergency mode, critical service tracking, periodic maintenance)_
- [x] Dependency-based parallel service startup _(svcstart.rs: topological sort, start levels)_
- [x] Socket activation _(sockact.rs: socket→service mapping, trigger/claim/release, idle-stop)_
- [x] Automatic crash restart with exponential backoff _(svcstart.rs: 1s→60s cap, max 5 retries)_
- [x] Resource limits per service (cgroup-equivalent) _(reslimit.rs: hierarchical groups, CPU/memory/IO/process limits, soft/hard enforcement, usage tracking)_
- [x] JSON-lines structured logging (text-based, NOT binary) _(eventlog.rs + logpersist.rs)_
  - [x] Event logging service (system-wide event collection daemon) _(eventlog.rs: 4096-entry ring buffer)_
    - [x] Hierarchical event namespace taxonomy (mirrors hook namespaces from Phase 6.5):
      - `system.*` — boot, shutdown, sleep/wake, OOM, hardware errors, DPI changes
      - `process.*` — launch, exit (normal/crash with exit code), suspend/resume, priority change
      - `security.*` — login/logout, capability grant/revoke, user create/delete, auth failures
      - `network.*` — interface up/down, DHCP lease, DNS failures, firewall blocks, connection events
      - `storage.*` — mount/unmount, partition changes, disk errors, SMART warnings
      - `filesystem.*` — permission changes, quota exceeded, corruption detected
      - `service.*` — service start/stop/crash/restart, dependency failures, socket activation
      - `driver.*` — driver load/unload, device attach/detach, driver errors
      - `application.*` — app-defined events via logging API (namespaced per app)
    - [x] Severity levels per event: debug, info, notice, warning, error, critical
    - [x] Structured fields: timestamp (ns), namespace, severity, source PID/service, source executable path, message, key-value payload
    - [x] Ring buffer in kernel for early-boot events (before logging service starts)
    - [ ] Logging API for userspace services and applications (IPC channel to logging daemon)
  - [x] Log storage and rotation _(logpersist.rs)_
    - [x] Configurable per-namespace log files (e.g., security.jsonl, network.jsonl, or single combined.jsonl)
    - [x] Rotation policies: by size (default 50 MB per file), by time (daily/weekly), by count (keep N rotated files)
    - [x] Compression of rotated logs (zstd) _(logpersist.rs: zstd/lz4/gzip via fs codec libs, configurable, only keeps compressed if smaller)_
    - [x] Maximum total log storage cap (default 500 MB, configurable)
    - [x] Automatic pruning: oldest rotated logs deleted when cap exceeded
    - [x] Crash-safe writes: append + fsync, no partial JSON lines on power loss
  - [x] Log query API (for Event Viewer and CLI tools) _(eventlog.rs: EventFilter + query())_
    - [x] Filter by namespace (prefix match: `security.*` gets all security events)
    - [x] Filter by severity range, time range, source PID/service name
    - [x] Full-text search within message and payload fields
    - [x] Streaming mode: tail new events matching a filter (like `journalctl -f`) _(elog tail command)_
- [x] "Service ready" notification API (app tells OS "I'm fully loaded") _(svcstart::signal_ready)_
- [x] Startup app list (separate from service manager, simple sequential list) _(svcstart.rs)_
  - [x] Disk-idle heuristic for "app is loaded, start next one" (2-3 sec timeout) _(configurable timeout)_
  - [x] Option to wait until previous app signals ready, or load immediately
- [x] Low-priority I/O for background service/library loading (don't obstruct user) _(reslimit.rs: IoLimits::low_priority flag + weight-based I/O scheduling)_
- [x] On-demand service loading with priority insertion into schedule _(sockact.rs trigger)_
- [x] Only two ways to load programs on startup: service manager + startup app list

_Startup app list is a service manager config section (not a separate system). Entries: app path, arguments, whether to wait for readiness. Settings UI shows reorderable list with toggles. Entries can be shell commands for dynamic logic._

### 2.7 Shell and Basic Userspace Tools

#### Shells
- [ ] Port Oils (bash-compatible, replaces bash for POSIX compatibility)
- [ ] Port Nushell as default interactive shell (Rust, structured data piping)

_Nushell as default interactive shell (structured data, Rust-native). Oils for POSIX/bash compatibility (replaces bash)._

#### Core Utilities
- [ ] Port coreutils (ls, cp, mv, rm, mkdir, cat, etc.)
- [ ] Port rsync
- [ ] Port curl
- [ ] Port ssh / sshd
- [ ] Port find (compatible with Linux find)
- [ ] Build custom grep in Rust (with Python grep's unique features + standard grep features)
- [ ] Filename sanitizer utility
- [ ] Monitor-off utility (like nircmd monitor off)

_nircmd: full feature set (see `nircmd.html`). CLI wrapper over system functions. Telnet: client only (for BBSs), no server._

#### Terminal Emulator
- [ ] Persistent input history (searchable)
- [ ] Arrow keys and insert work in input
- [ ] Tab autocomplete for file/directory names
- [ ] Find text in backscroll (Ctrl+F)
- [ ] Configurable colors and font
- [ ] Ability to log all output
- [ ] Unicode and ANSI support
- [ ] Resizable, remembers last size and location
- [ ] Word wrap option (if off, horizontal scroll to longest line)
- [ ] tmux-like session detach/reattach
- [ ] Readline-style line editing (shared library for terminal + any CLI app):
  - [ ] Left/right arrow to move cursor within line
  - [ ] Home/End to jump to start/end of line
  - [ ] Up/down arrow to navigate input history
  - [ ] Ctrl+R to reverse-search history
  - [ ] Shift+arrow keys to select text (character-level)
  - [ ] Shift+Home/End to select to start/end of line
  - [ ] Ctrl+Shift+arrow to select word-by-word
  - [ ] Copy selected text (Ctrl+C when selection active, Ctrl+C without selection = interrupt)
  - [ ] Paste from clipboard (Ctrl+V)
  - [ ] Shift+Enter for newline (multiline input mode)
  - [ ] Ctrl+A to select all text in current input
  - [ ] Ctrl+K / Ctrl+U to kill to end/start of line (with kill ring)
  - [ ] Undo (Ctrl+Z) for input edits

#### CLI Copy/Move
- [ ] Command-line copy/move using same mechanism as file explorer drag-drop
- [ ] Options: auto-merge subdirectories, auto-rename (foo (2)) or overwrite on conflict

### 2.8 Error Handling Philosophy

- [ ] Always give meaningful error messages, never generic
- [ ] Tracebacks with string error messages at each level
- [ ] Non-bug failures: meaningful message with tips on why it might happen
- [ ] Include unwind info in release builds (< 2% perf impact)
- [ ] Separate debug symbol files for on-demand symbolization

---

## Phase 3: Graphics and GUI

### 3.1 GPU Drivers

- [ ] Port AMDGPU driver (open source, well-documented — first priority)
- [ ] Port Intel i915/xe driver (integrated graphics — covers most laptops)
- [ ] NVIDIA: defer, use Linux compat layer later (FreeBSD approach)

### 3.2 Graphics Stack

- [ ] DRM/KMS equivalent (kernel mode setting, GPU memory management)
- [ ] Vulkan loader and basic GPU command submission
- [ ] OpenGL via Mesa port
- [ ] 2D drawing library for application UI
- [ ] OS-level image codec support (all common formats apps can decode/encode via system API):
  - [ ] JPEG, PNG, GIF (animated), BMP, TIFF, WebP, AVIF, HEIC/HEIF, ICO, SVG
  - [ ] RAW formats (CR2, NEF, ARW, DNG — via libraw or similar)
- [ ] OS-level video codec support (via FFmpeg/libav):
  - [ ] H.264, H.265/HEVC, VP8, VP9, AV1, MPEG-4, WMV, MOV container, MKV container, WebM
  - [ ] Hardware-accelerated decode where GPU supports it (VAAPI/NVDEC)
- [ ] Thumbnail generation service (used by file explorer, shared across apps)

_2D library: Vello (Rust-native, GPU compute shaders) + HarfBuzz FFI for complex text shaping. Future Vello contributions if needed: blur/shadow, image filters, SVG coverage._

### 3.3 Compositor

- [ ] Wayland-inspired compositor (userspace)
- [ ] GPU-accelerated window compositing
- [ ] DMA-BUF buffer sharing between apps and compositor
- [ ] Apps submit GPU command buffers directly (Vulkan/OpenGL bypass compositor for rendering)
- [ ] Compositor only composites final output
- [ ] Fullscreen bypass / direct scanout (for games)
- [ ] Fullscreen optimization: detect single-app-fullscreen and optimize to no-op (Windows approach)
- [ ] Native remote desktop streaming (compositor streams draw commands over network)
- [ ] Video-encoded screen capture fallback (H.264/VP9 for games/video)
- [ ] Benchmark: composite full desktop in < 2ms at 4K (for 144Hz vsync)
- [ ] Surface owner liveness as a precondition for compositing. Every frame, before compositing a surface, the compositor verifies the owning process is alive AND the IPC channel to it is healthy (no broken pipe, no missed heartbeats beyond threshold). Surfaces whose owner is dead or unresponsive are evicted, not drawn. Rationale: the most common cause of stray artifacts that survive every normal redraw mechanism is an *orphan surface* — the owning process died (or its connection broke) while a tooltip / popup / hover label was up, and the compositor kept drawing the last submitted frame because nothing told it to drop the surface. Trusting clients to clean up their own surfaces is fragile; verifying liveness on the compositor side makes the system robust against client crashes, IPC drops, and buggy widget destructors.
- [ ] Popup / tooltip / hover-label TTL. All transient surfaces (tooltips, hover labels, context menus, dropdowns) require a heartbeat from the owner to stay alive, with a short default TTL (e.g. 10 seconds without heartbeat → auto-dismiss). Prevents the orphan-popup class of artifact at the source: if the owner dies, the popup goes away on its own within the TTL window, no user action required.
- [ ] Surface tree rebuild from live-client enumeration. Rather than incrementally adding/removing surfaces based on client requests, the compositor must support rebuilding its surface list from scratch by re-enumerating all live clients and asking each "what surfaces do you currently own?" Any surface in the old list not claimed by a live client in the new enumeration is dropped. Runs on the full-redraw trigger; can also run periodically as a sanity sweep.
- [ ] Hardware overlay / DRM plane reset. The full-redraw path must also flush all hardware planes (DRM overlay planes, cursor planes, direct-scanout buffers) back to compositor-managed state. Some classes of artifact live in hardware planes that bypass the normal compositor buffer; recompositing the main plane doesn't touch them.
- [ ] Full-screen redraw / artifact-recovery path. User-triggered operation (shortcut: e.g. Ctrl+Super+R) and IPC method that:
  - Triggers the surface-tree rebuild (above) — drops any surface without a live, responsive owner
  - Discards all cached surface contents and the damage region cache
  - Sends every connected client a "repaint full window" request (clients must redraw their entire surface, not just the dirty region they think they have)
  - Resets all hardware planes (above)
  - Recomposites the whole screen from scratch
  - This is recovery, not the common path. Normal compositing stays strictly damage-tracked for performance; the full redraw is opt-in and user-triggered. Also serves as a debugging aid: if the artifact survives a full redraw, the bug is in the compositor's own state; if it disappears, the bug was in client / IPC / popup-lifecycle land.

### 3.4 Window Manager / Desktop Shell

#### Taskbar
- [ ] Pinned apps on left, running apps on right, divider between sections
- [ ] Drag to reorder in both sections
- [ ] Optional app name alongside icon
- [ ] Aero-style blurry transparency (taskbar and/or window titlebars)

#### System Tray
- [ ] System tray icons: clock, wifi, volume, battery, emoji input, keyboard layout, network drives, GPU usage, date/time
- [ ] Can drag and drop icons into and out of system tray
- [ ] Apps can start in system tray or minimize to system tray
- [ ] User can override any app: always start in system tray, always in taskbar, or neither
- [ ] Sound mixer accessible from volume icon: per-app volume, shows currently-playing apps first
- [ ] Sound history: view which programs recently played sounds, button to go to that app's sound capabilities

#### Desktop
- [ ] Desktop icons: snap-to-grid or free placement (user option)
- [ ] Drag and drop icons between pinned apps, desktop, and start menu
- [ ] Multi-monitor support

#### Start Menu
- [ ] Applications tree
- [ ] Settings icon
- [ ] Terminal shortcut
- [ ] Power options: off, logout, reboot, hibernate, sleep, reboot in safe mode
- [ ] Input field for finding and running apps
- [ ] Start menu icon: round, shrunken version of the XOR logo (`xor2.png`)

_Kexec-style OS reboot without rebooting the PC, available as a power menu option._

#### Other Desktop Features
- [ ] Notification pane (per-app disable option)
- [ ] Widget support
- [ ] Ctrl+R run dialog (completion dropdown, recent commands)
- [ ] Context menu extension API:
  - [ ] Programs must request capability to add items
  - [ ] Items load lazily (don't load program just to show menu)
  - [ ] Settings page to see and disable individual extensions
  - [ ] 200ms timeout per handler, show "loading..." if exceeded
- [ ] OLE-style drag-and-drop system (multi-format data transfer: text, HTML, file path, image, custom)
- [ ] File explorer drop zones (empty space = this dir, folder = into folder, file = open with if executable)
- [ ] Atomic file operations with undo/resume:
  - [ ] Copy/move/delete can be undone before finished
  - [ ] If interrupted (shutdown, etc.), offer abort or resume on next boot
  - [ ] Smart conflict resolution: rename to "foo (2)", skip, overwrite

#### Themes and Appearance

_A theme is a declarative YAML file plus optional bundled assets. Themes are pure data — never executable code. A theme defines visual treatment; it never changes layout, behavior, or hotkeys._

##### Theme Format
- [ ] YAML theme file following the OS config convention (comment-preserving parser)
- [ ] `meta` block: name, author, version, license, tags, screenshots, `supports` list
- [ ] `supports` field declares which axes this theme covers (e.g., `[colors, window-decorations, icons, cursors, widget-style, sounds, terminal]`)
- [ ] Mix-and-match: each axis is independently overridable — user can apply one theme's colors with a different theme's icons. A "full theme" sets everything, but no axis is mandatory.

##### Tier 1 — Colors (baseline, include from the start)
- [ ] Semantic color tokens (~30-40 defined by OS): `background`, `surface`, `primary`, `secondary`, `accent`, `error`, `warning`, `text`, `text-dim`, `text-on-primary`, `border`, etc.
- [ ] Apps reference semantic tokens, not hardcoded colors — theme redefines tokens and everything updates
- [ ] Light and dark mode variants in a single theme file (`colors` and `colors-light` sections)
- [ ] Auto mode: switch light/dark based on time of day or system toggle
- [ ] Theme color API for applications (apps query current token values)

##### Tier 1 — Window Decorations
- [ ] Title bar: height, button layout (close/min/max order and position), button shape (circle, square, icon), title font, title alignment
- [ ] Window border: radius, width, color
- [ ] Window shadow: offset, blur, color, spread
- [ ] Aero-style blurry transparency (taskbar and/or window title bars)

##### Tier 1 — Icon Theme
- [ ] App icons, file type icons, system tray icons, folder icons
- [ ] SVG-based with color token substitution (monochrome icon sets automatically match theme accent color)
- [ ] Bundled in theme directory as SVGs, or referenced by name as a separate installable icon theme package

##### Tier 1 — Cursor Theme
- [ ] Cursor shape, size, color
- [ ] Animated cursors (loading spinner)
- [ ] SVG-based or XCursor format

##### Tier 2 — Font Preferences (add in early update)
- [ ] System font, monospace font, font sizes (base, small, large), font weight
- [ ] Themes recommend fonts (not bundle — licensing issues). Settings app offers to install recommended fonts from package manager.

##### Tier 2 — Widget Styling
- [ ] Button shape: border radius, padding, shadow
- [ ] Input field styling: border style, focus ring color/style
- [ ] Scrollbar appearance: thin/wide, overlay/always-visible, color
- [ ] Checkbox/radio/toggle appearance (e.g., pill toggle vs. checkbox)
- [ ] These variables define the visual feel (flat modern vs. skeuomorphic vs. glassmorphism)

##### Tier 2 — Taskbar/Panel Styling
- [ ] Transparency/blur level
- [ ] Icon spacing
- [ ] Visual treatment (color, border, shadow) — not position or size (those are layout settings, not theme)

##### Tier 3 — Sound Scheme (nice-to-have, add later)
- [ ] System sounds: notification, error, login, logout, empty recycle bin, etc.
- [ ] Small OGG files bundled in theme directory, or reference a separate sound scheme package
- [ ] Optional — many users run silent, but themes that pair colors with sounds are more cohesive

##### Tier 3 — Animation Tuning
- [ ] `animation-duration-ms` (global default for window open/close, menu transitions)
- [ ] `animation-easing` (ease-out, spring, linear)
- [ ] `enable-animations` (bool — global kill switch)
- [ ] Not full custom animations (that would be a compositor plugin). Just tuning built-in animation parameters.

##### Tier 3 — Wallpaper Integration
- [ ] Theme can bundle or recommend wallpapers
- [ ] Dynamic wallpapers: list of images with time-of-day triggers (e.g., day image 06:00-18:00, night image 18:00-06:00)

##### Tier 3 — Terminal Color Scheme
- [ ] 16 ANSI colors + background + foreground, specifically for terminal emulators
- [ ] Included as a `terminal` section in theme YAML so a single theme unifies the whole desktop including terminals

##### What Themes Do NOT Control
- **Layout** (taskbar position, widget placement, panel arrangement) — these are settings, not themes. Applying a theme must never break muscle memory.
- **Behavior** (click behavior, scroll direction, keyboard shortcuts) — a theme never changes how things work, only how they look.
- **Custom rendering code** (shaders, custom draw functions) — themes are declarative data, never executable. Fundamentally different widget appearances require a compositor/toolkit plugin, which goes through the full app vetting process.

##### Desktop Background (independent of themes, but themes can recommend wallpapers)
- [ ] Static image
- [ ] Animated background (video)
- [ ] Dynamic program-driven background (program receives desktop events as input — window changes, time, etc.)
- [ ] Fit options: fit with letterbox, fill with crop (user can scroll to center)
- [ ] Random background on boot or daily rotation (with exclusion filters)
- [ ] Login screen background (easy way to match desktop background)

#### Hotkeys
- [ ] Set hotkey: capture from keyboard, show existing binding, select function or arbitrary command
- [ ] Modify/delete hotkeys from list
- [ ] Minimal defaults (Alt+F4, Alt+Tab, Ctrl+C/V/X, Ctrl+Z, Print Screen)
- [ ] Available functions: monitor off, minimize all, change desktops, logoff, etc.

_Minimal hotkey defaults: Alt+F4, Alt+Tab, Ctrl+C/V/X, Ctrl+Z, Print Screen. Everything else user-configured._

### 3.5 GUI Toolkit / Widget API

#### Layout Engine
- [ ] Flexbox-based layout (main axis, cross axis, flex-grow, flex-shrink, alignment)
- [ ] Grid-based layout
- [ ] Implemented as native layout engines, NOT through CSS parsing
- [ ] Align elements vertically, horizontally
- [ ] Justify: left, right, center / top, bottom, center
- [ ] Size item to content
- [ ] Size to length of given text
- [ ] Dynamic sizing based on content and available space
- [ ] Margin (with color), padding, outline (curvature, thickness, color) on all or individual sides
- [ ] Auto-scale images by OS DPI/scale factor
- [ ] Image scaling: max fit in space, optional no-upscale with justify, fill to dimensions
- [ ] Read size of any item/section
- [ ] Nested sections
- [ ] Horizontal/vertical rules
- [ ] Rounded corners
- [ ] Transparency support
- [ ] Focus management (set/unset focus on app, input field, etc.)

#### Styling — CSS Subset with Inheritance, No Cascade
- [ ] **Keep:** color, background-color, font-family/size/weight, margin, padding, border, border-radius, width, height, min/max-width/height, opacity, box-shadow, text-shadow. Shorthands. Colors: hex, rgb(), rgba(), hsl(), hsla(), named. Units: px, %, em, rem, cm, mm, vw, vh, ch. var()/custom properties (OS theme variables). calc(). Transitions. Pseudo-classes: :hover, :active, :focus, :disabled, :checked. Selectors: type, .class, #id, > child. Position: absolute, relative, fixed. z-index.
- [ ] **Drop:** cascade/specificity, !important, descendant/sibling combinators, ::before/::after, float, display, @media queries (use OS var() for theming), pt/ex units.
- [ ] **Inheritance:** child widgets inherit parent font/color unless overridden. Style applied directly to widgets, no multi-source resolution. cm/mm use monitor EDID data for true physical sizes.

#### Signals and Slots
- [ ] Signal/slot mechanism (maps to Rust channels or callback registration)

#### Core Widgets
- [ ] Buttons (text, graphic)
- [ ] Labels
- [ ] Menus
- [ ] Checkboxes
- [ ] Tristate checkboxes (yes/no/default — useful for cascading option overrides)
- [ ] Radio buttons (grouped, only one selected)
- [ ] Treeview
- [ ] Tristate checkbox treeview (with function to populate from directory)
- [ ] Tabs view
- [ ] Grid view
- [ ] Color picker (like qtpyrc's)
- [ ] Scroll bars (auto-hide when nothing to scroll)
- [ ] Tooltips
- [ ] Modal and non-modal dialogs
- [ ] Simple alert popup with icon

_Click selected radio button to deselect (returns group to no-selection state)._

#### Text Views
- [ ] Simple text view: plain text, single font, ANSI colors (for terminals/logs)
- [ ] Rich text view: fonts, sizes, colors, inline images (NOT HTML, simpler markup)
- [ ] Web view: embedded browser engine (after Chromium port)
- [ ] Word wrap option (if off, horizontal scroll)
- [ ] Scroll-to-bottom / stay-at-bottom when new text added
- [ ] Emoji display without oversizing or resizing the line (unlike Qt)

#### Input Fields
- [ ] Single-line and multiline
- [ ] Word wrap option
- [ ] Placeholder text ("ghost text" showing field purpose)
- [ ] Rich input with formatting and image paste (optional formatting toolbar)
- [ ] Copy/paste: Ctrl+C/V and right-click context menu

#### Dockable Panel / Splitter Layout Widget
- [ ] Container widget that holds named panels separated by draggable splitters
- [ ] User can drag panels to rearrange (reorder, move to different split)
- [ ] User can drag splitters to resize
- [ ] Add/remove panels from a menu or context menu
- [ ] Horizontal and vertical splits, arbitrarily nested
- [ ] Layout serialization (save/restore user's arrangement)
- [ ] Panel tabs when multiple panels share a region
- [ ] Minimum size constraints per panel
- [ ] Apps define available panel types; user arranges them

#### Code-Aware TextEdit Widget
- [ ] Rope or gap buffer backing (efficient for large files)
- [ ] Syntax highlighting via tree-sitter integration
- [ ] Line numbers (toggleable)
- [ ] Undo/redo stack
- [ ] Multi-cursor support
- [ ] Selection modes: line, word, block/column
- [ ] Find/replace (regex-capable)
- [ ] Soft wrap or horizontal scroll (user choice)
- [ ] Indent/dedent selection
- [ ] Auto-indent
- [ ] Bracket matching
- [ ] Configurable tab width, tabs vs spaces

#### Advanced Features
- [ ] Clipboard: multi-format (text, HTML, image, structured data)
- [ ] Clipboard history with view and select
- [ ] Paste as plain text option
- [ ] Drag-and-drop (OLE-style multi-format)
- [ ] File picker / save dialog (reuses file explorer component)
- [ ] DPI/scaling awareness
- [ ] Enable/disable controls API (grey out, set/clear tooltip explaining why disabled)
- [ ] Encourage (but don't enforce) tooltip on disabled controls explaining why disabled
- [ ] SVG rendering support
- [ ] Web app framework: shared Chromium so each web app doesn't need 100MB Electron
- [ ] No separate "app" type — all applications use the same toolkit/framework

_Multi-format clipboard: source puts full rich + plain text, OS auto-generates sanitized rich text as third format. Apps request: full rich (at own risk), sanitized (safe default — strips backgrounds, embedded objects, invisible text; keeps bold, italic, links, font size, contrast-adjusted color), or plain text. Ctrl+Shift+V = plain text._

### 3.6 Credential Manager (Factotum-like)

- [ ] Central credential storage service (apps never touch raw passwords)
- [ ] API: define username/password fields in UI, OS autofills if user opts in
- [ ] API: verify user identity by OS password, with debounce (skip if entered recently)

### 3.7 Audio

- [ ] Audio driver framework
- [ ] Audio mixing (per-app volume control)
- [ ] System notification sounds (set of sounds for apps to use)
- [ ] Sound history (which apps played/are playing sound, link to app settings)
- [ ] ASIO driver — actual low-latency audio I/O for professional/music production use
  - [ ] Direct hardware access path bypassing the mixer when an app holds exclusive ASIO mode
  - [ ] Target: ≤ 3ms round-trip latency at 48 kHz (64-sample buffer), competitive with Windows ASIO drivers
  - [ ] Per-device ASIO buffer size configuration (32/64/128/256/512/1024 samples)
  - [ ] Multi-channel support (stereo minimum, 8+ channels for audio interfaces)
  - [ ] Sample rate switching (44.1/48/88.2/96/176.4/192 kHz)
  - [ ] Clock source selection (internal, S/PDIF, ADAT, word clock)
  - [ ] ASIO SDK-compatible C API so existing DAW software (Reaper, Ardour, etc.) can use it with minimal porting
  - [ ] Fallback: when no app holds exclusive ASIO mode, device is available to the normal mixer
  - [ ] Real-time thread priority for ASIO callback threads (deadline scheduler integration)

---

## Phase 4: Applications

### 4.1 File Explorer

- [ ] Path bar with autocomplete (absolute or relative paths)
- [ ] Thumbnails for images, video, PDFs
- [ ] Detail column view:
  - [ ] Columns are union of relevant columns per file type in folder
  - [ ] User can choose default columns per file type
  - [ ] Audio columns: stereo/mono/joint, VBR, kHz, bitrate, length, ID3v1/v2 tags
  - [ ] Image columns: width, height, EXIF metadata
  - [ ] General columns: size, dates (created/modified/accessed), permissions
  - [ ] Apps can register custom detail columns and file decoders
- [ ] View options: list, thumbnails (any size), column view, order by any column
- [ ] Optional preview panel: shows currently selected image/video, movable and resizable (uses dockable panel widget)
- [ ] Search feature with checkboxes for what to search:
  - [ ] Filename/path (always available, no indexer needed)
  - [ ] File contents (requires file indexer enabled)
  - [ ] OCR text in images (requires file indexer + ML option enabled)
  - [ ] ML image/video descriptions (requires file indexer + ML option enabled)
  - [ ] Natural language understanding in search queries (hybrid BM25 full-text + semantic similarity via Reciprocal Rank Fusion — same algorithm as thumbsup2)
  - [ ] Metadata fields for common file types (searchable without full indexer — read on demand or cached):
    - [ ] Audio: artist, album, title, genre, year, track number, bitrate, duration, sample rate
    - [ ] Images: dimensions, camera model, date taken, GPS location, aperture, ISO, focal length
    - [ ] Video: dimensions, duration, codec, framerate, bitrate
    - [ ] Documents: author, title, page count, creation date
    - [ ] General: size (range), date created/modified/accessed (range), file type/extension
- [ ] Used as system file-save and file-open dialog for applications
- [ ] Drop zones for drag-and-drop
- [ ] Atomic copy/move/delete with undo, resume on interruption

### 4.2 Text Editor

_Custom Python (fastpy) text editor. Editing engine is a toolkit widget (Phase 3.5 TextEdit). App is a thin wrapper. All apps get the engine for free via the widget._

- [ ] Text editor app: tab bar for multiple open files
- [ ] Text editor app: file open/save with encoding detection
- [ ] Text editor app: split panes (horizontal/vertical)
- [ ] Text editor app: minimap
- [ ] Text editor app: session restore (remember open tabs, cursor positions)
- [ ] Text editor app: plugin system (Python scripts)
- [ ] Text editor app: status bar (line/col, encoding, language, indentation mode)

### 4.3 Process Explorer

- [ ] Identify process by clicking window, kill it
- [ ] Find process by name
- [ ] Pause, resume, kill, change priority, restart
- [ ] Show all libraries loaded by process
- [ ] Show all subprocesses and threads
- [ ] Show: capabilities, running user, priority levels, app name, what launched it, is it a service, what's blocking it, what's waiting on its locks, running/paused status, full path
- [ ] Switch to any window or terminal a process owns
- [ ] System resource graphs (CPU, RAM, disk, network over time)

- [ ] Code signing display: process explorer shows "repo-verified," "signed by [entity]," or "unsigned"
  - [ ] Repo packages verified by repo signature + content-addressed hash
  - [ ] Direct .nx installs support optional CA code signing
  - [ ] Unsigned apps NOT blocked — capability system is the real security, signing is informational only

### 4.4 Other Core Applications

- [ ] Photo/video viewer (not a separate app — file explorer's thumbnail view + preview panel)
- [ ] Music player (custom Python/fastpy — see decision below)
- [ ] Settings/configuration UI (comprehensive — see Settings section below)
- [ ] System information explorer (hardware + OS info + tuning params + mounted drives)
- [ ] Backup program (snapshot-based, all common backup types)
- [ ] Background file indexer (configurable paths/extensions, OFF by default)
  - [ ] Full-text content indexing for searchable file types
  - [ ] Optional ML features (OFF by default, separate toggle from indexer itself):
    - [ ] Image captioning via BLIP (same model/approach as thumbsup2)
    - [ ] OCR via EasyOCR with GPU acceleration (same as thumbsup2), Tesseract as CPU fallback
    - [ ] Semantic search embeddings via Sentence-Transformers (same as thumbsup2)
    - [ ] Video: extract keyframes, caption and OCR those
  - [ ] Search ranking: hybrid BM25 full-text + semantic cosine similarity, fused via Reciprocal Rank Fusion (same algorithm as thumbsup2)
  - [ ] Results cached by file content hash (re-index only changed files)
  - [ ] Exception to "no AI" rule — user must explicitly opt in, clearly labeled as ML feature
- [ ] Event Viewer (custom Python/fastpy — replaces Windows Event Viewer with better UX)
  - [ ] Hierarchical namespace browser (tree view, collapsible):
    - Top-level: system, process, security, network, storage, filesystem, service, driver, application
    - Expandable sub-namespaces (e.g., security → login, capability, user, auth)
    - Tristate checkboxes on each node: ✓ show all, ▪ show some children, ☐ show none
    - Checking/unchecking a parent propagates to all children; mixed children → parent shows partial
  - [ ] Event list (main panel): timestamp, severity icon+color, namespace, source, message
    - [ ] Color-coded severity: debug=gray, info=default, notice=blue, warning=yellow, error=red, critical=red+bold
    - [ ] Click to expand: full structured payload (key-value pairs), stack trace if present
    - [ ] Multi-select for export or bulk operations
  - [ ] Filtering toolbar:
    - [ ] Severity filter: checkboxes for each level (default: info and above)
    - [ ] Time range picker: last hour / today / this week / custom range
    - [ ] Source filter: by service name or PID
    - [ ] Text search bar (searches message + payload fields)
    - [ ] Save/load filter presets (e.g., "Security audit", "Network issues", "Service crashes")
  - [ ] Live tail mode: stream new events in real time (auto-scroll, pause on user scroll-up)
  - [ ] Log rotation settings panel:
    - [ ] Per-namespace rotation config: max file size, rotation interval, files to keep
    - [ ] Global storage cap with visual usage bar
    - [ ] Manual rotate / purge buttons
    - [ ] Compression toggle for rotated files
  - [ ] Export: filtered events to JSON, CSV, or plain text
  - [ ] Notification integration: configurable alerts for specific event patterns (e.g., "notify me on any critical event", "notify on 3+ auth failures in 5 minutes")
- [ ] Reminder/calendar/alarm program (custom Python/fastpy — see decision below)

_Custom music player in Python (fastpy). foobar2000 is closed source. Features:_
- _Audio decoding via FFmpeg/libav FFI (not custom decoders)_
- _Library browser, album art, metadata editing_
- _Equalizer_
- _User-customizable layout using the toolkit's dockable panel widget (drag-and-drop panels, add/remove, slide splitters) — simpler than foobar2000's layout system_
- _Playlist panels:_
  - _Any number of playlist panels simultaneously (each is a dockable panel)_
  - _Metadata columns: user picks which columns to show from all available metadata fields (title, artist, album, duration, bitrate, sample rate, codec, path, etc.)_
  - _Column behavior: drag splitter to resize, click column header to sort, drag column header to reorder_
  - _Directory-synced playlists: playlist watches one or more directories, auto-updates when files are added/removed_
  - _"Opened files" playlist: auto-populated with songs opened via command line or file association / drag-drop / other external means. Auto-plays the newly opened song._
  - _Manual playlists: user adds/removes/reorders songs freely_
- _Visualizations (all with option for per-channel or average-to-mono):_
  - _Oscilloscope (waveform)_
  - _Frequency visualization_
  - _Spectrum analyzer_
  - _Milkdrop2 (port from open-source C++/DirectX → Vulkan, as a visualization panel/plugin)_
- _Layout persists per user_

_Custom Python (fastpy) calendar/reminder/alarm/timer app. Uses OS wall-clock timer primitives for correct DST/NTP handling. Features:_
- _**Calendar view:** month/week/day views, write notes on any date. Reminders and alarms also appear on the calendar._
- _**Notes:** can attach a note to any date (via calendar), and also to any individual reminder or alarm._
- _**Reminders/scheduler:** configurable advance notice (start reminding N days before, and N hours/minutes before the time on the day). Recurrence patterns:_
  - _Every X days, specific days of the week, specific days of the month_
  - _Every X weeks, specific weeks of the month_
  - _Every X months, specific months of the year_
- _**Alarm clock:** any number of simultaneous alarms, each with its own duration until it goes off. Choose alarm sound per alarm, option to override current global volume when sounding. Same recurrence patterns as reminders._
- _**Timer:** simple countdown timers, any number simultaneous_

_No separate photo management app. Viewing via file explorer (thumbnails + preview panel). ML search via opt-in file indexer. Image/video decoding is OS-level._

_Advanced undelete utility (Python/fastpy). PhotoRec/TestDisk-style: metadata scanning + raw file carving. Features:_
- _Search within a specific directory (with option to recurse subdirectories)_
- _Search by filename mask (e.g., `*.jpg`, `report*`)_
- _Shows recoverable files with confidence level (metadata intact vs carved by signature)_
- _Preview recovered files before saving_
- _Save recovered files to a different drive/directory (never write to the drive being scanned)_

### 4.5 Package Manager

- [ ] Content-addressed immutable store (Nix model)
- [ ] Shared dynamic linking within a generation (fast security patches)
- [ ] Atomic updates and rollback (generation pointer swap)
- [ ] File-level deduplication via hardlinks within store
- [ ] Binary packages (preferred), with source build option
- [ ] Binary with source included is most preferred
- [ ] Show requested capabilities before install (Android-style)
- [ ] Repository model:
  - [ ] Official curated repository
  - [ ] Third-party repository support (user adds URL)
  - [ ] Direct .pkg installation from anywhere

_Automated gates + community signals, human review only for escalation:_
- _**Automated submission gates** (must pass all): builds from source (or static analysis for binary-only), declared capabilities match actual syscall/resource usage, no known malware signatures, basic quality checks (description, license, no duplicate name)_
- _**Community signals** (informational, not gatekeeping): download count, ratings, reviews, time-in-repo without complaints. Automated flagging of suspicious updates (new capabilities requested, binary size jumps, etc.)_
- _**Human review only when needed:** flagged packages, apps requesting sensitive capabilities (automation, network monitoring, cross-user access). Handled by OS team or trusted volunteers — rare, not every-package._
- _**Tiered trust display** in package manager UI:_
  - _"Official" — ships with OS or maintained by OS team_
  - _"Verified" — passed automated gates, has community history, no flags_
  - _"New" — passed automated gates but no track record yet_
  - _"Third-party repo" — user-added repo, no guarantees_

### 4.6 Theme Repository

_Themes are simpler to vet than apps — they can't execute code. The process is lighter than package manager submission._

#### Submission Flow (Git-Based)
- [ ] Public Git repository (GitHub/Forgejo) for community themes
- [ ] Authors fork the repo, add theme directory under `themes/<name>/` (YAML + screenshots + optional assets), open a PR
- [ ] Zero custom infrastructure needed initially — a Git repo with CI is free, version-controlled, and community-reviewable

#### Automated Validation (CI on PR)
- [ ] Schema validation: does the YAML match the theme schema? Are all required color slots filled?
- [ ] Asset validation: images are valid PNG/SVG under size limit, sound files are valid OGG/FLAC under duration/size limit, no executable content, no embedded scripts
- [ ] Contrast checking: automated WCAG contrast ratio checks (text-on-background, text-on-surface, etc.). Does not reject — flags accessibility warnings for the author to see
- [ ] Preview rendering: CI job renders standardized screenshots (taskbar, file manager, settings app, terminal, a dialog box) using the actual theme colors and widget styles, so reviewers don't need to install the theme

#### Human Review (Light Touch)
- [ ] Maintainer checks: does it look intentional (not random colors)? Do screenshots match the colors? Any obvious issues automation missed?
- [ ] This is a 2-minute review, not a code audit — themes are data, not code
- [ ] Community can also review each other's PRs

#### Distribution to Users
- [ ] Repository publishes a JSON index file: name, author, tags, color summary, screenshot URLs, download URL, download count, date added, date updated
- [ ] Settings app fetches the index, shows searchable/filterable gallery
- [ ] Individual themes downloaded on demand (themes are tiny — a few KB for colors-only, a few MB max with icons/cursors)
- [ ] Simple HTTPS fetch + client-side schema validation — no need for full package manager pipeline
- [ ] Theme updates: authors update via PR, users get updates through theme browser's update check

#### Theme Browser in Settings App
- [ ] **Large visual previews** as the primary selection mechanism — grid layout of standardized screenshots, click to see full-size preview
- [ ] **Featured/Curated section** at top — hand-selected by community maintainers, rotated periodically (e.g., monthly). ~5-10 highlighted themes. Highlights new quality work, not just popularity.
- [ ] **"New" section** — recently added themes get automatic visibility for 2-4 weeks
- [ ] **Download count** displayed on each theme, but NOT used as the primary sort — popular doesn't mean good for any particular user
- [ ] **No rating/star system** — aesthetic ratings are too noisy (taste varies too much for averages to be meaningful), attract gaming (sock puppets, friend brigading), create popularity feedback loops (high-rated stay on top, new themes can't compete), and most themes won't get enough ratings for the average to stabilize. Curation is more useful than aggregation.
- [ ] **Filtering:**
  - [ ] Light / dark / both variants
  - [ ] What the theme covers: colors only, full theme with icons, includes sounds, etc. (based on `supports` field)
  - [ ] Tags (author-assigned, maintainer-adjustable): warm, cool, high-contrast, pastel, retro, minimal, vibrant, earthy, monochrome, neon, professional, playful, etc.
- [ ] **Sorting:** newest, most downloaded, recently updated
- [ ] **Search** by theme name or author
- [ ] **Local favorites** — users can mark themes they like, creating a private shortlist (not aggregated or published)
- [ ] **Report button** — for themes with accessibility issues, broken assets, or screenshots that don't match the actual theme. More useful than ratings for quality control.
- [ ] **One-click apply** with instant preview before committing
- [ ] **Per-axis apply** — apply only colors from one theme, only icons from another, etc.

#### Theme Editor in Settings App
- [ ] **Live preview panel** showing a miniature desktop with the current theme applied, updating in real time as user makes changes
- [ ] **Color picker** for each semantic token, with contrast ratio display for text/background combinations
- [ ] **Built-in WCAG contrast checker** — flags when text/background combinations fail WCAG AA
- [ ] **Import/Export** — import from the theme repository or a local YAML file, export current customizations as a shareable theme YAML
- [ ] **Derive from existing** — start from any installed theme, tweak individual values, save as a new theme
- [ ] **Preview multiple contexts** — see how changes look across taskbar, file manager, terminal, dialog, etc.

_The theme editor makes it easy for non-technical users to create themes, which feeds the community repository. Authors can export their customizations and submit them directly._

### 4.7 Port Chromium

- [ ] Port Chromium (~35M lines C++)
- [ ] System web app framework (shared Chromium, not per-app Electron)
- [ ] Port VS Code (via Chromium + Node.js)
- [ ] Port Thunderbird (email)

_Chromium first (required for web app framework + VS Code). Firefox later via Linux compatibility layer._

### 4.8 Development Tools

#### Compilers and Toolchains
- [ ] gcc, cmake, make, pkg-config (via POSIX layer)
- [ ] Rust toolchain (for kernel recompilation)
- [ ] CPython (latest, for ecosystem compatibility and fastpy bootstrapping)
- [ ] fastpy compiler (AOT Python compiler — first-class language for OS userspace)
- [ ] Custom Rust target for the OS
- [ ] Port Rust std library to native syscalls

#### Programming Language Support
- [ ] Rust (native, first-class — kernel language)
- [ ] Python (via fastpy AOT compiler + CPython interpreter)
- [ ] C and C++ (via gcc/clang, POSIX layer)
- [ ] JavaScript and TypeScript (via Node.js / V8 — requires `mem.jit` capability)
- [ ] Java (via OpenJDK — requires `mem.jit` capability)
- [ ] PHP (via php-src port)
- [ ] Lua (via LuaJIT — requires `mem.jit` capability — or PUC-Rio Lua interpreter)
- [ ] Ruby (via CRuby/MRI port)
- [ ] Nim (native compiler, needs C backend which uses gcc)
- [ ] Zig (self-hosted compiler, minimal runtime)

_Goal: a developer should be able to use any mainstream language on this OS. Languages with JIT compilers require the `mem.jit` capability for full performance; they can fall back to interpreter mode without it._

### 4.9 Remote Desktop

- [ ] Port FreeRDP (working remote desktop early)
- [ ] Native compositor-level streaming (draw-command forwarding — most efficient)
- [ ] Video-encoded capture fallback (H.264/VP9 for games/video)
- [ ] DynDNS setup helper in settings (prefer free services, especially dynu.net)

### 4.10 System Snapshots

- [ ] Package snapshots (manifest of active store paths)
- [ ] Mutable data snapshots (CoW at filesystem level)
- [ ] Snapshot tree with branching (like VMs)
- [ ] Select what to include: files/dirs, programs, program data, program settings
- [ ] Rollback any OS update, permanently disable it or retry later
- [ ] Per-program snapshots/rollback (program data and settings)

### 4.11 Service Discovery / RPC

- [ ] Named service registry (D-Bus-like but better)
- [ ] Programs register named services with typed interfaces
- [ ] Service discovery by name, typed RPC calls over channel IPC
- [ ] Serialization format: Cap'n Proto (zero-copy — wire format IS the in-memory format, no serialization step, ~1-5us locally) or FlatBuffers (similar zero-copy approach, from Google). Either avoids the XML/serialization overhead that makes D-Bus slow (~50-200us). Final choice between the two at implementation time.
- [ ] Standard event loop integration API ("give me the waitable handle")

### 4.12 Push Notifications

- [ ] Notification daemon (programs send notifications to it)
- [ ] Standard API for long-lived server connections (WebSocket or similar)
- [ ] Programs register to receive notifications, daemon routes them
- [ ] Capability-gated: "receive push notifications"
- [ ] Store messages for apps not currently running, deliver on next launch

_Push notifications use standard channel IPC + RPC serialization (Cap'n Proto/FlatBuffers). No separate wire format. Daemon maintains server connections, delivers via channels, stores for offline apps._

### 4.13 Program Automation Framework

Programs expose events (observable hooks) and actions (invocable functions) through a standard automation protocol. This enables shell scripting, inter-program workflows, accessibility tools, and macro recording — all through the same channel-based IPC the OS already uses.

#### Core Infrastructure

- [ ] Automation channel: every automatable program registers a well-known channel via the service registry (§4.11)
- [ ] Three meta-commands on the automation channel:
  - `describe` — returns full schema (all events, actions, properties, with types and documentation)
  - `subscribe <event_name>` — start receiving notifications when the named event fires
  - `invoke <action_name> [params...]` — execute a named action with typed parameters, returns result
- [ ] Self-describing schema format: each event/action includes name, human-readable description, typed parameters (name + type + description + optional/required + default), return type, and version (for backwards compatibility)
- [ ] Schema versioning: programs declare automation API version; clients can request specific versions
- [ ] `libautomation` standard library (Rust crate + Python package via fastpy) that handles:
  - Registering the automation channel with the service registry
  - Parsing meta-commands and dispatching to user-registered handlers
  - Schema generation from annotated function signatures (derive macro in Rust, decorator in Python)
  - Event emission (broadcast to all subscribers)
  - Typed parameter validation before dispatch
- [ ] Capability-gated: accessing a program's automation channel requires `automation.connect` capability; subscribing to events requires `automation.subscribe`; invoking actions requires `automation.invoke`. Programs can require additional capabilities for sensitive actions (e.g., `automation.invoke.destructive` for actions that delete data)

#### Automatic Lifecycle Events (all programs get these for free)

Every program using `libautomation` automatically exposes these events without any extra code:

| Event | Fires when | Payload |
|-------|------------|---------|
| `lifecycle.launched` | Program starts and automation channel is ready | `{pid, timestamp}` |
| `lifecycle.focused` | Program window gains focus | `{window_id, timestamp}` |
| `lifecycle.unfocused` | Program window loses focus | `{window_id, timestamp}` |
| `lifecycle.minimized` | Program window is minimized | `{window_id, timestamp}` |
| `lifecycle.restored` | Program window is restored from minimize | `{window_id, timestamp}` |
| `lifecycle.tray_minimized` | Program window is minimized to the system tray | `{window_id, timestamp}` |
| `lifecycle.closing` | Program is about to close (can be subscribed to for cleanup) | `{pid, reason, timestamp}` |
| `lifecycle.idle` | No user input for configurable duration | `{idle_seconds, timestamp}` |
| `lifecycle.active` | User input resumes after idle | `{timestamp}` |

#### Shell Integration

- [ ] `on` keyword in the shell for event-driven scripting:
  ```
  on chat.message_received --from "Alice" { notify "Message from Alice: $event.text" }
  on media.track_changed { log "$event.artist - $event.title" >> ~/music-log.txt }
  on lifecycle.closing --program "text-editor" { invoke text-editor document.save_all }
  ```
- [ ] `invoke` shell builtin for calling actions:
  ```
  invoke chat send_message --to "Bob" --text "Hello from a script"
  invoke media set_volume --level 50
  invoke text-editor document.open --path ~/notes.txt
  ```
- [ ] `automate` CLI tool:
  - `automate list` — list all programs currently exposing automation channels
  - `automate describe <program>` — print full schema for a program
  - `automate describe <program> <event-or-action>` — print details for one item
  - `automate subscribe <program> <event>` — stream events to stdout (for piping)
  - `automate invoke <program> <action> [args...]` — invoke and print result
  - `automate record` — record user actions across programs into a replayable script
  - `automate replay <script>` — replay a recorded automation script

#### Standard Naming Conventions

The tables below are **suggested conventions**, not an exhaustive list. Programs can define any events, actions, and properties they want — these are just the recommended names for common operations in common program categories. The benefit of following them: scripts and tools written against the standard names work across any program that implements them. A script that reacts to `media.track_changed` works with every media player that follows the convention, without per-app special-casing.

Programs are free to add domain-specific events and actions beyond what's listed here. A video editor might expose `timeline.clip_added`, a 3D modeling app might expose `scene.object_selected` — there's no restriction. The conventions simply ensure that the *obvious* operations have *predictable* names.

All automation names use `snake_case`. Events describe what happened (past tense or present continuous). Actions describe what to do (imperative). Properties describe current state (noun or adjective). Names are organized into dot-separated namespaces.

##### Universal (all programs)

| Type | Name | Description |
|------|------|-------------|
| Event | `lifecycle.*` | See automatic lifecycle events above |
| Event | `error.occurred` | `{code, message, severity, context}` |
| Event | `preference.changed` | `{key, old_value, new_value}` |
| Action | `window.focus` | Bring window to front |
| Action | `window.minimize` | Minimize window |
| Action | `window.minimize_to_tray` | Minimize window to the system tray |
| Action | `window.maximize` | Maximize/restore window |
| Action | `window.close` | Close window (may prompt to save) |
| Action | `window.move` | `{x, y}` — move window |
| Action | `window.resize` | `{width, height}` — resize window |
| Property | `window.title` | Current window title (read-only) |
| Property | `window.geometry` | `{x, y, width, height}` (read-only) |
| Property | `window.is_focused` | Boolean (read-only) |

##### Chat / Messaging (`chat.*`)

| Type | Name | Description |
|------|------|-------------|
| Event | `chat.message_received` | `{sender, text, channel, timestamp, attachments[], is_mention}` |
| Event | `chat.message_sent` | `{recipient, text, channel, timestamp}` |
| Event | `chat.typing_started` | `{user, channel}` |
| Event | `chat.typing_stopped` | `{user, channel}` |
| Event | `chat.presence_changed` | `{user, status: online/away/dnd/offline}` |
| Event | `chat.channel_joined` | `{channel, user}` |
| Event | `chat.channel_left` | `{channel, user}` |
| Event | `chat.reaction_added` | `{message_id, user, emoji}` |
| Action | `chat.send_message` | `{to, text, ?channel, ?reply_to}` → `{message_id}` |
| Action | `chat.set_status` | `{status: online/away/dnd, ?message}` |
| Action | `chat.join_channel` | `{channel}` |
| Action | `chat.leave_channel` | `{channel}` |
| Action | `chat.mark_read` | `{channel, ?up_to_message_id}` |
| Property | `chat.unread_count` | Total unread messages (read-only) |
| Property | `chat.current_channel` | Currently viewed channel (read-only) |
| Property | `chat.online_users` | List of online users (read-only) |

##### Media Player (`media.*`)

| Type | Name | Description |
|------|------|-------------|
| Event | `media.track_changed` | `{title, artist, album, duration_ms, track_number, cover_art_path}` |
| Event | `media.playback_started` | `{title, artist, position_ms}` |
| Event | `media.playback_paused` | `{title, artist, position_ms}` |
| Event | `media.playback_stopped` | `{}` |
| Event | `media.volume_changed` | `{level: 0-100, muted}` |
| Event | `media.position_changed` | `{position_ms, duration_ms}` (fires on seek, not continuously) |
| Event | `media.queue_changed` | `{queue_length, action: added/removed/reordered}` |
| Event | `media.repeat_changed` | `{mode: off/one/all}` |
| Event | `media.shuffle_changed` | `{enabled}` |
| Action | `media.play` | `{?uri}` — resume or play specific URI |
| Action | `media.pause` | Pause playback |
| Action | `media.stop` | Stop playback |
| Action | `media.next_track` | Skip to next track |
| Action | `media.prev_track` | Go to previous track |
| Action | `media.seek` | `{position_ms}` |
| Action | `media.set_volume` | `{level: 0-100}` |
| Action | `media.set_mute` | `{muted}` |
| Action | `media.queue_add` | `{uri, ?position}` → `{queue_position}` |
| Action | `media.queue_remove` | `{queue_position}` |
| Action | `media.set_repeat` | `{mode: off/one/all}` |
| Action | `media.set_shuffle` | `{enabled}` |
| Property | `media.current_track` | `{title, artist, album, duration_ms}` or null (read-only) |
| Property | `media.playback_state` | `playing/paused/stopped` (read-only) |
| Property | `media.position_ms` | Current position in ms (read-only) |
| Property | `media.volume` | `{level: 0-100, muted}` (read-only) |
| Property | `media.queue` | List of queued tracks (read-only) |

##### Text Editor / Document Apps (`document.*`)

| Type | Name | Description |
|------|------|-------------|
| Event | `document.opened` | `{path, type, encoding, size_bytes}` |
| Event | `document.closed` | `{path, saved}` |
| Event | `document.saved` | `{path, size_bytes}` |
| Event | `document.modified` | `{path, is_dirty}` (fires when dirty state changes, not on every keystroke) |
| Event | `document.selection_changed` | `{path, start_line, start_col, end_line, end_col, selected_text}` |
| Event | `document.cursor_moved` | `{path, line, col}` (fires on deliberate navigation, not every keystroke) |
| Event | `document.language_changed` | `{path, language}` (for code editors — syntax mode changed) |
| Action | `document.open` | `{path, ?encoding, ?line, ?col}` |
| Action | `document.save` | `{?path}` — save current or save-as |
| Action | `document.save_all` | Save all open documents |
| Action | `document.close` | `{?path}` — close specific or current document |
| Action | `document.insert_text` | `{text, ?line, ?col}` — insert at cursor or position |
| Action | `document.replace_text` | `{start_line, start_col, end_line, end_col, text}` |
| Action | `document.select` | `{start_line, start_col, end_line, end_col}` |
| Action | `document.goto` | `{line, ?col}` |
| Action | `document.find` | `{pattern, ?regex, ?case_sensitive}` → `{matches[]}` |
| Action | `document.replace` | `{find, replace, ?regex, ?case_sensitive, ?all}` → `{count}` |
| Action | `document.undo` | Undo last edit |
| Action | `document.redo` | Redo last undone edit |
| Property | `document.current_path` | Path of active document (read-only) |
| Property | `document.open_documents` | List of open document paths (read-only) |
| Property | `document.is_dirty` | Whether current document has unsaved changes (read-only) |
| Property | `document.cursor_position` | `{line, col}` (read-only) |
| Property | `document.language` | Current syntax language (read-only) |

##### Web Browser (`browser.*`)

| Type | Name | Description |
|------|------|-------------|
| Event | `browser.tab_opened` | `{tab_id, url, ?opener_tab_id}` |
| Event | `browser.tab_closed` | `{tab_id, url}` |
| Event | `browser.tab_switched` | `{tab_id, url, title}` |
| Event | `browser.navigation_started` | `{tab_id, url}` |
| Event | `browser.navigation_completed` | `{tab_id, url, title, status_code}` |
| Event | `browser.download_started` | `{download_id, url, filename, size_bytes}` |
| Event | `browser.download_completed` | `{download_id, path, size_bytes}` |
| Event | `browser.download_failed` | `{download_id, url, error}` |
| Event | `browser.bookmark_added` | `{url, title, folder}` |
| Event | `browser.bookmark_removed` | `{url, folder}` |
| Event | `browser.fullscreen_changed` | `{tab_id, is_fullscreen}` |
| Action | `browser.open_url` | `{url, ?tab_id, ?new_tab, ?background}` → `{tab_id}` |
| Action | `browser.close_tab` | `{tab_id}` |
| Action | `browser.switch_tab` | `{tab_id}` |
| Action | `browser.reload` | `{?tab_id, ?hard}` |
| Action | `browser.go_back` | `{?tab_id}` |
| Action | `browser.go_forward` | `{?tab_id}` |
| Action | `browser.stop_loading` | `{?tab_id}` |
| Action | `browser.find_in_page` | `{text, ?tab_id, ?case_sensitive}` → `{match_count}` |
| Action | `browser.add_bookmark` | `{?url, ?title, ?folder}` |
| Action | `browser.screenshot_tab` | `{?tab_id, ?full_page}` → `{image_path}` |
| Action | `browser.execute_js` | `{script, ?tab_id}` → `{result}` (requires elevated cap) |
| Property | `browser.current_url` | URL of active tab (read-only) |
| Property | `browser.current_title` | Title of active tab (read-only) |
| Property | `browser.tab_count` | Number of open tabs (read-only) |
| Property | `browser.tabs` | List of `{tab_id, url, title, is_active}` (read-only) |
| Property | `browser.is_private` | Whether current window is private/incognito (read-only) |

##### File Manager (`files.*`)

| Type | Name | Description |
|------|------|-------------|
| Event | `files.directory_changed` | `{path, entry_count}` |
| Event | `files.selection_changed` | `{paths[], count}` |
| Event | `files.file_renamed` | `{old_path, new_path}` |
| Event | `files.file_copied` | `{source, destination}` |
| Event | `files.file_moved` | `{source, destination}` |
| Event | `files.file_deleted` | `{path, to_trash}` |
| Event | `files.file_created` | `{path, type: file/directory}` |
| Event | `files.transfer_progress` | `{operation: copy/move, source, destination, bytes_done, bytes_total, files_done, files_total}` |
| Event | `files.transfer_completed` | `{operation, source, destination, file_count, total_bytes}` |
| Event | `files.transfer_failed` | `{operation, source, destination, error}` |
| Action | `files.navigate` | `{path}` — open directory |
| Action | `files.select` | `{paths[]}` — select items |
| Action | `files.select_all` | Select all items in current directory |
| Action | `files.copy` | `{sources[], destination}` |
| Action | `files.move` | `{sources[], destination}` |
| Action | `files.delete` | `{paths[], ?to_trash}` (default: to_trash=true) |
| Action | `files.rename` | `{path, new_name}` |
| Action | `files.create_directory` | `{path}` |
| Action | `files.create_file` | `{path}` |
| Action | `files.open` | `{path}` — open with default application |
| Action | `files.open_with` | `{path, application}` |
| Action | `files.get_properties` | `{path}` → `{size, created, modified, type, permissions}` |
| Action | `files.set_view` | `{mode: icons/list/details/tiles}` |
| Action | `files.sort_by` | `{field: name/size/date/type, ?ascending}` |
| Property | `files.current_directory` | Current directory path (read-only) |
| Property | `files.selected_paths` | List of selected file/directory paths (read-only) |
| Property | `files.view_mode` | Current view mode (read-only) |
| Property | `files.sort_field` | Current sort field and direction (read-only) |

##### Terminal Emulator (`terminal.*`)

| Type | Name | Description |
|------|------|-------------|
| Event | `terminal.output_received` | `{text, session_id}` (fires on complete lines, not every byte) |
| Event | `terminal.command_started` | `{command, session_id, pid}` |
| Event | `terminal.command_finished` | `{command, session_id, pid, exit_code, duration_ms}` |
| Event | `terminal.session_opened` | `{session_id, shell, cwd}` |
| Event | `terminal.session_closed` | `{session_id}` |
| Event | `terminal.directory_changed` | `{session_id, old_cwd, new_cwd}` |
| Event | `terminal.bell` | `{session_id}` |
| Event | `terminal.title_changed` | `{session_id, title}` |
| Action | `terminal.send_input` | `{text, ?session_id}` — send text as if typed |
| Action | `terminal.send_keys` | `{keys, ?session_id}` — send key sequence (e.g., "ctrl+c") |
| Action | `terminal.new_session` | `{?shell, ?cwd}` → `{session_id}` |
| Action | `terminal.close_session` | `{session_id}` |
| Action | `terminal.switch_session` | `{session_id}` |
| Action | `terminal.scroll_to` | `{position: top/bottom/?line_number, ?session_id}` |
| Action | `terminal.get_text` | `{?session_id, ?start_line, ?end_line}` → `{text}` |
| Action | `terminal.set_font_size` | `{size}` |
| Action | `terminal.clear` | `{?session_id}` |
| Property | `terminal.current_session` | Active session ID (read-only) |
| Property | `terminal.sessions` | List of `{session_id, title, shell, cwd}` (read-only) |
| Property | `terminal.cwd` | Current working directory of active session (read-only) |
| Property | `terminal.running_command` | Currently running command or null (read-only) |

##### Email Client (`email.*`)

| Type | Name | Description |
|------|------|-------------|
| Event | `email.received` | `{message_id, from, to[], cc[], subject, preview, timestamp, has_attachments, folder}` |
| Event | `email.sent` | `{message_id, to[], cc[], bcc[], subject, timestamp}` |
| Event | `email.read` | `{message_id, folder}` |
| Event | `email.deleted` | `{message_id, folder, permanent}` |
| Event | `email.moved` | `{message_id, from_folder, to_folder}` |
| Event | `email.flagged` | `{message_id, flag}` |
| Event | `email.sync_completed` | `{account, new_count, folder}` |
| Action | `email.send` | `{to[], ?cc[], ?bcc[], subject, body, ?attachments[], ?html}` → `{message_id}` |
| Action | `email.reply` | `{message_id, body, ?reply_all}` → `{message_id}` |
| Action | `email.forward` | `{message_id, to[], ?body}` → `{message_id}` |
| Action | `email.move` | `{message_id, folder}` |
| Action | `email.delete` | `{message_id, ?permanent}` |
| Action | `email.mark_read` | `{message_ids[]}` |
| Action | `email.mark_unread` | `{message_ids[]}` |
| Action | `email.flag` | `{message_id, flag}` |
| Action | `email.search` | `{query, ?folder, ?from, ?date_range}` → `{results[]}` |
| Action | `email.sync` | `{?account, ?folder}` — trigger manual sync |
| Property | `email.unread_count` | Total unread across all folders (read-only) |
| Property | `email.current_folder` | Currently viewed folder (read-only) |
| Property | `email.accounts` | List of configured accounts (read-only) |

##### Image Viewer / Editor (`image.*`)

| Type | Name | Description |
|------|------|-------------|
| Event | `image.opened` | `{path, width, height, format, size_bytes}` |
| Event | `image.closed` | `{path}` |
| Event | `image.saved` | `{path, format, size_bytes}` |
| Event | `image.zoom_changed` | `{level_percent}` |
| Event | `image.navigated` | `{path, index, total}` (when browsing folder of images) |
| Event | `image.edited` | `{path, operation}` (crop, rotate, filter applied, etc.) |
| Action | `image.open` | `{path}` |
| Action | `image.save` | `{?path, ?format, ?quality}` |
| Action | `image.zoom` | `{level_percent}` or `{action: in/out/fit/actual}` |
| Action | `image.rotate` | `{degrees}` (90, 180, 270) |
| Action | `image.crop` | `{x, y, width, height}` |
| Action | `image.next` | Next image in folder |
| Action | `image.prev` | Previous image in folder |
| Action | `image.slideshow` | `{?interval_ms}` — start/stop slideshow |
| Action | `image.set_wallpaper` | Set current image as desktop wallpaper |
| Property | `image.current_path` | Path of current image (read-only) |
| Property | `image.dimensions` | `{width, height}` (read-only) |
| Property | `image.zoom_level` | Current zoom percentage (read-only) |
| Property | `image.format` | Image format (read-only) |

##### System Monitor / Process Explorer (`system.*`)

| Type | Name | Description |
|------|------|-------------|
| Event | `system.process_started` | `{pid, name, parent_pid, user}` |
| Event | `system.process_exited` | `{pid, name, exit_code, cpu_time_ms}` |
| Event | `system.threshold_crossed` | `{metric: cpu/memory/disk/network, value, threshold, direction: above/below}` |
| Event | `system.disk_space_low` | `{mount_point, free_bytes, total_bytes, percent_used}` |
| Action | `system.kill_process` | `{pid, ?force}` (requires elevated cap) |
| Action | `system.set_priority` | `{pid, priority}` (requires elevated cap) |
| Action | `system.set_threshold` | `{metric, value, ?direction}` — configure threshold alerts |
| Action | `system.refresh` | Force refresh of displayed data |
| Property | `system.cpu_usage` | Overall CPU usage percent (read-only) |
| Property | `system.memory_usage` | `{used_bytes, total_bytes, percent}` (read-only) |
| Property | `system.process_count` | Number of running processes (read-only) |
| Property | `system.uptime_seconds` | System uptime (read-only) |

##### Calendar / Scheduling (`calendar.*`)

| Type | Name | Description |
|------|------|-------------|
| Event | `calendar.event_created` | `{event_id, title, start, end, ?location, ?attendees[]}` |
| Event | `calendar.event_updated` | `{event_id, changes{}}` |
| Event | `calendar.event_deleted` | `{event_id, title}` |
| Event | `calendar.reminder` | `{event_id, title, start, minutes_until}` |
| Event | `calendar.event_starting` | `{event_id, title, start, minutes_until}` |
| Action | `calendar.create_event` | `{title, start, end, ?location, ?description, ?attendees[], ?reminder_minutes}` → `{event_id}` |
| Action | `calendar.update_event` | `{event_id, ?title, ?start, ?end, ?location, ?description}` |
| Action | `calendar.delete_event` | `{event_id}` |
| Action | `calendar.list_events` | `{start, end, ?calendar}` → `{events[]}` |
| Action | `calendar.navigate` | `{date}` — jump to date in the UI |
| Property | `calendar.current_date` | Currently displayed date (read-only) |
| Property | `calendar.view_mode` | `day/week/month/year` (read-only) |
| Property | `calendar.next_event` | Next upcoming event or null (read-only) |

#### Naming Convention Rules

1. **Namespace**: Top-level namespace matches program category (`chat`, `media`, `document`, `browser`, `files`, `terminal`, `email`, `image`, `system`, `calendar`). `lifecycle` and `error` are reserved universal namespaces.
2. **Events** use past tense or present participle: `message_received`, `track_changed`, `typing_started`. Never imperative — events describe what happened, not what to do.
3. **Actions** use imperative: `send_message`, `play`, `open`. Never past tense — actions describe what to do, not what happened.
4. **Properties** use nouns or adjectives: `current_track`, `is_dirty`, `unread_count`. Booleans are prefixed with `is_` or `has_`.
5. **Parameters** use `snake_case`. Optional parameters are prefixed with `?` in documentation. Arrays use `[]` suffix in documentation.
6. **New program categories** should follow the established patterns. If a program spans multiple categories (e.g., a chat app with video calling), use the primary category as namespace and add sub-namespaces (e.g., `chat.call.started`).
7. **Custom events/actions**: Programs may define events/actions outside the standard set. Custom names must not conflict with standard names in the same namespace. Prefix custom names with `x_` if there's any risk of future standard name collision (e.g., `chat.x_custom_reaction`).
8. **Type system**: Standard types are `string`, `int`, `float`, `bool`, `bytes`, `timestamp` (ISO 8601), `path`, `uri`, `duration_ms`. Compound types use `{}` for objects and `[]` for arrays.

#### Developer Adoption Strategy

Automation support is opt-in, but we want it to be the norm rather than the exception. The goal is to make it so easy and so rewarding that developers feel they're missing out if they *don't* support it.

**Make it trivially easy to adopt:**
- [ ] `libautomation` handles all boilerplate — adding automation to an existing program is ~10 lines of code (Rust: derive macro on handler functions; Python: `@automatable` decorator)
- [ ] Starter templates: every project template (GUI app, CLI tool, service) includes automation support out of the box, pre-wired with standard events/actions for that category
- [ ] Automatic lifecycle events come free — just linking `libautomation` gives your program `lifecycle.*` events with zero extra code

**Make it visibly valuable to users:**
- [ ] Programs that expose automation show an "Automatable" badge in the package manager, app launcher, and about dialog
- [ ] Settings app includes an "Automation" section where users can see all automatable programs, browse their schemas, and create simple rules ("when X happens in program A, do Y in program B") without writing any code
- [ ] Shell tab-completion auto-discovers available events and actions from running programs — users see what's possible immediately

**Make it valuable to developers:**
- [ ] Accessibility tools (screen readers, switch access, voice control) use the automation API — supporting automation means your program is automatically more accessible
- [ ] The OS's built-in testing framework can drive programs through their automation API — developers get free integration testing infrastructure
- [ ] `automate record` captures user actions as automation scripts — free macro/replay functionality for any automatable program
- [ ] Package manager search ranks automatable programs higher (all else being equal) — more visibility

**Documentation and ecosystem:**
- [ ] Developer documentation prominently features automation as a first-class OS feature, not an afterthought
- [ ] "How to make your program automatable" guide in the developer docs, with before/after examples showing ~10 lines of code to add full automation support
- [ ] Example programs in the SDK all include automation support, establishing it as "the way things are done"
- [ ] Automation API coverage is part of the quality checklist for programs in the official package repository

_The automation framework uses the same channel IPC + RPC serialization (Cap'n Proto/FlatBuffers) as the rest of the OS. `libautomation` is a thin layer on top of the service registry — no new kernel primitives required. Programs that don't opt in are simply not automatable. The standard naming conventions ensure that scripts written for one chat client work with any chat client, one media player work with any media player, etc._

---

## Phase 5: Settings and Configuration

_This is the comprehensive settings UI. Most items depend on the subsystem they configure._

### 5.1 Display and Appearance

- [ ] Screen resolution (auto-revert if user doesn't confirm in N seconds)
- [ ] Desktop background (static, animated, video, dynamic, random, fitting options)
- [ ] Login screen background
- [ ] Theme browser (see Phase 4.6 for full spec): browse, filter, preview, install, apply themes
- [ ] Theme editor (see Phase 4.6): create/modify themes with live preview, color pickers, contrast checker
- [ ] Per-axis theme mixing: independently select color theme, icon theme, cursor theme, sound scheme, etc.
- [ ] Active theme display: show which theme(s) are currently applied and which axes each covers

### 5.2 Input

- [ ] Mouse pointer style
- [ ] Mouse pointer speed
- [ ] Keyboard repeat speed
- [ ] Keyboard layout customizer: arbitrary remap from any starting layout, save as named layout
- [ ] Include optimized keyboard layouts (Dvorak, Colemak, others)

_Keyboard layouts: Dvorak (+ left-hand, right-hand, programmer variants), Colemak, Workman. Plus QWERTY and locale-specific. Custom remap from any starting layout._

### 5.3 Network

- [ ] Ethernet configuration
- [ ] WiFi: selection, password (show password option)
- [ ] DNS servers (manual or auto)
- [ ] DHCP or static
- [ ] IPv4 configuration
- [ ] IPv6 configuration
- [ ] Firewall settings
- [ ] UPnP/NAT-PMP port forwarding
- [ ] Router detection (button to open router IP in browser)
- [ ] Show LAN IP address
- [ ] Show internet IP addresses (IPv4, IPv6)
- [ ] VPN: OpenVPN settings, show active VPN (including third-party), button to launch VPN app

### 5.4 Security

- [ ] Capability group management
- [ ] Per-user capability management
- [ ] Per-program capability management
- [ ] Per-file/directory capability requirements

### 5.5 Users

- [ ] User management (create, delete, modify)
- [ ] Per-user capabilities

### 5.6 Kernel and System Tuning

- [ ] Memory management tuning parameters (may require reboot)
- [ ] Scheduling tuning parameters (may require reboot)
- [ ] Paging tuning parameters (page size requires recompile)
- [ ] Filesystem tuning parameters
- [ ] Workload type selector (populates all tuning fields)
- [ ] Show advantages/disadvantages of each model/param profile
- [ ] Recompile kernel with specified parameters (detect source changes)
- [ ] Recompile OS component with specified parameters

### 5.7 Storage

- [ ] Swap file size
- [ ] Filesystem deduplication toggle
- [ ] Partition manager (with data loss warning)
- [ ] Mounted drives, network drives: show capacity and free space

### 5.8 Programs

- [ ] Per-program: set priority, set capabilities
- [ ] Uninstall (option to keep program files, keep program settings)
- [ ] Recompile with specified parameters (if source available)
- [ ] Notification settings per app: sound (dropdown, previewable), show in pane, per-notification-type control
- [ ] Program data stored in standard subdirectory (encouraged)
- [ ] Program settings stored in standard subdirectory (encouraged)
- [ ] Wipe program data
- [ ] Per-program snapshot/rollback

### 5.9 Power

- [ ] Action on power button press (shutdown/sleep/hibernate/run program)
- [ ] Action on laptop lid close
- [ ] Turn off screen after N minutes (disableable)
- [ ] Put computer to sleep after N minutes (disableable)
- [ ] Action on battery low (configurable threshold: minutes or percentage)
- [ ] Webcam/mic wake (opt-in only, with privacy warning):
  - [ ] Separate toggles for webcam wake and mic wake
  - [ ] Configurable motion threshold for webcam
  - [ ] Configurable sound level threshold for mic

### 5.10 Snapshots

- [ ] View, create, delete snapshots
- [ ] Snapshot tree with branching
- [ ] Select what to include

### 5.11 Boot

- [ ] GRUB integration for dual-boot
- [x] Optional boot task list display

### 5.12 OS Maintenance

- [ ] OS reset (like Windows)
- [ ] OS repair: restore to initial state, re-import apps (warn/exclude dangerous-settings apps)
- [ ] OS settings re-import by category
- [ ] Rollback options
- [ ] Let's Encrypt SSL certificate creation helper

---

## Phase 6: Advanced Features and Ecosystem

### 6.1 Linux Compatibility Layer

- [ ] Linux syscall translation (FreeBSD Linuxulator approach)
- [ ] epoll, eventfd, signalfd emulation
- [ ] /proc emulation (enough for WINE and common Linux apps)
- [ ] Linux threading model (clone, futex)
- [ ] Linux DRM/KMS compatibility (for NVIDIA proprietary driver userspace)
- [ ] ALSA/PulseAudio compatibility shim
- [ ] Result: WINE runs → Windows app support

_WSL-style Linux distro support: built from compat layer + ext4 + container namespaces. Management layer for distro images. Syscall translation first, lightweight VM fallback if needed._

### 6.2 Additional Filesystems

- [ ] Port Btrfs (CoW, snapshots, checksums)
- [ ] Port F2FS (SSD optimization)
- [ ] NTFS read/write support
- [ ] Queryable file metadata / indexed attributes (BeOS BFS-inspired)

### 6.3 Additional Schedulers

- [ ] EEVDF-style scheduler option
- [ ] Deadline scheduler option (real-time/audio)
- [ ] Selectable in settings, requires reboot

### 6.4 Advanced Security

- [ ] Per-process filesystem namespaces for sandboxing
- [ ] Interceptor hooks implementation (`hook.intercept` — see Phase 1.5 for capability definition)
- [ ] Async notification hooks / tracing subsystem implementation (`hook.*` — see Phase 1.5)
- [ ] Debugging suite implementation (`debug.*` — see Phase 1.5 for capability definition)

### 6.5 Hooks / Tracing Subsystem

_All hooks are gated by their respective capability from Phase 1.5. Programs can only hook filesystem events on paths they have `fs.*` access to. The event logging service (Phase 2.6) subscribes to these hooks to populate the system event log; the Event Viewer app (Phase 4.4) provides the user-facing UI for browsing and filtering logged events. Hook implementation details:_

_Filesystem hooks — `hook.filesystem` (async notification):_
- [ ] Rename file or dir
- [ ] Create file or dir
- [ ] Delete file or dir
- [ ] Change file or dir
- [ ] Read file or dir
- [ ] Read/write errors (corrupt, locked, not found, out of space)
- [ ] Capabilities changed on file or dir
- [ ] Other metadata changed
- [ ] Change journal: "what changed since timestamp X" queries (works across reboots, for backup programs)

_Storage hooks — `hook.storage` (async notification):_
- [ ] Mount or unmount drive
- [ ] Create, resize, delete partition

_Process hooks — `hook.process` (async notification):_
- [ ] Program launched
- [ ] Program exited (normal or crash, with exit code/crash info)
- [ ] Program suspended / resumed
- [ ] Program priority changed

_System hooks — `hook.system` (async notification):_
- [ ] System going to sleep
- [ ] System shutdown
- [ ] OS errors (OOM, I/O error)
- [ ] DPI/scaling factor changed

_Security hooks — `hook.security` (async notification):_
- [ ] User created or deleted
- [ ] User capabilities changed
- [ ] Capability grant/revocation events

_Network hooks — `hook.network` (async notification, scoped per source/destination):_
- [ ] Network activity by program (inbound/outbound, destination, bytes)

_Update hooks — `hook.updates` (async notification):_
- [ ] Program/library loading
- [ ] Library update
- [ ] Snapshot created or rolled back

_Interceptor hooks — `hook.intercept` (synchronous, can BLOCK operations):_
- [ ] All `hook.filesystem` events (can reject before completion)
- [ ] Network connection events (can reject before connection established)
- [ ] 100ms timeout — operation proceeds if interceptor doesn't respond
- [ ] Requires elevated grant, separate from async hooks

_Debugging suite — `debug.*` (developer tools, never granted to normal apps):_
- [ ] `debug.attach` — ptrace-like process attach
- [ ] `debug.memory.read` / `debug.memory.write` — read/write other process memory
- [ ] `debug.breakpoint` — breakpoints, single-step execution
- [ ] `debug.trace.syscalls` — per-process syscall tracing
- [ ] `debug.trace.ipc` — per-process IPC message tracing
- [ ] `debug.trace.locks` — lock acquisition/release/contention tracing
- [ ] `debug.profile` — high-frequency profiling mode (alloc/dealloc, syscall timing, lock contention timing, CPU sampling via hardware perf counters). NOT a general hook — millions of events/sec, specialized infrastructure.

### 6.6 Container Support

- [ ] Namespace primitives (PID, network, mount, user)
- [ ] Resource control groups (CPU, memory, I/O limits per group)
- [ ] Port Docker (or equivalent container runtime)

### 6.7 Additional Software

- [ ] Archive support: zip, 7z, tar.gz, rar
- [ ] ISO file support (navigable, not just extractable)
- [ ] Speech input / speech output (exception to "no AI")
- [ ] Cellphone camera integration (like Windows)
- [ ] Cellphone microphone integration
- [ ] Cellphone-computer integration app
- [ ] Scripting language registration API (ActiveScript-style — see decision below)
- [ ] Network drive support
- [ ] POP3/IMAP email program (or port Thunderbird)

_ActiveScript-style language-agnostic scripting: apps define extension points, OS brokers to registered engines by file extension. Scripts sandboxed by capabilities. Ship with Python (fastpy, default), Lua, WASM. Anyone can add engines._

_Typst as OS-level library (Rust crate, ~30MB). LaTeX as installable package for academics. MathML rendered via Chromium in web content._

### 6.8 Hot-Reload for Updates

- [ ] Userspace services: restart with new version (no reboot)
- [ ] Kernel modules/drivers: unload old, load new
- [ ] Shared libraries: update via generation-based package manager, restart affected services
- [ ] NOT hot-reloadable: core kernel code (scheduler, memory manager, syscall dispatch)
- [ ] Rollback any update, permanently disable or retry later

### 6.9 ABI Stability

- [ ] Phase 1 (internal dev): API changes freely
- [ ] Phase 2 (alpha): API versioned, breaking changes with notice
- [ ] Phase 3 (beta): API v1 declared, breaking changes only in new version
- [ ] Phase 4 (release): versioned syscall tables, old versions maintained
- [ ] Don't change existing functions — make new function if breaking change needed
- [ ] Can remove whole version table when no longer used

### 6.10 Kernel Primitives for Userspace Concurrency

_Don't put green threads in the kernel. Provide efficient primitives instead:_
- [ ] Fast context switching (for userspace thread libraries)
- [ ] io_uring (async I/O without kernel threads)
- [ ] Futexes (synchronization without syscall in uncontended case)
- [ ] These enable Go-style goroutines, Rust async, Python asyncio at userspace level

---

## Phase 7: Installation Wizard

### Pre-Install
- [ ] Keyboard/layout selection
- [ ] Auto-detect monitor DPI, show typical results, let user adjust scaling

### Easy Install
- [ ] Automatic partitioning with sane defaults
- [ ] Swap file sizing (not partition)

### Manual Install
- [ ] Hard drive selection (show device name, size, filesystem, free space, browse files)
- [ ] Partition manager (create, delete, resize — with warnings)
- [ ] Boot partition sizing
- [ ] Swap file sizing
- [ ] Sanity checks on partition sizes
- [ ] Warn about data erasure, confirm

### Configuration
- [ ] Workload type selection (populates tuning presets, show that they're changeable later)
- [ ] Show individual tuning parameters, let user change them
- [ ] Sanity check on user's tuning settings

### Post-Reboot Setup
- [ ] Audio device selection (if multiple)
- [ ] Timezone (try to detect by GPS)
- [ ] Username, password (show password option, allow blank?)
- [ ] Autologin option
- [ ] Browser choice (default for links and HTML files)
- [ ] Theme selection
- [ ] Style selection
- [ ] WiFi selection and password (or ethernet)

### Unattended Install
- [ ] YAML configuration file specifying all options
- [ ] Supports: OS defaults, answering all questions upfront, YAML file, any combination with per-setting fallbacks

### Dual-Boot
- [ ] GRUB integration: add menu entry, don't replace GRUB
- [ ] Can modify GRUB config on Linux partition

---

## Design Decisions Reference

_All 33 original ambiguities from design.txt have been resolved. Decisions are integrated inline throughout the roadmap above. For the full discussion and rationale behind each decision, see the conversation history._
