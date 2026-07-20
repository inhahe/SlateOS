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

When AI finishes a feature that has aspects requiring human manual testing, it must document what needs testing and what specifically to look for in `manual-testing.txt`. Not in commit messages (too easy to miss), and not in `todo.txt`.

`todo.txt` is the **AI's working scratch file** (operator delegated ownership 2026-06-13; see design-decisions.md §14). Its scope is genuine open TODOs and deferred-with-rationale items only. Bugs / divergences / limitations / tech-debt go in `known-issues.md`; resolved judgment calls and design decisions go in `design-decisions.md`; judgment calls awaiting operator input go in `open-questions.md`; completed work is recorded by the commit + the `roadmap.md` checkbox, not duplicated into `todo.txt`.

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

### Linux Compatibility Boundary (non-negotiable)

User-directed constraint (2026-06-06): the Linux ABI surface
(`kernel/src/syscall/linux.rs`) is a **translator**. It accepts Linux
syscall numbers and Linux ABI semantics and dispatches to native
primitives. It does **not** reshape the native OS to look like Linux.
Specifically:

- **No Unix signals as a native process-control primitive.** Native uses
  IPC messages for shutdown / control. Linux signal delivery is
  *emulated* inside the compat layer on top of native primitives —
  Linux signal semantics must not leak into native code.
- **No 4 KiB page assumptions in the native MM.** Native is 16 KiB.
  Linux callers that depend on 4 KiB page size get a translation at the
  ABI boundary (e.g. `sysconf(_SC_PAGE_SIZE)` returns 4096 for
  compatibility); we do not add a 4 KiB allocator alongside the 16 KiB
  one.
- **No ambient-authority fds bleeding into the native handle table.**
  Linux fd integers are looked up in a per-process Linux fd table that
  maps to native unforgeable handles. Capabilities stay capabilities.
- **No fork/exec address-space-sharing assumptions reshaping the native
  process model.** Linux `clone()` is implemented on top of the native
  process / thread primitives; no CoW page-sharing across processes was
  added to satisfy Linux semantics.
- **No Linux-style `/proc`, `/sys`, `/dev` mounts in the native VFS.**
  The compat layer synthesises Linux-shaped pseudo-fs reads from native
  subsystem queries; the native VFS doesn't grow special procfs nodes.

If a Linux binary needs something the native side genuinely cannot
provide without architectural compromise, the right answer is `ENOSYS`
(or the closest Linux errno) with a doc comment explaining why — **not**
a hack into native code to satisfy a Linux quirk. When in doubt, the
native architecture wins.

#### Version-surface policy (user-directed, 2026-06-10): "baseline + honored extras"

The compat layer uses **Linux 6.6 (LTS)** as its *baseline* reference
surface — the version whose syscall/flag semantics we treat as the floor
we faithfully implement. Beyond that floor the rules are:

- **Never accept-without-honoring (the one hard rule).** If we do not
  actually implement the semantics of a flag / opcode / feature, reject
  it with the errno real Linux uses when it lacks that feature
  (`EINVAL` / `ENOSYS` / `EOPNOTSUPP`) — never silently accept it. A
  program that requests a guarantee we can't keep (e.g. `RWF_ATOMIC`)
  must get an honest "not supported" so it falls back, not a false
  success. This is what keeps feature-detection probes correct, and it
  is the real principle behind the version-attribution audit batches.
- **Post-baseline features MAY be kept if fully implemented.** We do
  *not* strip a newer-than-6.6 feature merely for postdating the
  baseline, provided it is correctly and completely implemented and
  obeys the rule above. Retained post-baseline features (audited
  2026-06-10): `fcntl(F_DUPFD_QUERY)` (Linux 6.10) answers truthfully
  and honors its full semantics; the futex2 family — `futex_waitv`
  (5.16), `futex_wait`/`futex_wake` (6.7) — is fully implemented (U32
  size wired; `FUTEX2_NUMA` → `ENOSYS`; `FUTEX2_PRIVATE` accepted with
  the same non-tagging behavior as the classic futex). Everything else
  the sweep batches touched was a *rejected* newer-than-6.6 feature, not
  a retained one. (This supersedes the earlier "strict v6.6, strip
  everything newer" reading that some sweep batches had been following;
  that reading was an inherited interpretation, not this directive.)
- **Keep sibling features consistent — avoid the "Frankenkernel" trap.**
  Real kernels have monotonically-growing feature sets, so some software
  infers "feature A is present ⇒ its siblings from the same era are
  present" and then *uses a sibling without probing it*. Our capability
  surface is a subset, which breaks that assumption. Therefore: when we
  implement a post-baseline feature, also implement (or make a
  deliberate, documented decision about) the closely-related siblings a
  caller is likely to assume ship alongside it — e.g. a paired
  `*_QUERY`/`*_SET`, the other operations of the same syscall family, or
  the companion flags introduced in the same release. If a program is
  likely to infer one function works from another working, the inferred
  one must actually work. If we genuinely cannot provide a sibling,
  record the asymmetry here and in `todo.txt` so it is a known,
  deliberate gap rather than a surprise `ENOSYS` on a path the caller
  believed was guaranteed. Worked example (audited 2026-06-10):
  `futex_requeue` (6.7) is a member of the implemented futex2 family but
  is *not* plumbed through `ipc::futex` yet, so it returns `ENOSYS`.
  This is acceptable because `ENOSYS` is Linux's own canonical "this
  kernel lacks this syscall" signal — glibc/pthread fall back to a
  `FUTEX_WAKE` + rewait loop — so it cannot mislead a caller into a
  Frankenkernel inference. It is recorded here as the one deliberate
  futex2 gap.

Rule of thumb: **6.6 is the floor we guarantee; anything above it is
opt-in but must be truthful and internally consistent** — no lying
functions, and no lone features whose presence would mislead a caller
about absent siblings. (Resolved 2026-06-10, batch 526: `uname` now
reports `sysname = "Linux"` and `release = "6.6.0-slateos"`. These are
Linux-ABI-only surfaces — only Linux binaries call `uname(2)` in our
architecture — so reporting the Linux personality is the faithful
answer, and the leading `6.6` satisfies glibc's startup "kernel too
old" version gate while the `-slateos` suffix still marks our build.)

The truncation-audit work (batches 281+ in `todo.txt`) is purely inside
`linux.rs`: masking high-half register garbage at the ABI boundary to
match what Linux's kernel sees. It does not touch native code. Future
batches must hold to the same discipline.

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

- [x] Choose a project name — RESOLVED: **Slate OS** (display name; identifier form "SlateOS"). Renamed from OuRoS across the codebase 2026-06-13. [original brainstorm: out of ai's suggestions, so far it's Slate, Facet or Rime. My ideas: Neo (going with that so far)]
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
- [x] System-wide commit-policy toggles — **one per ABI** (native and Linux
  have different idioms, so they get independent knobs):
  - [x] Native: `sysctl mm.lazy_default` / `PARAM_MM_LAZY_DEFAULT` (default
    committed on Desktop)
  - [x] Linux: `sysctl mm.linux_lazy_default` / `PARAM_MM_LINUX_LAZY_DEFAULT`
    (default lazy/overcommit), surfaced via `/proc/sys/vm/overcommit_memory`
    (mirrors the live sysctl: lazy→`0`, committed→`2`)
- [x] Linux-ABI mmap defaults to lazy/overcommit (Linux's expected idiom, now
  via `mm.linux_lazy_default`); native programs keep the strict-commit default
- [x] Per-program commit-policy override (`pcb::MmapCommitPolicy`
  {Inherit, ForceCommitted, ForceLazy}, consulted by both `mmap` paths,
  inherited across fork) — the kernel core of design-decisions.md §11
  "Option 5"; user-facing front-ends are Phase 5 (§5.6 system-wide, §5.8
  per-program)

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

#### Guaranteed Resource Headroom for Interactive / Critical Processes
_Goal: regular desktop use (compositor, window manager, input handling, shell,
foreground app) never hangs, stalls, or feels sluggish because a runaway or
greedy workload (a compile, a backup, an indexer, a misbehaving app) has soaked
up all of a resource. Reserve a configurable slice of each contended resource
that background/best-effort work can never fully consume, so critical work always
has capacity available. This is a QoS/reservation model, distinct from mere
priority: high priority still competes for the resource, whereas a reservation
carves out capacity that lower classes cannot touch._
- [ ] **CPU-bandwidth reservation.** Keep a configurable fraction of CPU time
  (per-CPU and/or system-wide) always available to a designated *critical* class
  (compositor, WM, input, audio, the focused foreground app). Implement as a
  cap/floor on the aggregate CPU share that background/best-effort cgroups can
  consume (e.g. throttle non-critical classes so they never exceed `100% −
  reserved%`), integrated with the existing cgroup CPU controller and workload
  profiles. Reservation is enforced even under full load, so the desktop stays
  responsive while a heavy build runs. Tunable per workload profile (Desktop
  reserves more for interactivity; Server/Database may reserve little).
- [ ] **RAM headroom reservation.** Keep a configurable amount of physical RAM
  free/reclaimable for critical processes so allocations and page faults on the
  desktop path don't stall waiting on reclaim/swap. Enforce a *minimum free* /
  *reserved* pool that best-effort workloads' allocations must respect (they get
  throttled or pushed to reclaim/swap before critical work is starved), building
  on the existing `mm.min_free_pages` / reclaim / swappiness machinery and cgroup
  memory limits. Prevents the "everything freezes while the system thrashes swap"
  failure mode. (Committed-memory-by-default already prevents silent overcommit;
  this adds an interactivity floor on top.)
- [ ] **I/O-bandwidth reservation (disk / I/O bus).** Keep a configurable share of
  disk / I/O-bus bandwidth (and queue depth) reserved for critical/foreground I/O
  so a bulk background job (backup, indexer, dedup, `cp` of a huge file) can't
  saturate the device and make the desktop's small reads/writes lag. Implement as
  a floor for the critical I/O class in the existing BFQ-style I/O scheduler
  (background/idle classes throttled to `total − reserved` when critical I/O is
  pending), coordinated with `resource.io_priority` and the idle-priority class.
- [ ] Reservation amounts are tunable (per workload profile + admin-capability
  override) and documented; a sane Desktop-profile default ships out of the box.
  _Note: verify each reservation is physically meaningful for the resource — CPU
  and I/O bandwidth reservation are well-defined; RAM "reservation" is a
  free-pool/min-watermark floor rather than a rate. Where a reservation doesn't
  make sense for a given resource, prefer the min-free-floor formulation over a
  rate cap._

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
- [ ] `fs.recycle.policy` — read/set a filesystem's recycle-bin retention policy (max age/size/count + pruning strategy), scoped per drive/mount. Gates the per-drive policy get/set API in §2.3 → Per-Drive Recycle Bin Access & Management, so an app can't silently reconfigure trash retention on volumes it has no business touching. Browsing/reading a bin needs only `fs.read` on that bin; restoring needs `fs.write`/`fs.create` on the target; purging needs `fs.delete`; only *policy* changes need this capability.
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
- [ ] `audio.exclusive` — take exclusive hold of an output device, bypassing the OS mixer so the app's bitstream goes to the device untouched (Atmos Blu-ray passthrough, exclusive-mode game audio, ASIO music production). Scoped per output device at grant time. Other apps routed to the device are muted-with-toast for the duration; on app exit or crash, the device returns to the mixer automatically. See §3.7 → Audio Settings dialog → Exclusive passthrough.

#### Capability Types — UI
- [ ] `ui.notification` — show notification in notification pane
- [ ] `ui.fullscreen` — show fullscreen window
- [ ] `ui.always_on_top` — show always-on-top window
- [ ] `ui.hide_taskbar` — remove own entry from taskbar
- [ ] `ui.context_menu` — add items to system context menus
- [ ] `ui.pointer_capture` — **exclusively capture (grab) the mouse pointer.** While an app holds the grab, the compositor routes *all* raw pointer input (motion deltas, buttons, wheel) to that app and to no one else, hides the system cursor, and confines the pointer to the app's window/region so it cannot leave — the same behavior as VMware/VirtualBox "capturing" the mouse, a full-screen game using relative mouse-look, or a remote-desktop/kiosk client that swallows the cursor. Distinct from `access.automate` (which *injects* synthetic input into *other* programs) — this capability only grants an app exclusive ownership of the *real* mouse for *itself*. Because a captured pointer can trap the user (the classic "I can't get my mouse back" problem), the grant carries mandatory safeguards the compositor enforces regardless of the app's wishes:
  - [ ] **Unbreakable release hotkey.** A system-reserved chord (default `Ctrl`+`Alt`, VMware-style, user-configurable in settings) always releases the grab and is handled by the compositor *before* the captured app ever sees it — an app holding `ui.pointer_capture` cannot intercept or suppress it.
  - [ ] **Auto-release on focus loss / minimize / window close / app crash.** The grab is tied to the grabbing window's lifetime and focus; losing focus (e.g. via keyboard Alt-Tab equivalent) or the window going away drops the grab immediately, so a buggy or malicious app can never permanently strand the cursor.
  - [ ] **Visible capture indicator.** While captured, the OS shows an unambiguous, non-spoofable indicator (status-bar/tray badge + brief on-grab toast naming the app and the release hotkey) so the user always knows who has the mouse and how to get it back.
  - [ ] **Grab requires foreground + user intent.** Only the focused, user-visible window may acquire the grab, and (by default) only in response to a user gesture (click into the window / enter its region); background or newly-launched windows cannot silently seize the pointer. Compositor-level, single-grabber arbitration: at most one app holds the pointer at a time; a new grab request while another app holds it is denied (not silently stolen).
- [ ] `ui.theme.*` — **change system-wide theme elements that reach outside the app's own window** (the appearance of the whole desktop, not just the app's own surfaces). An app never needs a capability to style *its own* widgets/window — that is ordinary in-process rendering. But mutating a *global* theme axis affects everything: the mouse pointer that shows over every window (the operator's motivating example — an app that changes the cursor image seen everywhere), the system color tokens every app inherits, the window-decoration style the compositor draws for all windows, the icon set, the system sound scheme, etc. These cross the app's boundary and therefore require an explicit, user-granted capability. Split **per theme element (axis)** so consent is meaningful and least-privilege — "this app wants to change your mouse cursor" is a very different risk from "this app wants to change your system colors," and the user grants only what a given app actually needs. The axis set is the *fixed, predefined* list from §3.4 → Theme Format's `supports` axes (unlike open-ended extension strings, so minting one capability name per axis is correct here, not a scope-per-string case — cf. `shell.fileassoc.*`):
  - [ ] `ui.theme.cursor` — set/replace the **global cursor theme** (pointer shape, size, color, animated cursors) seen across the entire desktop, including over other apps' windows and the desktop background. This is the operator's mouse-pointer case: the app changes the pointer image everywhere, not only while the pointer is over its own window (that latter, self-scoped per-window cursor override needs *no* capability). The highest-attention axis because a hostile app could hide or spoof the pointer system-wide, so the grant carries an anti-abuse safeguard: the system-reserved theme-reset chord (settings-configurable) instantly restores the OS default cursor and revokes the override, and the compositor refuses a fully-invisible/zero-size global cursor from an untrusted app.
  - [ ] `ui.theme.colors` — redefine the **global semantic color tokens** (§3.4 Tier 1 Colors) every app inherits, including light/dark variants.
  - [ ] `ui.theme.window_decorations` — change the **global window-decoration style** the compositor draws for all windows (title-bar geometry/buttons, border radius/width, shadow, Aero blur).
  - [ ] `ui.theme.icons` — replace the **global icon theme** (app/file-type/tray/folder icons).
  - [ ] `ui.theme.widget_style` — change **global widget styling** (button/input/scrollbar/toggle appearance) inherited by all toolkit apps.
  - [ ] `ui.theme.fonts` — change **global font preferences** (system/monospace font, sizes, weight).
  - [ ] `ui.theme.sounds` — replace the **global system sound scheme** (notification/error/login sounds, etc.). Attention axis: a spoofed system sound can mislead the user, so the OS still marks which sounds are OS-authentic vs. theme-provided where it matters (e.g. security prompts).
  - [ ] `ui.theme.wallpaper` — set the **desktop background / wallpaper** (static, animated, dynamic, or rotation) — see §3.4 → Desktop Background.
  - [ ] `ui.theme.animation` — tune **global animation parameters** (duration, easing, enable/disable) per §3.4 Tier 3 Animation Tuning.
  - [ ] `ui.theme.terminal` — set the **global terminal color scheme** (16 ANSI + fg/bg) shared by terminal emulators.
  - [ ] `ui.theme.full` — convenience grant covering *all* axes at once (apply/replace a complete theme wholesale, as a theme-manager or settings-sync app would). Prompted as "change your entire desktop appearance"; the user can down-scope to individual axes. Composed of the per-axis capabilities above, never a way to bypass any one of their per-axis safeguards.
  - _All `ui.theme.*` grants mutate declarative theme data only — never load executable code (themes are pure data per §3.4). A change is a full theme-axis swap or token override applied through the same path the Settings app uses; it is revocable (revoking the capability, or the theme-reset chord, restores the prior/default theme for that axis) and audit-logged (JSON-lines: which app changed which axis when), so the user can always see and undo "who changed my cursor/colors."_

#### Capability Types — Shell / File Type Integration

_Predefined capability *names* with grants scoped per extension list — same model as `fs.read` (scoped per path) or `net.connect` (scoped per domain). The user grants e.g. `shell.fileassoc.register` to a program with the extension list `[".docx", ".odt"]`; the program cannot register for any other extension without re-prompting the user. We do **not** mint one capability per extension — extension strings are open-ended and capability names are predefined, so the scope mechanism (already used by `fs.*` and `net.*`) is the right tool._

- [ ] `shell.fileassoc.register` — register the program as a candidate default handler for a file extension (i.e., push onto the per-extension launch stack described in §2.3 → File Type Associations). Scoped per extension list at grant time. Does not grant default-status by itself — the user still chooses which stack entry is active in settings, and the launch stack falls back automatically on uninstall.
- [ ] `shell.thumbnail.register` — register a thumbnail-generator entry for a file extension (push onto the per-extension thumbnail-generator stack described in §2.3). Scoped per extension list at grant time. Independent of `shell.fileassoc.register` — a program can ship a thumbnailer for `.foo` without being a launch handler for `.foo`, and vice versa. (This is the same capability referenced as `thumbnail.register` in §2.3 — name reconciled here.)
- [ ] `shell.protocol.register` — register the program as a handler for a URL scheme (e.g., `mailto:`, `magnet:`, custom app schemes). Scoped per scheme list at grant time. Same stack-and-fallback model as file extensions.
- [ ] `shell.open_with_menu` — appear in the "Open With" submenu of file explorer's context menu for a given extension list, without being on the default-handler stack. Lower-risk grant than `shell.fileassoc.register` because it only makes the program *discoverable* — it never becomes the default by accident. Scoped per extension list at grant time.

_Scoping mechanics: the grant carries a list of extension strings (or `*` if the user explicitly chose "any extension" — rare, intended for power-user tools like a generic hex-thumbnail viewer). Extensions are normalized at grant time (lowercased, leading `.` stripped/added consistently) so `.JPG` and `.jpg` cannot be spoofed as different. Re-prompts on attempted out-of-scope registration: the program calls `register_for(".bar")` while only granted `[".foo"]` → the request fails silently in the kernel but surfaces as a user-facing "<program> wants to also register for `.bar`" prompt the next time the user opens its settings, so the program isn't forced into an immediate modal interruption._

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
- [ ] `admin.memory_policy` — change a **system-wide** memory-commit policy (strict-commit vs lazy/overcommit). Gates **both** per-ABI knobs: the native default (`mm.lazy_default`) and the Linux default (`mm.linux_lazy_default`, surfaced as `/proc/sys/vm/overcommit_memory`). Fine-grained replacement for the slice of Linux's `CAP_SYS_ADMIN` that gates writing `/proc/sys/vm/overcommit_memory`; the Linux compat layer maps that CAP_SYS_ADMIN check to this capability. Needed when the "both commit strategies, configurable" feature lands (see design-decisions.md §11). Note: a user changing their *own program's* per-program overcommit override via Settings is a normal user action and does **not** require this elevated capability — only changing the global default does. We deliberately do **not** add `CAP_SYS_ADMIN` itself as a native capability (it is Linux's ambient-authority junk drawer; SlateOS maps each CAP_SYS_ADMIN-gated operation to its own fine-grained native capability).

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

- [ ] `debug.attach` — attach to another process (ptrace-like debugging interface) — blocked by: ptrace not yet built; deferred behind the same `Process`+`Rights::DEBUG` gate (todo.txt; design-decisions.md §24)
- [-] `debug.memory.read` — read another process's memory — kernel mechanism DONE 2026-06-14: cross-address-space `process_vm_readv` now gated by a `Process` capability carrying `Rights::DEBUG` over the target (`EPERM` without it); see design-decisions.md §24. Remaining: surface as a first-class `debug.memory.read` capability name + grant policy.
- [-] `debug.memory.write` — write another process's memory (higher risk than read) — kernel mechanism DONE 2026-06-14: cross-address-space `process_vm_writev`, same `Process`+`Rights::DEBUG` gate (§24). Remaining: distinct capability name (currently `DEBUG` gates both read and write — see §24 rationale).
- [ ] `debug.breakpoint` — set breakpoints, single-step execution
- [ ] `debug.trace.syscalls` — trace another process's syscall invocations
- [ ] `debug.trace.ipc` — trace another process's IPC messages
- [ ] `debug.trace.locks` — trace lock acquisition/release, contention
- [ ] `debug.profile` — high-frequency profiling mode (specialized, NOT general hooks):
  - [ ] Allocate/deallocate memory events (millions/sec — cannot be a general hook)
  - [ ] Syscall timing
  - [ ] Lock contention timing
  - [ ] Per-function CPU sampling (via hardware perf counters)

#### Crash Dumps & Postmortem Debugging

_When a program hits an **unhandled language-level exception** (our SEH-style model — hardware faults surface as exceptions, not Unix signals), the OS can capture a **crash dump file** for later postmortem analysis. Dump generation is policy-driven and capability-gated; the dump itself is a structured memory image (not a text log, so the "no binary logs" rule does not apply to it)._

- [ ] **Crash dump generation** — on an unhandled exception, the OS writes a dump file before tearing down the process. Captured contents:
  - [ ] Exception record (fault type, faulting address, the exception that went unhandled) and the full register state of every thread
  - [ ] Per-thread stack memory + unwound backtraces (symbolicated if debug info is present)
  - [ ] Loaded library/store-path manifest with versions (so the dump can be re-symbolicated later against the exact binaries — content-addressed store makes this exact)
  - [ ] Selectable memory scope: **minimal** (registers + stacks + a small window around faulting addresses) or **full** (entire committed address space). Default minimal to keep dumps small; full is opt-in.
- [ ] **Who can request a dump — two independent triggers:**
  - [ ] **Program self-opt-in** — a program declares in its manifest (or toggles at runtime via API) that it wants dumps on crash, and chooses minimal vs full. A program can also install its own last-chance handler that writes a custom dump and/or annotates the OS dump before exit.
  - [ ] **User/admin policy** — a system-wide setting ("collect crash dumps: never / minimal / full") with per-program overrides in settings, independent of whether the program asked. Lets a user capture a dump for a third-party app that never opted in.
- [ ] **Capability gating** — capturing a *full* dump of a process you don't own (i.e., another user's or a system process) requires a debug capability (`debug.memory.read`-equivalent); a program dumping *itself* needs nothing. User-policy full dumps are an admin action.
- [ ] **Storage & lifecycle** — dumps land in a per-user crash-dump directory with rotation/size caps (oldest pruned past a configurable quota); a crash-notification surfaces the dump and offers to open it in the dump debugger or report it.
- [ ] **Dump file debugger** — a ported postmortem debugger that loads a dump file, re-symbolicates against the recorded store paths, and presents threads/stacks/registers/memory. Listed under Development Tools (§4.8). The on-disk dump format should be one the ported debugger already understands (e.g. a minidump/ELF-core-compatible container) so existing tooling works with minimal porting.

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
- [ ] **Fast recursive directory traversal — match Linux `find`, not Windows `dir /s`.** Walking large trees (`find /`, `du -sh`, file indexer, backup scan) must be fast in both warm- and cold-cache regimes. NTFS itself is structurally fine; Windows is slow because every open/stat round-trips through the AV/indexer minifilter stack. Our microkernel design eliminates that class of overhead by construction (no filter-driver hook points — drivers can't transparently intercept every fs op the way Windows minifilters do), but the kernel/VFS still has to do the rest:
  - [ ] **Batched dirent syscall** (`getdents`-style — return many entries per call, not one at a time). One-entry-per-syscall APIs (`FindNextFile`) are 10–100× slower than batched ones at scale.
  - [ ] **`d_type` (file-type bit) included in the dirent record.** A walk that only needs "name + is-this-a-directory" must never have to touch the inode. ext4 already stores this in the directory entry; the VFS dirent struct and syscall ABI must expose it. This is the single biggest reason `find -type f` is fast on Linux and slow on Windows.
  - [ ] **Negative-dentry cache.** Repeated lookups of non-existent names (common in `PATH` resolution, library search, `find -name`) must hit cache, not re-walk the directory.
  - [ ] **Per-CPU dcache shards / RCU-style lookup** so concurrent walks on different CPUs don't contend on a global dcache lock.
  - [ ] **Benchmark targets for recursive traversal** (name-only walk, no per-file stat):
    - [ ] Warm cache: ≥ 1M dirents/sec single-threaded on dev hardware (matches Linux `find -type f` on ext4 with hot dcache).
    - [ ] Cold cache on NVMe: ≥ 100K dirents/sec single-threaded.
    - [ ] Stat-every-file walk (`find -printf '%s\n'` equivalent): within 20% of Linux on the same hardware and filesystem.
  - [ ] **Anti-regression test:** measure `find / -type f | wc -l` on a synthetic 1M-file tree at every kernel/VFS change; fail CI on >10% regression.

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
- [ ] **"File is complete" finalizer / write-in-progress signal.** A writer can mark a file as still-being-written and later emit a *finalize* ("done, safe to read") event, so consumers watching the change-notification stream (backup tools, indexers, sync daemons, thumbnailers, antivirus scanners) know not to start on a file that's mid-download / mid-write and won't grab a torn, half-written copy. Design points to work out: (a) an explicit API — a writer opens/creates with a "provisional / in-progress" flag and calls a `finalize()` (or closes with a "committed" disposition) to publish completion, which surfaces as a distinct `Finalized` event on the change-notification stream (distinct from ordinary `Modified`/`Closed`); (b) **automatic finalize on process exit** — if the writing process quits (cleanly *or* crashes), the kernel/VFS auto-fires the finalizer during capability/handle teardown so a crashed download doesn't leave a file permanently stuck in "in-progress" limbo (mirrors the compositor's `release_capabilities()` teardown model §compositor). Distinguish the two exit cases in the event (clean-close vs. process-died) so consumers can choose to treat a crash-finalized partial file cautiously. (c) Consumers that don't opt in see today's behavior (they just see the final `Modified`/size-settled state); the finalizer is an *additional* positive signal, not a gate that blocks reads. Ties into Atomic write transactions above (a committed transaction is implicitly a finalize) and the backup program §4.4 / change journal. Rationale: watching for "size stopped changing for N seconds" is a racy heuristic every backup/sync tool reinvents badly; a first-class completion signal makes "back this up now" correct by construction.
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

##### Per-Drive Recycle Bin Access & Management
Each mounted filesystem owns exactly one recycle bin, living on that filesystem (so a trashed file never crosses a filesystem boundary — trashing is a rename within the same volume, cheap and atomic, and the trashed data travels with the drive when it's unmounted and moved to another machine). The OS must make it easy to see, browse, and manage *each* drive's bin distinctly, and clearly attribute every trashed item to the drive it came from.

- [ ] **Enumerate all recycle bins currently reachable.** A syscall / VFS query that lists every mounted filesystem's bin: for each, the owning mount point, the backing device/volume label, the filesystem UUID, item count, and total bytes held. This is the "which recycle bin is which" surface — the caller (file explorer, settings, a CLI) gets an unambiguous per-drive breakdown, not one merged view. A merged "all trash" view is offered as an *optional aggregation on top*, but the per-drive identity is always preserved and shown (each item is labeled with its source drive).
- [ ] **Stable per-bin identity.** A bin is identified by the filesystem's UUID (not by mount point, which can change between boots). The UI shows a human-friendly name (volume label → mount point → device node, in that fallback order) but the durable handle is the UUID, so "the recycle bin of drive X" resolves correctly even if X is mounted at a different path next time.
- [ ] **Read / browse a specific drive's bin.** List the entries in one named bin (by mount point or UUID): original path, original name, deletion timestamp, size, owning user, and whether it's a file/dir/tree. Support sorting and filtering (by age, size, original directory) within a single drive's bin. Reading a bin requires only read access to that filesystem's trash area — no ambient authority over other drives' bins.
- [ ] **Restore from a specific drive's bin.** Restore an item to its recorded original path on the same filesystem (with collision handling: prompt / auto-rename / skip). Restore is same-volume by construction; cross-volume "restore to elsewhere" is a copy+delete the caller requests explicitly, not the default.
- [ ] **Permanently delete from a specific drive's bin.** Purge selected items, or empty one drive's bin entirely, or empty *all* reachable bins in one operation — but the "empty everything" path enumerates per-drive and reports per-drive success/failure (one unwritable/full/read-only drive must not abort purging the others; track and report the worst error per §Code-Quality batch-op rule).
- [ ] **Per-drive auto-delete (retention) policy.** Each bin has its *own* policy, stored on that filesystem (so the policy travels with the drive): max age (delete items older than N days), max size (cap the bin at N bytes / N% of the volume), max item count, and the pruning strategy knobs above (oldest-first / smart / lazy). A system-wide default policy applies to any drive that hasn't set its own; a per-drive override wins. Removable drives can carry a stricter or looser policy than the system disk.
- [ ] **Read/set policy through a clear per-drive API.** Get/set the retention policy for a named bin (by mount point or UUID). Setting a policy is capability-gated (a `fs.recycle.policy` capability, scoped per drive) so an app can't silently reconfigure retention on volumes it has no business touching. Settings UI exposes each drive's bin as its own row with its own policy controls, plus the editable system default.
- [ ] **Run retention policies opportunistically when a drive is mounted.** Auto-delete is *not* a global timer that assumes every bin is always online — removable and network drives come and go. Instead: on mount, evaluate that filesystem's own stored policy and prune it (respecting the lazy/gradual option so a freshly-mounted USB stick doesn't stall the mount on a big purge); then re-evaluate periodically while it stays mounted, and on unmount cleanly (best-effort, non-blocking). A drive that was offline for a month gets its age-based pruning applied the moment it reappears, using timestamps recorded at deletion time (wall-clock stored per item), not "time since mount."
- [ ] **Mount/unmount hooks own the policy pass.** The VFS mount path fires the retention evaluation (capability-gated internal call), and the unmount path gets a last-chance flush; both are bounded and preemptible so they never wedge a mount/unmount. Policy runs are logged (JSON-lines) per drive: what was pruned, how much space reclaimed, which policy rule triggered it.
- [ ] **Space-pressure pruning is still per-drive.** When a *specific* filesystem is low on space, its *own* bin is pruned first (the trash on drive X frees space on drive X, never on Y). The global "system needs space" trigger fans out to the affected volume's bin, not all bins indiscriminately.

#### File Type Associations
- [ ] Extension → default app mapping
- [ ] Per-app icons per extension (e.g., audio vs video files can have different icons even if same app)
- [ ] **Per-extension "brief description" is set by the owning application.** Manifest field on `shell.fileassoc.register` — alongside the extension list and icon, each manifest entry carries a short human-readable label (e.g. "MP3 audio", "Markdown document") that becomes the file's "Brief description" column value in file explorer (§4.1) and anywhere else the OS surfaces a file-type label. The description follows the active top-of-stack handler: if the user re-prioritizes the launch stack or the top handler is uninstalled, the description shifts to whatever the new top handler provided in *its* manifest. Same stack-and-fallback semantics as the launch target. The user can override the description per extension in settings; the override persists across handler changes (it's stored on the user side, not on the handler side). When no registered handler supplies a description and no user override exists, the description falls back to the owning-application name, then to the uppercase extension (e.g. "FOO file").
- [ ] User can change association: pick from registered apps, any installed app, or any executable + arguments
- [ ] **Fallback chain (stack) on handler uninstall.** Per-extension default-app associations are stored as a *history stack*, not a single value. Installing a new handler pushes onto the stack (gated by `shell.fileassoc.register`, scoped per extension list — see §1.5 → Capability Types — Shell / File Type Integration); uninstalling pops to whatever was active before. Multiple uninstalls in a row continue to fall back further down the chain until either an installed handler is found or the stack is empty (in which case the extension reverts to "no default — ask user."). Same stack model for the "user manually changed association" path: the manual choice is just another push, and uninstalling it pops to the previous entry. User can view and edit the full chain in settings if they want — e.g., reorder it, or remove a specific historical entry without uninstalling the program.
- [ ] **Thumbnail-generator registration (independent of file-type ownership, capability-gated).** Any program can register a thumbnail generator for any extension(s), given the appropriate capability — they do *not* need to be the default handler for that extension, or even own the file type at all. This lets a standalone thumbnail generator for `.docx` files coexist with whatever document viewer the user prefers; a third-party generator for a proprietary format ship without bundling a full viewer; a power user install niche generators for filetypes the OS doesn't otherwise have viewers for. Generators are declared at install time (manifest entry: extension list, generator executable path, supported output sizes, max-memory hint, timeout hint). The OS maintains a central extension → active-generator map used by file explorer and any other thumbnail-consuming app.
  - [ ] **Capability:** registering a thumbnail generator requires `shell.thumbnail.register` (defined in §1.5 → Capability Types — Shell / File Type Integration). Without it, the manifest entry is silently ignored. The capability *name* is predefined; the grant is *scoped per extension list*, so the user picks at install time which extensions the program may thumbnail. A program that lists `.foo` and `.bar` in its manifest can ask for both in one grant, but cannot later add `.baz` without re-prompting the user. Becoming the default handler for a file extension (launch, not thumbnail) is a separate capability — `shell.fileassoc.register` — so a program can ship a thumbnailer for a format it doesn't own.
  - [ ] **Stacking + auto-fallback on uninstall.** Each extension has a *stack* of registered generators, ordered by registration time (newest on top). The top-of-stack installed generator is the active one. When the active generator is uninstalled, the next one down becomes active automatically — no broken thumbnails, no user intervention. If the user manually picks a non-top generator as preferred (settings UI), that promotion is also stored on the stack so uninstalling the preferred one falls back to the previous preferred, then the previous-previous, etc. Empty stack → built-in placeholder thumbnail.
  - [ ] **User control.** Settings UI lets the user see, for each extension, the full list of registered generators and which is active. User can: pick a different one as active, disable specific generators without uninstalling them, or reorder the stack. This matches the same UX pattern as the default-app association (above) so users only learn one mental model.
  - [ ] Each thumbnail generator runs in its own sandboxed process — never in-process with the file explorer or any other consumer. A crash in a generator (native or third-party) must not crash the calling app; the OS surfaces a generic placeholder thumbnail for files whose generator crashed or timed out.
  - [ ] Generator processes have minimal capabilities: read-only access to the specific file being thumbnailed, write access only to a return pipe. No network, no other filesystem, no IPC outside the request/response pair. (The sandbox is what makes the "any program can register" rule safe — a third-party generator can't exfiltrate file contents or harm the system even if malicious, because the sandbox doesn't let it.)
  - [ ] Per-generator timeout (default ~2 s, manifest-tunable) and per-generator memory cap (default ~256 MiB, manifest-tunable) enforced by the kernel; exceeding either kills the worker and marks the file as "no thumbnail."
  - [ ] Crash/timeout backoff: if a generator crashes N times in a row on different files, it's marked degraded and skipped for the rest of the session (placeholder used instead). When a generator is degraded, the OS automatically tries the next generator down the stack for affected files — same fallback path as an uninstall. User can re-enable from settings.
  - [ ] Generator workers are pooled per-extension and reused across files within a session to amortize startup cost. Pool size cap per generator (default 4) prevents a directory of thousands of files from spawning thousands of workers.
  - [ ] Built-in generators (images, video, PDF) use the same registration mechanism, the same stack, and the same sandbox — no privileged in-process path, and a user-installed third-party generator can override a built-in one by sitting on top of its stack entry. This forces the framework to be robust against its own first-party generators and lets users genuinely replace the defaults.

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
- [ ] **Windows-shell familiarity layer: a `cmd.exe` emulator (and, stretch, a PowerShell emulator).** For users migrating from Windows, provide a shell that accepts classic `cmd.exe` syntax — the builtin commands (`dir`, `copy`, `move`, `del`, `ren`, `type`, `cd`/`chdir`, `md`/`mkdir`, `rd`/`rmdir`, `cls`, `echo`, `set`, `path`, `where`, `for`, `if`, `goto`, `call`, `start`, `title`, `%VAR%`/`%ERRORLEVEL%` expansion, `&`/`&&`/`||`/`|` operators, `.bat`/`.cmd` batch-file execution) — mapping them onto native filesystem/process/env syscalls so muscle-memory and existing `.bat` scripts work. It is an *emulation/compat layer*, not the default shell (Nushell stays default); it lives alongside Oils the same way. **Stretch goal: a PowerShell emulator** — much larger scope (a real object pipeline, cmdlets, .NET-esque type system). Two realistic paths, to be decided when tackled: (a) port PowerShell Core (open-source, MIT) via the .NET/CoreCLR runtime once that's available on the OS — the faithful option; or (b) a *subset* emulator covering the most common cmdlets (`Get-ChildItem`/`gci`, `Get-Content`, `Set-Location`, `Copy-Item`, `Where-Object`, `ForEach-Object`, `Select-Object`, `$_`, object pipeline basics) mapped onto Nushell's already-structured pipeline where semantics align. Record as an open question which path to take before starting PowerShell specifically; the `cmd.exe` emulator is the committed near-term deliverable and does not depend on it.

_Nushell as default interactive shell (structured data, Rust-native). Oils for POSIX/bash compatibility (replaces bash). A `cmd.exe` emulator (and stretch PowerShell emulator) ships as a Windows-familiarity compat layer, not as a default shell._

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
- [ ] **"Who's holding it?" — mandatory attribution on contention failures.** When an operation fails because some other process holds a resource (file lock, exclusive open, port binding, capability slot, shared-memory region, device handle), the error must volunteer the identity of the holder, not just say "resource busy." Required fields in the error: **installed application name** (the human-meaningful name from the package manager, e.g. "Firefox", not just `firefox-bin`), the **full executable path**, the **PID**, and — when applicable — the **window title** or **service name** so a user can recognize "oh, that's the browser tab I forgot about." Apply this rule to every OS API that can return a contention error: file open with conflicting share mode, `rename`/`unlink` blocked by an open handle, `bind` blocked by a held port, IPC channel held exclusively, recursive lock held by a different thread, etc. The kernel tracks ownership of every contentious resource already — the policy is "never let that information die at the API boundary."
  - [ ] When the holder is itself the OS, name the *originating* program through the service-attribution chain (see §4.3 Process Explorer), not the service process. "Locked by Photos.app (via filesystem service)" — not "locked by `fs-svcd`."
  - [ ] When attribution isn't available (e.g., the holder crashed leaving a stale lock), say so explicitly: "Resource held by orphan lock from PID 4123 (process no longer running). Run [recover-stale-locks] to clear." Never leave the user staring at "resource busy" with no recourse.
  - [ ] Bubble this attribution all the way up through the GUI: file explorer's "couldn't delete" dialog shows the holder identity inline, with a button to switch to that program or kill it. Same for the editor's "couldn't save" dialog, package manager's "couldn't upgrade — file in use" dialog, etc.

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
- [ ] Surface owner liveness as a precondition for compositing. Before drawing a surface each frame, the compositor confirms the owning process is alive and its IPC channel is healthy; dead/unresponsive owners' surfaces are evicted, not drawn. **This is event-driven, not a poll.** The mechanism: the compositor holds a capability handle to each surface. When an owner process dies (or its IPC channel breaks), the kernel's `release_capabilities()` runs synchronously during process teardown and revokes the handle — this is a guaranteed-prompt event, not something the compositor has to discover. The per-frame "check" is then a single relaxed atomic load on the surface's `handle_valid` flag (or equivalent — a check that the surface's slab entry hasn't been zombified). Cost: <1ns per surface, well below the cost of even setting up the surface's draw call. At 144Hz × 50 surfaces ≈ 7.2k atomic loads/sec — negligible. **Explicitly NOT this:** pinging the owner over IPC every frame and waiting for a reply. That would be microsecond-scale per surface per frame and would burn a CPU core on health checks. We do not poll liveness — we react to revocation, and the per-frame load just confirms no revocation arrived since the last frame. Rationale: the most common cause of stray artifacts that survive every normal redraw mechanism is an *orphan surface* — the owner died (or its connection broke) while a tooltip / popup / hover label was up, and the compositor kept drawing the last submitted frame because nothing told it to drop the surface. Capability revocation + per-frame atomic check catches this within one frame, automatically, with no user action and effectively no overhead. The user-triggered full-redraw hotkey (below) is the *fallback* for when this mechanism itself has a bug — not the primary defense, because a stray widget persisting until the user finds the hotkey is the exact UX failure that erodes trust in a desktop.
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
- [ ] **Volume icon popup (click on system-tray volume icon).** Lightweight Aero-styled flyout — opens instantly without launching the full Settings app. Contents:
  - [ ] **Main volume slider** at top with current level, and a mute toggle next to it. Adjusting the slider takes effect immediately (no Apply button).
  - [ ] **Output device selector** — dropdown or radio list of available audio output devices (speakers, headphones, HDMI sinks, Bluetooth devices, USB DACs, virtual outputs). Selecting one switches the system default output. Currently active device shown highlighted; unavailable devices (unplugged, asleep) shown disabled with reason. Refreshes live as devices appear/disappear.
  - [ ] **Per-app volume mixer** — one row per application that has output sound in the current windowing session, each row showing app name, app icon, a volume slider, and a mute toggle. Adjusting an app's slider changes only that app's volume; muting only that app does not affect others.
    - [ ] **Cap at 10 rows.** If more than 10 apps have output sound this session, show the 10 most recently active (most-recent-first ordering). A "show all (N)" link at the bottom expands the list to the full set or jumps to the full audio settings dialog (below).
    - [ ] App entries persist for the full windowing session even after the app stops outputting, so the user can pre-mute an app that's about to play (e.g., mute the browser tab before the video starts). Entries are dropped when the app exits.
  - [ ] **"Audio settings…" button** at the bottom of the popup that opens the full audio settings dialog (§3.7 → Audio Settings dialog). Single dialog covers both output and input — no separate "output settings" and "input settings" entry points; one place to go.
- [ ] Sound history: view which programs recently played sounds, button to go to that app's sound capabilities
  - [ ] **Attribute to the *real* originating app, not the service that relayed it.** When audio is played through an OS service or intermediary process (a shared audio daemon, a notification service, a media-key handler, a "play sound" helper, etc.), the history must show the actual application that requested the sound — not the service program. This requires the audio path to carry the originating app's identity (process metadata / a submitted "on-behalf-of" attribution token) through the service so the mixer/history can resolve it. Fall back to the relaying service only when no originating identity is available.
  - [ ] **Sub-app / tab granularity where the app supplies it.** For multiplexed apps that host many independent sound sources — most importantly **Chromium**, which can play audio from many tabs — the history should identify *which tab* (or sub-context) played the sound, not just "Chromium". Surface this via app-supplied metadata embedded in / attached to the audio stream (e.g. a per-stream name such as the tab title/URL, or a sub-stream identifier the app registers). Same mechanism generalizes to any app that names its individual output streams (media players with multiple sources, multi-document apps, etc.).

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

##### Default Theme — Aero
- [ ] Ship an Aero-inspired theme as the out-of-the-box default. Reference: `Aero Desktop (offline).html` in the project root — match the look of:
  - [ ] **Window frames** — glassy/blurry title bar, rounded top corners, soft drop shadow, gradient highlight on focused window, dimmed/desaturated frame for unfocused windows, Aero-style close/minimize/maximize buttons in the top-right
  - [ ] **Taskbar** — translucent/blurry panel, grouped running-app icons with hover thumbnails, Aero Peek-style preview on hover, distinct visual treatment for pinned vs. running apps
  - [ ] **Start menu** — two-column layout (pinned/recent on the left, system folders/power on the right), translucent background matching taskbar, search field at the bottom, jump-lists from pinned apps
  - [ ] **File explorer** — Aero-styled chrome (translucent title bar, Aero address bar, pane splits with the same glass treatment), default view styling that matches the rest of the shell
  - [ ] **Search dialog** — Aero-styled modal: glassy chrome, accent-color focus ring, result rows with the same row styling as file explorer
  - [ ] **File/folder select (open/save) dialog** — same chrome and styling as file explorer (it IS the file explorer component per §4.1), Aero-styled OK/Cancel buttons in the footer
  - [ ] **Indexing Options dialog** — Aero-styled settings modal for the file-indexer (which paths are indexed, which content types, ML/OCR opt-ins, status of current indexing pass): glassy chrome, two-column layout (indexed locations list on the left, configuration controls on the right) matching the Windows Indexing Options dialog visible in the reference file, Aero-styled action buttons in the footer
- [ ] The default theme is a normal YAML theme file — it uses the same axis system as third-party themes, so users can swap it out wholesale or override any single axis (e.g., keep Aero window decorations but switch icons to a flat-modern pack). No hard-coded "Aero mode" path in the compositor.
- [ ] Aero blur and transparency are theme axes (window-decorations, taskbar-panel-styling), so users who want a flat/opaque look can disable them without losing the rest of the default visual identity.

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
- [ ] **Title-text overflow styling (CSS-like `text-overflow`).** When the window
  title is too long to fit the available title-bar width, the theme/style controls
  how it is truncated. Support at least: (a) **clip** (hard cut), (b) **ellipsis at
  the end** (`"Long docume…"`), and (c) a **"keep the tail" / ellipsis-at-start**
  mode that shows a leading ellipsis followed by the *rightmost* N characters that
  fit (`"…ort/final.txt"`) — useful for paths and filenames where the end is the
  distinguishing part. Truncation is measured in fitted glyphs, not a fixed char
  count, so it adapts to the actual pixel width. Same property vocabulary is shared
  with taskbar tab labels (Tier 2 — Taskbar/Panel Styling) so both truncate
  consistently.
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
- [ ] **Taskbar tab-label overflow styling (CSS-like `text-overflow`).** When a
  taskbar entry shows an app/window name (optional label mode, §Taskbar) and the
  label is wider than the tab, the theme controls truncation using the *same*
  property set as window titles: **clip**, **end-ellipsis**, or **start-ellipsis /
  keep-the-tail** (leading `…` + the rightmost N glyphs that fit). Truncation is
  fitted to actual pixel width, not a fixed char count, and updates live as tabs
  grow/shrink (more windows open, taskbar resized). Shared vocabulary with Tier 1
  window-title overflow so tabs and titles behave identically.

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
- [ ] **Hotkey → emit an arbitrary emoji / Unicode character or string.** In addition to functions and shell commands, any hotkey can be bound to *type or paste a chosen piece of text* — a single emoji, any Unicode character, or a multi-character string/snippet. When bound, pressing the hotkey inserts that text into the focused input (via the same synthetic-input/clipboard-paste path the OS uses for other text injection). The binding UI for this action offers two ways to pick the character/string: (a) type/paste it directly into a text field, or (b) open the **Unicode selection dialog** — a picker with category tabs (Smileys & Emotion, People, Animals, Symbols, Math, Arrows, Currency, Latin/Greek/Cyrillic, etc.) and a live **search** box (match by character name, keyword, or codepoint, e.g. "shrug", "U+00E9", "arrow right"). The picker is a reusable OS component (the same one surfaced by the tray emoji-input entry in §desktop and available to apps), so users learn one dialog. The stored binding keeps the literal string, so it survives font/emoji-set changes and is not tied to a particular codepoint lookup at press time.

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
- [ ] **Font picker dialog** (family, style/weight, size, and other font attributes).
  - [ ] **Live "tentative selection" events.** The picker fires an event *whenever
    the user tentatively/temporarily changes any font attribute* (hovers or
    highlights a family, changes the size, toggles bold/italic, etc.) — before the
    dialog is committed with OK. This lets the host app show a **live preview** of
    the setting as the user browses (e.g. re-render the document/target text in the
    tentatively-selected font in real time), then either keep the final choice on
    accept or revert to the original on cancel. Distinct signals for *tentative
    change* (may fire many times, freely revertible) vs. *committed* (OK) vs.
    *cancelled* (revert), so apps can preview without persisting. The same
    tentative-vs-committed event pattern generalizes to the color picker and other
    attribute-choosing dialogs where live preview is useful.
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

#### Ribbon Widget
_A tabbed command surface (Office-style) for command-dense applications: file explorer, text editor, image editor, etc. The ribbon is a widget, not a mandatory chrome — apps that don't want one use traditional menus and toolbars instead._

- [ ] Tabbed top bar: each tab is a category of commands (e.g., Home, View, Tools)
- [ ] Each tab divided into named groups; groups contain buttons, split-buttons, dropdowns, galleries, toggles
- [ ] Three button sizes within a group (large with icon-above-label, medium with icon+label side-by-side, small icon-only)
- [ ] Contextual tabs that appear only when relevant content is selected (e.g., a "Picture Tools" tab when an image is selected — appears with a distinct color band)
- [ ] Keyboard access via key tips (overlay letters/numbers on every command, navigable like Office Alt-sequences)
- [ ] Minimize/expand ribbon (double-click a tab or hotkey toggles full-height vs. tabs-only)
- [ ] Adaptive group collapsing when window is too narrow: groups collapse to a single dropdown button showing their label and icon, expanding to the full group on click — the collapse order is per-group priority defined by the app
- [ ] Quick Access Toolbar (small always-visible row above or below the ribbon for user-pinned commands)
- [ ] Customization UI: user can reorder tabs, add/remove commands from groups, hide tabs they don't use
- [ ] Theme-aware rendering: ribbon chrome uses the same window-decoration tokens as the title bar so it blends with the active theme (Aero glass by default)

_Patent caution: Microsoft holds patents covering specific aspects of ribbon layout — particularly the "Office Fluent UI" licensing program covers the precise tab/group/contextual-tab arrangement, the gallery-on-hover preview behavior, and the specific collapse heuristics. **Implement the general tabbed-command pattern, which is not patentable, but stop short of the specific arrangements, animations, and behaviors covered by Microsoft's claims.** Concretely: don't replicate Office's exact contextual-tab color rules, don't copy the specific gallery live-preview UX verbatim, and don't reproduce Office's exact key-tip overlay sequence.  This is intentional under-implementation — better to ship a deliberately-different ribbon than risk an infringement claim._

_Author's note: I once found a Microsoft document specifying exactly how the Office ribbon rearranges groups when commands are added/removed, including the precise priority encoding and collapse-order rules. **TODO for Claude:** search online for this spec (likely titled something like "Office Fluent UI Design Guidelines" or "Office UI Command Design Specification") and use it to implement the adaptive-collapse algorithm — but only to the degree that doing so doesn't reproduce patented behavior. If the spec turns out to describe patented mechanisms verbatim, treat it as inspiration only and implement a deliberately-distinct algorithm._

_Patent timeline: most of Microsoft's ribbon-specific patents (filed around 2005–2007) are expiring within the next year or so. **TODO:** revisit this entry after the relevant patents have expired and lift the deliberate under-implementation — at that point we can implement the full Office-faithful behavior without legal risk._

#### Advanced Features
- [ ] Clipboard: multi-format (text, HTML, image, structured data)
- [ ] **Watch for Chromium's broken RTF clipboard write (port bug to fix).** On the host, Chrome/Chromium writing **RTF** to the clipboard is observably broken — random spans get dropped, so a paste into an RTF-consuming target receives most of the content missing. Since we're porting Chromium (§4.7), its RTF clipboard code path is a strong candidate to carry the same bug into our OS. Action items: (a) when the Chromium port reaches clipboard integration, test copy→paste of rich text specifically in the **RTF** flavor (not just HTML/plain) against a target that requests RTF, and confirm round-trip fidelity; (b) if the bug reproduces, fix it in our Chromium fork (root-cause the RTF serializer, don't just paper over it) and, ideally, upstream the fix; (c) as an OS-level safety net, since our multi-format clipboard auto-generates a sanitized rich-text flavor from the source's full-rich/HTML anyway (see the multi-format note below), prefer/repair RTF from HTML when an app's RTF flavor looks truncated. Log under known-issues once the port exists so it isn't forgotten.
- [ ] Clipboard history with view and select
- [ ] Paste as plain text option
- [ ] Drag-and-drop (OLE-style multi-format)
- [ ] File picker / save dialog (reuses file explorer component)
- [ ] DPI/scaling awareness
- [ ] **Forgiving drag margins.** Every draggable divider/edge (window resize borders, dockable-panel splitters, list/table column dividers, and any other "grab the boundary and drag" affordance) must be easy to hit — either the visible grab strip is drawn thick enough to target comfortably, or the drag hit-region extends a few pixels past the visible margin on both sides (a hysteresis/"snap-to-edge" hit-target that starts the drag mode slightly before the cursor reaches the exact pixel line). Users should never have to pixel-hunt to start a resize/reposition drag.
- [ ] **Generous hit-regions for draggable control handles (sliders too).** The same "clickable region wider than the visible norm" principle applies to every draggable *handle*, not just dividers — most importantly **slider thumbs and their tracks**: the grab region for a slider handle (and the click-to-jump region along its track) extends past the drawn thumb/track so the user can grab and drag it without pixel-hunting, and a thin visual track still presents a comfortably tall/wide hit-strip. Generalize to any small draggable affordance (scrollbar thumbs, resize grips, dockable-panel drag handles, color-picker sliders §colorpicker, volume sliders §audio): the *interactive* region is decoupled from and larger than the *drawn* region. Implemented once in the toolkit's hit-testing layer so every widget inherits it uniformly; scale the extra padding with DPI/scaling so it stays a consistent physical target.
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
- [ ] **Audio Settings dialog (full).** Single dialog reachable from the volume-icon popup's "Audio settings…" button and from the Settings app. Combines output and input — no separate dialogs — so users learn one place. Aero-styled per §3.4 default theme. Sections:
  - [ ] **Output**
    - [ ] **Output device selector** — same device list as the volume-icon popup, but with full per-device controls (test tone, properties, set as default, set as default for communications).
    - [ ] **Master output volume** with mute.
    - [ ] **Panning** — left/right balance slider for the selected output device. Center detent. (For multi-channel outputs, the panning control becomes a per-channel level matrix instead of a single left/right slider — single slider is the 2-channel default presentation.)
    - [ ] **Output channel layout / spatial mode** — radio group covering the *layout* axis (how the audio is spatialized), separate from the bitstream encoding (next control). Options:
      - [ ] **Mono** — combines all channels into a single output. Applied at the mixer, so it works on any device regardless of the device's own channel count. Always available.
      - [ ] **Stereo** — standard 2-channel. Always available.
      - [ ] **Multichannel (5.1 / 7.1 PCM)** — discrete-channel surround over uncompressed PCM. Available when the active output device advertises ≥6 channels (HDMI sinks, surround USB DACs, multichannel SPDIF). Sub-selector for the exact layout (5.1, 7.1, 5.1.2, 5.1.4, 7.1.4).
      - [ ] **Spatial / object-based** — object-based immersive audio with height channels. Available when the device advertises support for at least one supported bitstream format (see next control). When selected, the bitstream-format sub-selector is enabled.
      - [ ] Selection is per output device, not global — headphones can be stereo while HDMI is Spatial. Unavailable options are disabled with a hover tooltip explaining why ("HDMI sink advertises only 2 channels", "no installed spatial-audio renderer supports this device", etc.).
    - [ ] **Spatial bitstream format** — **single-select dropdown** (radio semantics), only enabled when the layout above is set to **Spatial / object-based**. This control picks the *wire format* the mixer encodes its output as when sending to this device — only one bitstream goes over the cable at any moment in time, so checkboxes would be misleading (the user would tick three and only one would ever actually be active). Populated dynamically from **the intersection of (formats the device advertises) ∩ (formats the OS has installed encoders for)**. A device typically advertises many formats at once (an AVR's CEA-861 audio data block usually lists Dolby Digital, DD+, TrueHD, Atmos, DTS, DTS-HD MA, DTS:X all together; Bluetooth A2DP devices commonly list SBC, AAC, aptX, LDAC, LE Audio LC3; USB audio descriptors list multiple alternate settings) — the dropdown surfaces all of them and lets the user pick the active one. Likely contents in 2026:
      - [ ] **IAMF (Immersive Audio Model and Formats)** — *recommended.* Royalty-free open standard from the Alliance for Open Media (Google / Samsung / Netflix / Amazon / Meta — same group behind AV1). Supports channel-based, scene-based (Ambisonics), and object-based audio in one container. Default when the device supports it. Same role in audio as AV1 has in video for this OS.
      - [ ] **Dolby Atmos** — dominant proprietary object-based format (Netflix, Disney+, Apple Music, Tidal, UHD Blu-ray). Use when the device advertises Atmos but not IAMF.
      - [ ] **DTS:X** — DTS's object-based equivalent. Use when the device advertises DTS:X but not IAMF or Atmos.
      - [ ] **Sony 360 Reality Audio** — object-based, music-streaming oriented. Niche but real.
      - [ ] **MPEG-H 3D Audio** — ISO standard, used in ATSC 3.0 broadcast. Patent-licensed.
      - [ ] **Higher-Order Ambisonics (HOA, scene-based)** — fully open, no licensing. Useful for VR / game-audio pass-through and as an internal mixer representation.
      - [ ] Format ordering in the dropdown puts the open-standard options (IAMF, HOA) first, followed by proprietary formats. The OS picks the highest-ranked supported format by default when the user selects "Spatial / object-based" without explicitly choosing a format.
      - [ ] Unsupported formats are listed but disabled with a hover tooltip explaining the missing capability (e.g., "HDMI sink does not advertise Atmos", "no IAMF encoder installed", "Bluetooth A2DP profile does not support DTS:X").
      - [ ] **Preferred-format priority list** (power-user, behind a "Show advanced" disclosure). Instead of locking in one format, the user can supply a reorderable priority list (default: IAMF → HOA → Atmos → DTS:X → 360 RA → MPEG-H → multichannel PCM → stereo). The OS uses the highest-ranked entry the active device supports; if the user later plugs in a different device that doesn't support the top choice, the OS automatically uses the next supported entry without re-prompting. Same ordinal model (one active format per device at any moment), just with a fallback chain instead of a single hard choice.
    - [ ] **App-input formats are independent of the device wire format.** Apps don't pick a bitstream format — they pick an *input* format to the mixer. A game might feed the mixer 7.1 PCM, a music player feeds stereo, a VR app feeds Ambisonics, an Atmos-aware movie player feeds Atmos objects, all simultaneously. The mixer composes them into a unified spatial scene and encodes that scene *once* into the device's selected wire format. Multiple apps with different native formats running at the same time is the normal case, not an edge case. The OS audio framework accepts any of: stereo / multichannel PCM, Ambisonics (1st through higher-order), Dolby Atmos objects, IAMF channel/scene/object payloads, and bitstream passthrough buffers (for apps that already hold the encoded stream).
    - [ ] **Per-device format selection is fully independent.** Every output device has its own layout / bitstream / HRTF selection. A user can simultaneously have an Atmos AVR on HDMI and stereo headphones on USB, both active, with different apps routed to different sinks (Spotify → headphones, movie player → AVR). Each device-output path picks the best format from its own advertised list with no cross-device interference. Per-app output routing (which app goes to which device) is a separate control in the per-app output mixer below.
    - [ ] **Exclusive passthrough** — checkbox per output device in the device's advanced section, plus a per-request OS API for apps to take exclusive hold temporarily. When an app holds the device exclusively:
      - [ ] The mixer steps aside completely for the duration. The OS's device-format selection is bypassed; the app's chosen bitstream goes over the wire untouched. (Required for fidelity-critical passthrough like Atmos Blu-ray, ASIO music production where the existing roadmap entry already covers this case, and exclusive-mode game audio with sub-millisecond latency budgets.)
      - [ ] Other apps routed to that device are muted-with-toast: a notification appears that says e.g. "Spotify can't play to HDMI — held exclusively by Plex" with one-click options to reroute Spotify to a different device or wait silently.
      - [ ] Exclusive hold is **capability-gated** (`audio.exclusive`) — granted at install time, never ambient. Users can revoke it per app in settings. Without the capability, the app's request to take exclusive hold fails and the app falls back to the shared mixer path.
      - [ ] Exclusive hold is **time-bounded by the app's session** — if the app crashes or exits, the device returns to the mixer automatically. No way for a misbehaving app to permanently capture a device.
    - [ ] **Headphone HRTF renderer** — dropdown, only meaningful when the active output device is a headphone (wired or Bluetooth). Orthogonal to the bitstream format above: HRTF takes any of the spatial / multichannel outputs and binaural-renders them down to stereo for headphones. Options:
      - [ ] **Off** — pure stereo passthrough, no HRTF processing. Lowest latency.
      - [ ] **Built-in HRTF (open)** — OS-provided open binaural renderer (based on libspatialaudio / IEM Plug-in Suite equivalents). Default when headphones are connected and the layout is Spatial or Multichannel.
      - [ ] **Dolby Atmos for Headphones** — Dolby's binaural Atmos renderer (when installed and licensed).
      - [ ] **Sony 360 Reality Audio for Headphones** — Sony's binaural 360 RA renderer.
      - [ ] **DTS Headphone:X** — DTS's binaural renderer.
      - [ ] Personalized HRTF: per-user HRTF profile (from ear-photo measurement or imported SOFA file) used by the Built-in HRTF renderer for better localization. Per-user, optional, never enabled silently.
      - [ ] Same disable-with-reason pattern: renderers that aren't installed are listed but disabled with a tooltip.
    - [ ] **Per-app output mixer** — full list (not capped at 10 like the popup) of every app with output sound in the current windowing session. Each row: app name, icon, volume slider, mute toggle, **per-app panning slider** (center detent). Per-app panning is applied independently of the device-level panning; both compose.
  - [ ] **Input** (covered separately in input-device settings — same dialog, separate section, mirroring the output controls where applicable: input device selector, master input level, per-app input gain, monitor toggle, noise suppression / echo cancellation toggles).
  - [ ] **Changes apply live.** No "OK / Cancel / Apply" buttons — adjusting any control takes effect immediately. A small "revert to last opened state" button in the footer for accidental changes, with a session-scoped undo history.
  - [ ] Settings persist per user. Per-device settings (volume, panning, channel config) persist per device and survive unplug/replug.
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
- [ ] Thumbnails for images, video, PDFs (built-in generators, registered through the same OS-wide mechanism as third-party generators — see §2.3 File Type Associations → Thumbnail-generator registration). Explorer is a thumbnail *consumer*, never an executor: every generator runs in its own sandboxed worker, so a buggy or malicious generator can never crash the explorer or compromise its caps.
- [ ] **Optional metadata labels in thumbnail / icon view.** Even in thumbnail (icon/grid) view — not just details view — the user can choose to show per-item **filename**, **date** (modified/created/taken, user-selectable which), and/or **size** beneath or beside each thumbnail. Each of the three is an independent toggle (any combination, including all off for a pure-image wall or all on for a labeled contact-sheet look), exposed in the view menu and persisted with the same per-folder / global-default preference mechanism as the detail columns. The point is that switching to thumbnail mode should not force the user to give up seeing basic file metadata: they get the visual preview *and* the name/date/size at a glance if they want it. Values reuse the same typed metadata source as the detail columns (so the date/size shown match, and sorting within thumbnail view is consistent). Long filenames wrap/ellipsize to a configurable line count so the grid stays even.
- [ ] **Thumbnail cache — stored in a dedicated OS directory, never alongside the source files.** Generated thumbnails are persisted in a centralized cache so re-listing a directory is fast and so the same thumbnail can be served to other consumers (file picker, image viewer, taskbar previews, etc.) without re-running the generator. The cache lives in a dedicated OS-managed directory (under the per-user state path, e.g. `~/.cache/os/thumbnails/` — exact path TBD; never in the user's source directories). Rationale:
  - [ ] **No `Thumbs.db`-style pollution.** The cache must never write sidecar files into the directories being thumbnailed. This avoids the Windows-era pattern where every photo folder ends up with a hidden cache file that complicates sync (cloud sync diffs noisy), syncs to other devices that don't want it, breaks read-only mounts, ends up in backups, and shows up in `ls -a` / scripted tooling that has to learn to ignore it. Source directories stay clean.
  - [ ] **One cache, many filesystems.** A single OS-managed cache means a thumbnail generated on the local SSD is reusable when the same file is later opened from a network mount, an external drive, or a read-only filesystem (which can't store a sidecar anyway). Cache keyed by content-derived identity (see below), not by path, so moves and renames don't invalidate.
  - [ ] **Cache schema.** SQLite-backed index file plus a blob directory (one file per thumbnail, named by hash) so individual thumbnails can be invalidated and recompacted without rewriting a monolithic database. Index columns: cache key, source path (most recent known), source mtime, source size, source inode, generator id + version, thumbnail size bucket, blob filename, last-accessed timestamp, byte size.
  - [ ] **Cache key (identity).** Primary key is `(source-content-hash, generator-id, generator-version, thumbnail-size-bucket)`. The content hash is taken from a lightweight signature — a fast hash over (size, mtime, first 64 KiB, last 64 KiB) is typically sufficient and avoids reading huge files; a full content hash is used for files below a threshold (e.g. 16 MiB) where it's cheap and worth the precision. Falling back to `(absolute-path, mtime, size)` only when content hashing fails (e.g. permission denied). The hash strategy is documented and versioned so the cache can be migrated forward if the strategy changes.
  - [ ] **Invalidation.** Cache entries are invalidated automatically when source mtime/size changes (caught either by the lookup path before serving a stale thumbnail or by the filesystem change-notification stream the OS already maintains for the directory-size cache). Generator-version bumps invalidate all entries from that generator. Moves and pure renames do *not* invalidate, because the cache key is content-derived; the cache just updates the "most recent known source path" field on next access.
  - [ ] **Size cap with the same model as the directory-size cache.** Install-time default fixed byte cap based on total disk size at install (default ~0.5% of the system drive, e.g. ~5 GiB on a 1 TB drive — thumbnails are larger and longer-lived than dir-size entries, so a more generous default is warranted). Settings UI exposes a single byte-count control with a "(recommended)" tip computed in the moment against current free space, and a one-click "use recommended" button — same pattern as §4.1's directory-size cache so users learn one model. Pressure-aware shrinking via the kernel shrinker subsystem: under disk-space or memory pressure the cache evicts cold entries by LRU all the way to zero, then refills toward the cap when pressure subsides. Same anti-pattern note applies — do *not* size the cap by free disk; size the *fill* by pressure.
  - [ ] **Per-user, never cross-user.** Each user has their own cache directory under their own per-user state path. A thumbnail one user generated for a file they could read is never served to another user, even when they request a thumbnail for the same file — that second user goes through their own generator invocation, which the sandbox restricts to files *they* can read. This prevents the cache from leaking thumbnails of files the requesting user wouldn't have been able to open themselves.
  - [ ] **No cache in source directories, ever.** Hard rule, enforced by the framework: the generator-runner API does not accept "write near the source" as an option, and the source-file path passed to the generator is read-only-mounted in the sandbox. A misbehaving or malicious generator that tries to write `Thumbs.db`-style sidecars cannot do so because the sandbox has no write capability outside the OS cache directory.
  - [ ] **Cache management UI.** Settings → Storage shows the cache's current size and entry count, with a "clear cache" button (forces full regeneration on next view), a per-extension breakdown ("`.psd` thumbnails: 412 entries, 318 MB"), and the byte-count cap control. Power users can disable the cache entirely per extension — useful for highly-volatile file types where the cache miss rate would be high enough that caching wastes space.
  - [ ] **Backup-aware.** The cache directory is marked with a "do-not-back-up" attribute so the OS backup tool (§4.4) skips it by default — losing the cache is harmless, regenerating it is just CPU time. Same attribute used by other regenerable caches (page cache spillover, font atlases, etc.) to keep backups focused on irreplaceable data.
- [ ] Detail column view:
  - [ ] **No content-based column auto-selection.** The OS never inspects a directory's contents to decide which columns to show. A folder containing only audio files does *not* automatically gain bitrate/length/sample-rate columns; a folder of photos does *not* automatically gain width/height/camera columns. The visible column set is determined exclusively by (a) the user's explicit per-view preference, (b) the user's saved default for "all folders" or for the specific folder, or (c) the OS's initial out-of-the-box default — never by sniffing what's inside the directory. Rationale: content-sniffing makes the column set jitter as the user navigates (same view changing shape when the user enters or leaves a folder of mixed content), creates surprise when a single off-type file in an otherwise-uniform directory suppresses or restores a column, makes "why did my columns change?" answers require explaining the heuristic, and forces the OS to scan every file's type on every directory listing just to decide chrome. The user is in charge of which columns are shown; the OS just shows them.
  - [ ] User can show/hide any column from a column-picker dropdown on the header row (Windows-style). Changes apply to the current view, with a "save as default for all folders" / "save as default for this folder" option in the same menu.
  - [ ] User can save per-folder column preferences (the chosen set, order, and widths persist with the folder so revisiting it restores the view).
  - [ ] User can save a global default column set (used for any folder without a saved per-folder preference).
  - [ ] Out-of-the-box default column set is fixed and minimal — name, size, datetime modified — independent of what's in any directory. The user expands from there.
  - [ ] Apps can register custom detail columns and file decoders (capability-gated; same model as thumbnail-generator registration in §2.3 — extension-scoped, sandboxed worker reads the file, returns the column value, never runs in-process with explorer)
  - [ ] **Directories have a size column too** — recursive total of contents (matching the visual reference in `Aero Desktop (offline).html`). Most file managers leave this blank because computing it on every directory listing is expensive; we cache instead (see directory-size cache below).
  - [ ] **Enablable columns — full initial list.** All values shown blank when not applicable to the row's file type. Sort works on every column. Each column maps to a typed value (integer/float/duration/datetime/string/enum) so sorting is correct and not lexicographic on numeric content.
    - **Filesystem / general (apply to any file or directory):**
      - [ ] Name
      - [ ] Extension
      - [ ] Size (bytes — formatted with binary unit suffix in the cell, raw bytes for sorting)
      - [ ] Brief description (short human-readable file-type label, e.g. "JPEG image", "Markdown document", "MP3 audio". Sourced from the file-type registry — every registered extension carries a description string. **Set by the owning application** at registration time (manifest field alongside the extension list — see §2.3 File Type Associations and `shell.fileassoc.register` in §1.5): when a program pushes itself onto the launch stack for `.foo`, it also supplies the brief description it wants shown for `.foo`. The active description follows the active owning application — if the user re-prioritizes the launch stack or the top-of-stack handler is uninstalled, the description shifts to whatever the new top-of-stack handler provided. The user can override any registered description in settings and the override persists across handler changes. If no description is registered (and no override exists), falls back to the owning-application name (see next column); if no application owns the extension either, falls back to the extension itself in uppercase, e.g. "FOO file".)
      - [ ] Owning application (the application currently at the top of the launch stack for this file's extension — see §2.3 File Type Associations. Blank when no application owns the extension. This is the column that changes when the user installs / uninstalls / re-prioritizes a handler; "Brief description" is mostly stable across handler changes.)
      - [ ] Datetime created
      - [ ] Datetime last modified
      - [ ] Datetime last read (atime, subject to the OS's relatime-style update policy — see §2.x atime semantics; if atime updates are disabled for a mount the column shows the last reliably-known value with a small marker indicating it may be stale)
      - [ ] Owner (user)
      - [ ] Permissions / capability summary
      - [ ] Path (full absolute path — useful in search results)
    - **Directory-only:**
      - [ ] Files (recursive count of all files in subtree — served from the directory-size cache, same staleness semantics as the size column)
      - [ ] Subdirectories (recursive count of all subdirectories in subtree — same cache, same staleness semantics)
      - [ ] Immediate children count (non-recursive — cheap, always fresh)
    - **Image columns:**
      - [ ] Width (pixels)
      - [ ] Height (pixels)
      - [ ] Megapixels (derived — useful sort key)
      - [ ] Aspect ratio
      - [ ] Color depth (bits per pixel)
      - [ ] Camera make / model
      - [ ] Date taken (EXIF DateTimeOriginal — separate from filesystem create/modify)
      - [ ] Exposure time, aperture (f-stop), ISO, focal length
      - [ ] Lens model
      - [ ] Flash fired (yes/no)
      - [ ] GPS coordinates (location)
      - [ ] Orientation (EXIF — 1..8)
    - **Audio columns:**
      - [ ] Title
      - [ ] Artist
      - [ ] Album
      - [ ] Album artist
      - [ ] Track number / disc number
      - [ ] Genre
      - [ ] Year (release year)
      - [ ] Length (duration)
      - [ ] Bitrate (kbps)
      - [ ] Variable bitrate (yes/no)
      - [ ] Sample rate (Hz)
      - [ ] Number of channels
      - [ ] Channel layout (mono / stereo / joint stereo / 5.1 / etc.)
      - [ ] Sample bit depth
      - [ ] Audio codec
      - [ ] Composer
      - [ ] Comment
      - [ ] ReplayGain (track / album)
    - **Video columns:**
      - [ ] Width (pixels)
      - [ ] Height (pixels)
      - [ ] Resolution label (e.g. "1080p", "4K UHD")
      - [ ] Frames per second
      - [ ] Length (duration)
      - [ ] Bitrate (kbps)
      - [ ] Variable bitrate (yes/no)
      - [ ] Video codec
      - [ ] Audio codec (of the embedded audio track)
      - [ ] Number of audio tracks
      - [ ] Number of subtitle tracks
    - **Document columns:**
      - [ ] Title
      - [ ] Author
      - [ ] Description
      - [ ] Page count
      - [ ] Word count
      - [ ] Application that created the document
    - **Location / geotagging (when present in any media type):**
      - [ ] Location (GPS coordinates, latitude/longitude; renders as a clickable link that opens the map app)
      - [ ] Place name (reverse-geocoded — capability-gated and offline-only; never sends file location to a network service without an explicit per-mount opt-in)
  - [ ] **Future-expansion notes (TODO before shipping detail-column view):**
    - _Look into adding many more columns that Windows Explorer supports out of the box (Windows ships several hundred property keys — `System.Photo.*`, `System.Music.*`, `System.Document.*`, etc.). Pick the long tail that's actually useful and skip the noise._
    - _Look into the full EXIF tag set (`Make`, `Model`, `LensInfo`, `WhiteBalance`, `MeteringMode`, `SubjectDistance`, IFD0/Exif/GPS/Interop IFDs)._
    - _Look into the full ID3v2 frame set (`TCOM` composer, `TPUB` publisher, `TBPM` BPM, `TCMP` compilation, `USLT` lyrics, `APIC` embedded cover-art presence flag) plus Vorbis comments, MP4 atoms, FLAC tags, and other format-native metadata._
    - _Look into the column set Foobar2000 exposes for audio files — it's the most comprehensive in the wild and a good reference for what columns power users actually want (track gain, album gain, dynamic range / DR, encoding tool, encoder settings, codec profile, CUE sheet presence, embedded cover art presence, etc.)._
    - _All additional columns should land through the same registration mechanism third-party apps use, so first-party and third-party live on equal footing — no privileged in-process path._
- [ ] **Directory-size cache (OS-wide service).** A persistent on-disk cache of recursively-computed directory sizes, keyed by inode (or path on filesystems without stable inodes). Any tool can query it — file explorer, `du` equivalent, settings/storage panel, backup tool — so the work isn't duplicated.
  - [ ] **Lookup contract:** caller asks for a directory's recursive size. Cache returns either (a) a cached value with a "fresh" marker, (b) a cached value with a "stale, recomputing" marker plus a subscription handle that fires when the recompute finishes, or (c) "unknown, computing" with the same subscription handle. Callers (file explorer) show the cached value immediately and update the row in place when the subscription fires — no blocking on cold cache.
  - [ ] **Invalidation is event-driven, not polled.** The cache subscribes to the filesystem's change-notification stream (the same mechanism that powers live file-explorer refresh). Any create/delete/resize/rename anywhere under a cached directory invalidates that directory's entry *and* every ancestor entry up to the root. No periodic scans; no time-based expiry.
  - [ ] **Background recompute.** Invalidated entries are recomputed in the background by a low-priority worker, throttled so it doesn't compete with foreground I/O. Recompute uses the same change-notification stream to short-circuit: if a subdirectory hasn't been invalidated since the last successful recompute, reuse its cached size instead of walking it again. This makes a "small change in a huge tree" cost proportional to the changed subtree, not the whole tree.
  - [ ] **Persistence.** Cache lives on disk so it survives reboots. Stored under the per-user state directory. On mount, the cache is presumed valid only if the filesystem's change-notification stream has been continuously observed since the last clean unmount — otherwise entries on that mount are marked stale and recomputed lazily on first access.
  - [ ] **Permission model.** Cache entries are stored per-user, since a recursive size depends on what the querying user can read. Cross-user queries either recompute under the querying user's identity or return "unknown" — never leak sizes a user couldn't have computed themselves.
  - [ ] **Bounded memory: constant cap, install-time default based on total RAM.** In-memory hot set; cold entries spill to disk. The OS doesn't track every directory ever seen — only those that have been queried.
    - [ ] **Install-time default:** at install, the OS picks a fixed byte cap based on total RAM at that moment (default ~0.1%, e.g. ~16 MiB on a 16 GiB machine, ~64 MiB on a 64 GiB machine). After install the cap is just a number — no auto-scaling.
    - [ ] **Settings UI exposes one control:** a byte count. Users who want more or less change it directly. Total RAM rarely changes after install, so making the cap "follow" RAM doesn't earn its complexity; the rare user who adds RAM and notices the cache feels tight can raise the number once and be done.
    - [ ] **Recommended-value tip beside the control.** Next to the byte-count field, display the OS's recommended amount computed *in the moment* against current total RAM (same ~0.1% rule used at install). If the user has added RAM since install, the tip will read higher than the currently-set value — a low-friction nudge that they could raise the cap if they want, without the OS silently doing it for them. A one-click "use recommended" button next to the tip applies it. If current setting already matches the recommendation, the tip just shows "(recommended)" next to the value with no nag.
    - [ ] **Live application:** raising the cap lets more entries stay hot immediately; lowering it triggers an eviction pass.
    - [ ] **Pressure-aware shrinking (not pressure-aware sizing).** The cap is the ceiling, not the floor. The cache registers a shrinker callback with the kernel's memory-pressure subsystem; under pressure the kernel asks it to evict cold entries (LRU), and the cache complies all the way down to zero if pressure persists. When pressure subsides, the cache refills toward the cap as queries come in. The user-visible effect: on an idle machine the cache happily sits at its cap; under pressure it gets out of the way automatically; recovery is automatic too. Same pattern the kernel uses for the dentry cache, inode cache, and slab caches — well-trodden machinery, not a new invention.
    - [ ] **Anti-pattern (do not do this):** sizing the *cap* by free memory rather than the *fill* by memory pressure. Free memory is a moving target that points the wrong direction at the worst moments (huge at boot when caching does nothing, near-zero right before swap when caching matters most), is ambiguous in definition (`MemFree` vs. `MemAvailable`), and would put the dir-size cache in direct competition with the page cache over the same accounting. Pressure-driven shrinking gives you the *intent* of "use memory when slack exists, give it up when it doesn't" without any of those problems.
  - [ ] **Capability:** the cache service exposes a query handle as an OS capability so apps don't get ambient access. File explorer holds it by default; other apps must be granted.
- [ ] View options: list, thumbnails (any size), column view, order by any column
- [ ] **Manual (custom) file ordering with persistence.** The user can drag files/folders into an arbitrary order within a directory and that ordering is remembered — it persists across navigations, refreshes, and reboots (stored per-directory in user-side state, keyed by directory identity, not baked into the filesystem). This is a distinct sort mode ("Custom" / "Manual"): while it is active, drag-to-reorder is enabled and new/renamed items land in a defined place (e.g. appended, then draggable). **Precedence rule:** enabling any "order by *column*" (name, size, date, type, etc.) **temporarily** overrides the manual order — the view shows the column sort while that sort is active — but it does **not** discard the saved manual order; switching back to Custom restores the user's hand-arranged sequence intact. So column sorts are a non-destructive temporary view on top of the persisted manual arrangement. Applies to the shared file-explorer component, so the file open/save dialogs inherit it.
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
- [ ] **Save dialog remembers the suggested default filename for one-tap restore.** When an application opens a save dialog it typically pre-fills a suggested filename (e.g. "Untitled.txt", "invoice-2026-07.pdf", the current document title). If the user, while navigating to a target path, accidentally overwrites or edits that filename field (e.g. they meant to type in the *path* bar but focus was in the *name* field, or they cleared it), the dialog must let them get the original suggested default back trivially — the app-supplied default is retained for the lifetime of the dialog and offered via a restore affordance (a "reset to suggested name" control / placeholder text / one-tap revert), so a slip doesn't force the user to remember and retype what the app proposed. The retained value is the *app's* suggested name at open time; it is not overwritten by the user's edits (those live in the editable field), so revert is always available even after multiple edits. Applies to the shared OS save-dialog component so every app benefits uniformly.
- [ ] Drop zones for drag-and-drop
- [ ] Atomic copy/move/delete with undo, resume on interruption
- [ ] **Durable, resumable bulk file operations (survive crash / restart / accidental abort).** A long-running mass copy/move/delete must be journaled to persistent OS state as it runs — a per-operation record of the source set, destination, per-file progress (which items are done, in-flight, pending), and the chosen conflict resolutions — so that if it is interrupted for *any* reason (app crash, system restart, power loss, the user accidentally aborting or closing the window), the OS knows the operation was still in progress and can resume it rather than leaving a half-finished mess. On next login (or when the filesystem/service layer comes back up), the user is **reminded**, when appropriate, that "N file operations were interrupted" and offered resume/discard. There is also a **central place** — a "File Operations" / "Transfers" view (surfaced in file explorer and/or the notification pane) — where the user can see all interrupted *and* in-progress bulk operations and resume, retry, or cancel them individually. Design ties: reuse the same operation-journal that powers "resume on interruption" above and the `files.transfer_progress` event (§4.10 service API); deletes to the recycle bin are trivially resumable, permanent deletes and moves journal each item's committed/uncommitted state so resume never double-copies or loses a file. Idempotent replay: each journaled step records enough to re-check the target state and skip already-completed items.

### 4.2 Text Editor

_Custom Python (fastpy) text editor. Editing engine is a toolkit widget (Phase 3.5 TextEdit). App is a thin wrapper. All apps get the engine for free via the widget._

- [ ] Text editor app: tab bar for multiple open files
- [ ] Text editor app: file open/save with encoding detection
- [ ] Text editor app: split panes (horizontal/vertical)
- [ ] Text editor app: minimap
- [ ] Text editor app: session restore (remember open tabs, cursor positions)
- [ ] Text editor app: plugin system (Python scripts)
- [ ] Text editor app: status bar (line/col, encoding, language, indentation mode)
- [x] **External-change detection with three-way merge.** When a file open in an editor is modified on disk while the in-editor buffer has unsaved edits, the editor does not silently clobber or blindly reload: it presents a modal offering **keep current changes**, **reload from disk** (discard buffer edits), **auto-merge** (three-way diff3 merge of buffer vs. disk against the common ancestor), or **review merge** side-by-side (per-conflict take-ours / take-disk / keep-both, then accept). A buffer with *no* unsaved edits reloads automatically. Implemented in both `apps/editor` and `apps/markdowneditor`; the shared engine lives in `apps/diffcore` (`diff3` three-way merge + `FileSync` external-change tracker + `MergeReview`), using the same LCS-based approach as orchestrator2's file-edit review viewer. _(Done — see todo2 item 18.)_

### 4.3 Process Explorer

- [ ] Identify process by clicking window, kill it
- [ ] Find process by name
- [ ] Pause, resume, kill, change priority, restart
- [ ] Show all libraries loaded by process
- [ ] Show all subprocesses and threads
- [ ] Show: capabilities, running user, priority levels, app name, what launched it, is it a service, what's blocking it, what's waiting on its locks, running/paused status, full path
- [ ] **Launch provenance (command line + who/when started it).** Each process row must surface the full launch context: (a) the *command line* it was launched with — the executable path plus every argument (argv), and if any, the parameters/flags it was passed — shown verbatim when available, and clearly marked "(no arguments)" or "(command line unavailable)" when the process was spawned without a recorded argv or has since cleared it; (b) the *originating process or thread* that launched it — the parent process (and specific thread, when the kernel records launcher thread id) resolved to a clickable identity so the user can jump straight to the launcher's row, with graceful "(launcher exited)" / "(launched by init)" fallbacks; and (c) the *launch timestamp* (wall-clock time the process was created), shown both absolutely and as an elapsed "started N ago" so the user can correlate a process with something they just did. This is read-only informational metadata; the command line is captured by the kernel at `exec`/spawn time (the same argv/envp the loader already stores) and exposed through a `system.*` process-query capability, never self-declared by the process (so it can't lie about how it was invoked). Long command lines are truncated in the row with full text on hover / in the detail pane, and are copyable.
- [ ] Switch to any window or terminal a process owns
- [ ] System resource graphs (CPU, RAM, disk, network over time)
- [ ] **Service-mediated resource attribution.** When a program's resource use flows through an OS service process — e.g., the program writes to a TCP socket and the network stack runs in a separate daemon, or the program opens a file and the disk I/O is performed by a filesystem service, or the program asks the audio service to mix samples — Process Explorer must attribute the resource consumption back to the *originating* program, not to the service. So a user sees "Firefox is using 4 MB/s of network" and "Slack is using 12% disk I/O" even though the kernel-visible network/disk traffic actually flows through the network/storage daemons.
  - [ ] **Mechanism.** Every service-mediated request carries the originator's identity through the IPC chain as a kernel-stamped tag (not a self-declared field — services can't lie). The service processes' accounting layer attributes bytes/cycles/I/O back to that tag. Process Explorer queries the kernel for a flattened "if I look through every service, who's actually using this resource" view alongside the raw "which process holds the syscall" view.
  - [ ] **Per-resource breakdown.** Each row in Process Explorer can expand to show "via which service" for that program's CPU / memory / disk / network — so a user troubleshooting "why is the disk thrashing" can see both "Backup.app is generating 200 MB/s of writes via the filesystem service" *and* "the filesystem service has 200 MB/s of writes pending, originated by Backup.app." Both views are consistent and reconcile to the same totals.
  - [ ] **Sort and filter by attributed resource.** "Sort by attributed network" puts the actual bandwidth consumers at the top, not the network service. Same for disk, memory, CPU.
  - [ ] **Discover-the-culprit UX.** Top bar of Process Explorer shows the top-3 attributed consumers per resource (CPU / RAM / disk / net) at a glance. One click drills into that program. The user shouldn't have to know that "the network service is at 60% CPU because of Firefox" — they should see "Firefox is at 60% CPU (via network service)" without expanding anything.
  - [ ] This attribution is also what powers the §2.8 file-lock error messages: the kernel-stamped originator tag is what lets a "Locked by Photos.app (via filesystem service)" message exist at all.

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
  - [ ] **Name renderer/helper processes so Process Explorer can map each to its tab.** Chromium spawns a process per site/tab (renderer), plus GPU/utility/network-service helpers. When the user opens Process Explorer (§4.3) and sees a fleet of Chromium processes, each must carry a human-meaningful name that identifies *which tab* (or which helper role) it is — e.g. "Chromium — GitHub · Pull Request #123 (renderer)", "Chromium — GPU process", "Chromium — Network service" — not an opaque `chromium-bin ... --type=renderer` command line. Wire Chromium's per-process title/description (it already tracks the primary document title and process type internally) into whatever OS mechanism sets a process's Process-Explorer display name, updating the renderer's name when the tab navigates or the title changes. This makes "which Chromium process is eating my CPU/RAM" answerable at a glance and lets the user kill a single runaway tab's process without guessing. Ties into §4.3 launch-provenance (the raw `--type=` command line is still shown as provenance; the friendly per-tab name is the primary label).
- [ ] System web app framework (shared Chromium, not per-app Electron)
  - [ ] **Electron-compatible app runtime backed by the shared system Chromium.**
    Ship an Electron-like runtime (Chromium renderer + a Node-compatible main
    process + the Electron-shaped app/BrowserWindow/IPC APIs) that is served by the
    *one* system Chromium install rather than each app bundling its own ~100–200 MB
    copy of Chromium+Node. The goal: existing Electron apps (VS Code, Slack,
    Discord, Obsidian, etc.) run **without shipping the entire browser** — they
    provide only their HTML/JS/CSS + main-process code and link against the shared
    runtime. Provide enough of Electron's main/renderer API surface (or a
    compatibility shim over it) that mainstream Electron apps run unmodified or with
    minimal patches; version the shared runtime so apps can target a known API
    level. Each such app still gets its own sandboxed renderer/main processes (named
    per §4.7 so Process Explorer maps them), but the heavy Chromium/Node binaries
    are deduplicated system-wide (content-addressed store, §pkg). This is the OS's
    answer to "every Electron app ships a browser": one browser, many web apps.
- [ ] Port VS Code (via Chromium + Node.js) — full IDE, not just an editor
  - [ ] Build from the **open-source `Code - OSS` tree (MIT)**, not Microsoft's branded binary. Microsoft's MIT license permits doing essentially anything with the open-source source **except** connecting it to the **Microsoft Visual Studio Marketplace** (that endpoint is reserved for the official MS build). So the port must ship pointed at an open extension registry (e.g. **Open VSX**) or a self-hosted registry instead of the MS marketplace — same constraint VSCodium operates under.
  - [ ] Integrate with our native toolchains so build/debug/run work out of the box: the **ported C/C++ compiler** (gcc/clang via POSIX layer), the **Rust compiler** (native target), and the **fastpy Python compiler** (AOT). Ship/auto-detect language extensions and task/launch templates wired to these toolchains, plus debugger integration against the OS crash-dump/postmortem debugger and the `debug.*` capabilities.
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
- [ ] Port a debugger (gdb/lldb) — both live attach (`debug.*` capabilities) and **postmortem dump-file loading** (opens the crash dumps from §1.5 → Crash Dumps & Postmortem Debugging, re-symbolicates against recorded store paths). Dump format chosen to match what the ported debugger understands (minidump/ELF-core-compatible) so minimal porting is needed.

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

#### Automatic Widget-Level Exposure (every interactive widget gets this for free)

Beyond the app-author-declared events/actions above, **every interactive widget
built with the GUI toolkit (§3.5) automatically exposes itself to the automation
API** — no per-app code required. The toolkit maintains a live, walkable tree of
the app's widgets (an accessibility/UI-automation tree, à la Windows UIAutomation
/ AT-SPI / macOS AX), and any program holding the right capability can **identify**
and **interact with** individual controls. This is what makes generic UI
automation, macro recording/replay, and assistive tech work against apps whose
authors never wrote a single automation handler.

- [ ] **Automatic exposure by the toolkit.** Buttons, checkboxes, radios, toggles,
  text fields, combo boxes, list/tree/grid rows and cells, menu items, tabs,
  sliders, scrollbars, ribbon commands, etc. are each surfaced as an automation
  node the moment they're created — the app opts *out*, not in. Custom-drawn or
  canvas widgets can supply their own nodes via a toolkit hook so they aren't
  invisible to automation.
- [ ] **Identify (query/enumerate).** Walk the widget tree; query each node's
  role/type, label/accessible name, value/state (checked, text, selection,
  enabled/visible/focused), bounds, and a stable within-app identifier. Find
  widgets by role, name, text, or path so a script can locate "the *Save* button"
  or "the search field" without pixel coordinates. Exposed as standard automation
  meta-commands/actions (e.g. `ui.tree`, `ui.find`, `ui.get`, read-only
  `ui.*` properties) alongside `describe`.
- [ ] **Interact (invoke).** Perform the widget's semantic action through the
  toolkit — click/press a button, toggle a checkbox, set a slider/field value,
  select a list item or menu entry, focus/scroll a control — routed through the
  real widget so it behaves exactly as a user action (validation, events,
  enabled-state all honored). No synthetic pixel clicks needed. Actions respect
  the widget's own enabled/visibility state (a disabled control refuses invoke).
- [ ] **Capability-gated, same model as the rest of the framework.** Enumerating a
  program's widget tree needs `automation.connect` + a new
  `automation.ui_inspect` right; interacting needs `automation.invoke` +
  `automation.ui_control`. Sensitive/destructive widgets (a "Delete" button, a
  password field) can demand an elevated right, and password/secure-entry fields
  are never readable via automation regardless of capability. Assistive tools
  (screen readers, switch access, voice control) consume the *same* tree, so
  automation and accessibility share one implementation.
- [ ] `automate` CLI and the `on`/`invoke` shell builtins gain widget-tree
  subcommands (e.g. `automate ui <program> tree|find|invoke`) so widget
  automation is scriptable exactly like declared actions.

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

### 5.0 Advanced Options Convention (cross-cutting)

_A Settings-wide UX convention for risky/expert knobs. Applies to every
section below — any setting that can degrade stability, performance,
compatibility, or security if misused is tagged **Advanced** and surfaced
through this shared pattern so users learn one mental model. (User-directed:
see design-decisions.md §11, the Option 5 "all Q2 options flagged as advanced
options with warnings" decision.)_

- [ ] **`advanced` flag on settings.** Each setting carries an `advanced:
  bool` attribute in its schema. Advanced settings are collapsed by default
  behind a per-section "Show advanced options" disclosure (off by default);
  non-advanced settings are always visible. The flag is data, not per-widget
  code, so the same rendering/warning logic covers every section.
- [ ] **General advanced-options warning.** The first time a user expands any
  "Show advanced options" disclosure (per session, or until dismissed with
  "don't warn again"), show a single general warning: *changing advanced
  options can cause instability, data loss, performance regressions, or
  programs to stop working.* This is the blanket warning that covers all
  advanced options so individual knobs don't each need their own modal.
- [ ] **Per-setting inline warnings.** Individual high-risk settings may add
  their own short inline caution (and, for the most dangerous, a confirm
  dialog) on top of the general warning — e.g. partition manager (data loss),
  system-wide memory-commit policy (§5.6), firewall disable.
- [ ] **Search surfaces advanced options** but marks them with an "Advanced"
  badge in results, so they're findable without hunting through disclosures.
- [ ] **Reset-to-default** is always offered for advanced sections (per-setting
  and per-section), since advanced knobs are the ones users most often need to
  back out of.

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

- [ ] **System-wide memory-commit policy selectors (Advanced) — two, one per
  ABI.** Choose the default allocation strategy for the whole system:
  **strict-commit** (every mapping guaranteed backed by RAM+swap — "committed by
  default") vs **lazy/overcommit** (mappings backed on first touch). The native
  and Linux ABIs get **independent** selectors because their idioms differ:
  - [ ] **Native** default — front-end for `sysctl mm.lazy_default`
    (`PARAM_MM_LAZY_DEFAULT`); defaults to strict-commit per the design spec.
  - [ ] **Linux** default — front-end for `sysctl mm.linux_lazy_default`
    (`PARAM_MM_LINUX_LAZY_DEFAULT`); defaults to lazy/overcommit (Linux programs
    expect it). Reflected in `/proc/sys/vm/overcommit_memory`.
  These are the user-facing front-end for the Option 5 kernel core that already
  exists. **Advanced + warning** (§5.0) — flipping a global default affects every
  program of that ABI. Gated by the `admin.memory_policy` capability (§1.5) since
  it changes a system-wide policy. Show the per-strategy tradeoffs (strict = no
  surprise OOM but apps that reserve huge sparse arenas may fail to start; lazy =
  max compatibility but allocations can fail late on touch). See
  design-decisions.md §11.
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
- [ ] **Per-program memory-commit policy override (Advanced).** Override the
  default commit strategy for a single program — force **strict-commit** or
  **lazy/overcommit** regardless of the system-wide default — to work around a
  misbehaving app (e.g. force a leaky program to strict-commit, or let one app
  overcommit). Front-end for the existing kernel mechanism
  (`pcb::MmapCommitPolicy` {Inherit, ForceCommitted, ForceLazy}, consulted by
  both `mmap` paths). Applies to both native and Linux-ABI programs. **Advanced
  + warning** (§5.0). Changing a program's *own* override is a normal user
  action and does **not** require an elevated capability (only the system-wide
  default needs `admin.memory_policy`). When the OS detects a program may be
  failing due to commit policy (allocation failures, refuse-to-start), surface
  a contextual hint pointing the user here with an explanation. See
  design-decisions.md §11.
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

#### 6.1a WSL2-style Linux VM (OPTIONAL — consider only after everything else is done)

_Operator note (2026-06-14): a true VM-backed Linux environment, in the spirit of WSL2, as a **fallback/alternative** to the syscall-translation layer (§6.1) — not a replacement for it. The translation layer (Linuxulator approach) remains the primary path because it runs Linux binaries as first-class Slate processes (shared scheduler, IPC, page cache, no second kernel). The VM is for the long tail the translation layer can't reach: programs needing a real Linux kernel (custom kernel modules, exotic ioctls/netfilter, kernel-version-specific behaviour). Deferred until the core OS is otherwise complete; recorded here so the idea isn't lost._

- [ ] Type-2/lightweight hypervisor on our microkernel (WHPX/KVM-style HW virtualization; we already run *under* Hyper-V/WHPX per the bench harness, and already activate hypervisor guest features — see Phase 1 "VMware tools equivalent")
- [ ] Run a real upstream Linux kernel image in the guest (like WSL2's bundled kernel)
- [ ] Deep host/guest integration (the WSL2 value proposition): share the **filesystem** (9p/virtio-fs-style bridge to the host VFS), **clipboard**, **window manager / display** (guest X11/Wayland clients composited into the host compositor, à la WSLg), **GPU driver** (paravirtual GPU passthrough for accelerated guest rendering), and network namespace
- [ ] Per-distro image management (download/import/export distro rootfs, à la `wsl --import`)
- [ ] Study: Windows Hyper-V architecture, VMware/VirtualBox device models, QEMU/KVM (QEMU-TCG is slow because it's pure emulation; with KVM/WHPX acceleration it is fast — the relevant comparison is accelerated, not TCG)
- [ ] Decision deferred: own minimal VMM vs. porting an existing one (crosvm/QEMU). crosvm (Rust, Chrome OS) is the closest fit to our stack

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
