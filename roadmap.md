# OS Development Roadmap

Status key: `[ ]` not started, `[-]` in progress, `[x]` done, `[~]` deferred

---

## Phase 0: Project Foundation

_No dependencies. Do this first._

- [ ] Choose a project name
- [x] Set up git repo, CI, build system (cargo workspace for kernel)
- [x] Set up QEMU/VirtualBox dev loop (edit on Windows, cross-compile, boot in VM)
- [x] Set up Rust cross-compilation (`x86_64-unknown-none` target)
- [x] Choose and configure bootloader (Limine or UEFI via `uefi` crate)
- [x] Write CLAUDE.md / coding standards for AI-assisted development
- [x] Set up benchmark infrastructure (for measuring performance as the OS grows)
  - [x] TSC calibration via PIT channel 2 (~10ms reference)
  - [x] rdtsc / rdtsc_serialized cycle-accurate timing
  - [x] Benchmark runner with warmup, min/mean/max reporting
  - [x] Standard kernel benchmarks: page alloc, heap alloc, compression
  - [x] Baselines in bench/baselines.toml with Linux/Fuchsia references
  - [x] Self-test verifying TSC frequency and conversion accuracy
- [ ] Integrate fastpy compiler into build system (Python AOT → native executables for OS components)

---

## Phase 1: Kernel Core

_The minimum kernel that can run a single userspace process._

### 1.1 Boot and hardware init
- [x] UEFI boot → enter kernel entry point (Limine bootloader, kmain entry)
- [x] Parse ACPI tables (RSDP→RSDT→MADT: I/O APIC address, IRQ overrides, processor count)
- [x] Initialize GDT, IDT, interrupt handlers
- [x] Set up 16 KiB page tables (4×4KiB hardware pages per logical frame)
- [x] Set up kernel heap allocator (slab allocator, power-of-2 size classes)
- [x] Initialize serial console for debug output (UART 16550, COM1)
- [x] Initialize PCI bus enumeration
- [x] SMP bootstrap (wake Application Processors via INIT-SIPI-SIPI)
  - [x] AP trampoline: hand-encoded 16-bit→32-bit→64-bit mode transitions
  - [x] Identity mapping for trampoline execution (0x0..0x10000)
  - [x] Low memory reserve: first 1 MiB excluded from frame allocator
  - [x] Per-AP kernel stack allocation, GDT/IDT/APIC init on each AP
  - [x] LAPIC ID → sequential CPU index mapping
  - [x] Scheduler CPU count update after all APs come online
  - [x] Per-CPU GDT and TSS (independent RSP0/IST stacks per CPU)
  - [x] TLB shootdown via IPI (vector 251, broadcast + ack protocol)
  - [x] Tickless idle on APs: stop periodic timer when idle, wake only on reschedule IPI
  - [x] Tested with 1, 2, and 4 CPUs under QEMU
  - [x] CPU feature detection: centralized CPUID caching (SSE3-4.2, AVX/AVX2/AVX-512, XSAVE, AES-NI, SHA, RDRAND, RDSEED, RDTSCP, 1GiB pages)

### 1.2 Memory manager
- [x] Physical page allocator (buddy allocator for 16 KiB pages)
  - [x] Per-CPU frame caches (lock-free single-frame alloc/free, batch refill/drain)
  - [x] Per-frame reference counting (u16 refcount array for CoW support)
  - [x] Zone-aware constrained allocation (DMA below 16M / below 4G, free-list scan)
- [x] Per-CPU heap slab caches (two-tier: interrupt-disabled local free lists + global spinlock)
- [x] Virtual memory manager (page tables, mapping, unmapping)
  - [x] Copy-on-Write page fault handling (COW PTE flag, resolve_cow_fault)
  - [x] CoW batch optimization: sole-owner eagerly resolves all 4 sibling PTEs
  - [x] CoW batch optimization: shared-frame copies all 4 pages into single new frame
- [x] Kernel virtual address space layout
- [x] Userspace virtual address space layout
- [x] Demand paging (page fault handler, lazy allocation)
- [x] Stack growth via page fault (guard page at bottom)
- [x] Swap file support (not partition)
  - [x] Swap subsystem infrastructure: slot allocator (bitmap), swap entry PTE format, in-memory backend
  - [x] `swap_out_page()` and `swap_in_page()` for evicting/restoring user pages
  - [x] Page fault handler integration: detects swap PTEs and triggers swap-in
  - [x] Sysctl parameters: mm.swappiness (default 15), mm.min_free_pages (default 32)
  - [x] Swappiness tunable integrated into workload profiles (Desktop=15, Server=30, Dev=10, Gaming=5)
  - [x] Page reclamation policy (Clock algorithm, triggered by low-memory threshold)
    - [x] `ReclaimablePage` tracking list with register/unregister API
    - [x] Clock (second-chance) algorithm: scans ACCESSED bit, gives second chance or evicts
    - [x] `try_reclaim(target)` integrated into frame allocator OOM path
    - [x] `register_reclaimable()` hooked into stack growth, demand paging, and swap-in paths
    - [x] Lock ordering: SWAP → RECLAIM → page table → frame allocator (deadlock-free)
  - [x] zswap/zram compressed swap (recommended for desktop)
    - [x] LZ4-like fast compression algorithm (compress.rs) with hash-table match finding
    - [x] Special 1-byte encoding for all-zero pages (BSS, stack)
    - [x] MemBackend transparently compresses/decompresses page data
    - [x] SlotData::Compressed vs ::Uncompressed storage per slot
    - [x] CompressionStats API: ratio_percent(), bytes_saved()
    - [x] Self-test: zero page (16K→1B), repeating (98% savings), sparse (>99% savings)
  - [x] Disk-backed swap via virtio-blk
    - [x] DiskBackend: reads/writes compressed page data to block device sectors
    - [x] SwapBackend enum: unifies Memory (zram) and Disk backends
    - [x] init_disk(): validates device capacity, configures sector layout
    - [x] Boot sequence: starts with zram, upgrades to disk when device available
    - [x] QEMU boot-test.sh: 16 MiB swap disk image, virtio-blk-pci device
    - [x] Self-test: write-read roundtrip (patterned data + zero page) on disk
- [x] Committed vs. lazy memory allocation modes
  - [x] Default: committed (immediate frame allocation, the design spec's mandate)
  - [x] MAP_LAZY flag (bit 6) for opt-in demand-paged allocation via SYS_MMAP
  - [x] Per-process VMA list in PCB for tracking lazy regions
  - [x] Page fault handler resolves user-space lazy faults: VMA lookup → frame alloc → zero → map → TLB flush
  - [x] SYS_MUNMAP cleans up VMA entries for lazy regions
- [x] Runtime-tunable memory parameters via sysctl-like interface
  - [x] `sysctl` module: flat registry of named parameters with ID, value, range, default
  - [x] 6 initial parameters: mm.max_stack_frames, mm.lazy_default, mm.oom_policy, mm.zero_on_alloc, sched.interactive_threshold, sched.interactive_boost
  - [x] SYS_SYSCTL_GET (60), SYS_SYSCTL_SET (61) syscalls
  - [x] mm.lazy_default wired to SYS_MMAP (system-wide lazy allocation default)
  - [x] Self-test: read/write/range-check/unknown-ID
- [x] Workload profiles (Desktop, Server, Development, Gaming) as presets for memory
  - [x] `MemoryProfilePreset` struct: per-profile mm.* parameter values
  - [x] `apply_memory_profile(profile_id)` sets max_stack_frames, lazy_default, oom_policy, zero_on_alloc
  - [x] `current_memory_profile()` detects active profile (returns None if manually tuned)
  - [x] `apply_system_profile(profile_id)` unified call setting both scheduler and memory parameters
  - [x] SYS_MM_SET_PROFILE (70), SYS_MM_GET_PROFILE (71), SYS_SYSTEM_SET_PROFILE (80) syscalls
  - [x] Self-test: all 4 profiles verified, invalid IDs rejected, manual tuning breaks detection

### 1.3 Scheduler
_Define scheduler trait interface first, implement one scheduler behind it._
- [x] Scheduler trait interface:
  - `pick_next_task(cpu) -> task`
  - `enqueue_task(task)`
  - `dequeue_task(task)`
  - `task_tick(task)` (timer interrupt)
  - `balance_load()` (periodic)
- [x] Priority round-robin scheduler (default):
  - [x] 32 or 64 priority levels, real-time levels at top
  - [x] Round-robin within each priority level
  - [x] Configurable time slices per level (shorter = higher priority)
  - [x] Per-CPU run queues (PerCpuScheduler, MAX_CPUS=16, cache-warm enqueue via last_cpu)
  - [x] Work stealing from longest queue when idle (steal half, prefer same NUMA node when SMP)
  - [x] Priority inheritance on mutex contention
    - [x] `inherited_priority: Option<u8>` on Task struct, `effective_priority()` considers it
    - [x] `sched::boost_priority()` / `sched::set_inherited_priority()` for PI donation/clear
    - [x] PI futex: `futex_lock_pi()` / `futex_unlock_pi()` with owner tracking + waiter queues
    - [x] Futex word format: bits 0-29 = owner TID, bit 30 = WAITERS flag
    - [x] Highest-priority waiter gets ownership on unlock
    - [x] Multi-lock priority recalculation on unlock (handles multiple PI locks per task)
    - [x] `SYS_FUTEX_LOCK_PI` (212), `SYS_FUTEX_UNLOCK_PI` (213) syscalls
    - [x] Self-test: high-prio task boosts low-prio lock holder, priority restored on unlock
  - [x] Interactive task detection (I/O-blocking tasks get small priority boost)
    - [x] Per-task EWMA burst tracking (fixed-point x8, α=1/8)
    - [x] Tasks with avg burst < 5 ticks (50ms) get +2 priority boost
    - [x] Boost applied on wake/yield/resume, decays with long bursts
  - [x] Runtime-tunable time slice durations
    - [x] `set_time_slice(level, ticks)`, `time_slice(level)`, `reconfigure_slices(base, increment)` on PriorityRoundRobin
    - [x] Public sched module API: `set_time_slice()`, `get_time_slice()`, `reconfigure_time_slices()`
    - [x] SYS_SCHED_SET_TIMESLICE (50), SYS_SCHED_GET_TIMESLICE (51), SYS_SCHED_RECONFIGURE (52)
    - [x] Self-test validating set/get/reconfigure + boundary conditions
- [x] Process/thread pause, resume, priority change while running
  - [x] sched::suspend/resume for task pause/unpause
  - [x] sched::set_priority for runtime priority change
  - [x] SYS_THREAD_SUSPEND (513), SYS_THREAD_RESUME (514), SYS_THREAD_SET_PRIORITY (515)
  - [x] Same-process authority enforcement
- [x] Workload profile presets for scheduler parameters
  - [x] `WorkloadProfile` enum: Desktop (0), Server (1), Development (2), Gaming (3)
  - [x] Each profile encodes (base, increment) for time slice formula
  - [x] `apply_profile()` on PriorityRoundRobin, `apply_workload_profile()` / `current_workload_profile()` on sched module
  - [x] SYS_SCHED_SET_PROFILE (53), SYS_SCHED_GET_PROFILE (54) syscalls
  - [x] Self-test: all 4 profiles, invalid IDs rejected, manual tuning detected
- [x] Kernel synchronization primitives:
  - [x] WaitQueue (fundamental blocking, 32 waiter slots)
  - [x] KMutex (sleeping mutex, adaptive spin + WaitQueue blocking, RAII guard)
  - [x] KRwLock (sleeping reader-writer lock, writer preference, concurrent readers)
  - [x] Semaphore (counting, AtomicI64 + WaitQueue)
  - [x] CondVar (condition variable, integrates with KMutex, wait_while/wait_until)
  - [x] Barrier (reusable multi-task rendezvous, generation counter)
  - [x] OnceEvent (one-shot latch, signal once + unblock all forever)
  - [x] KChannel (bounded MPMC typed channel, backpressure, close semantics)
- [x] Task supervisor (automatic restart of crashed kernel tasks)
  - [x] RestartPolicy: Always/OnFailure/Never, exponential backoff, max restarts
  - [x] Exit hook integration, deferred restart via ktimer → workqueue → spawn
- [x] Kernel trace buffer (ktrace)
  - [x] 512-entry lock-free ring buffer, ~20ns per event recording
  - [x] 12 categories, per-category filter, global enable/disable
  - [x] Trace points: task spawn, task exit
  - [x] Kshell 'trace' command for inspection
- [x] FPU/SSE context save/restore
  - [x] Eager fxsave64/fxrstor64 in switch_context assembly (matches modern Linux since 4.2)
  - [x] Per-task FpuState (512-byte aligned buffer, default FCW=0x037F, MXCSR=0x1F80)
  - [x] CR0/CR4 configuration on BSP (init_bsp) and APs (init_ap)
  - [x] Fixed latent AP bug: CR4.OSFXSR not set after INIT-SIPI (SSE would #UD)
  - [x] Self-test: XMM0 round-trip through save/restore verified
  - [x] Stress test: 4 tasks × 50 yields, unique XMM1 patterns, no cross-task leakage

### 1.4 IPC and syscalls
- [x] Syscall dispatch (many specialized syscalls, not few generic ones)
- [x] Versioned syscall tables
- [x] Channel IPC (Fuchsia-style, structured message-passing with capability transfer)
- [x] One-way pipes (byte streams)
- [x] Shared memory regions
- [x] Eventfd-like lightweight wake-up counters (kernel-managed integer, wait/wake)
- [x] IOCP-like completion port / unified wait:
  - [x] Register/unregister waitable objects with arbitrary user-data int
  - [x] Wait on: channels, pipes, eventfds, process exit, timers, semaphores, I/O completion (io_ring CQ readiness via WaitSource::IoCompletion)
- [x] io_uring-style submission queue (optional async path for batch I/O)
  - [x] Shared-memory SQ/CQ ring buffers with atomic head/tail pointers
  - [x] 18 opcodes: NOP, console write, channel send/recv, pipe read/write, FS read/write, file handle read/write/pread/pwrite, eventfd signal, semaphore signal, timeout, timeout_cancel, service_connect, sleep
  - [x] Completion port integration: io_ring notifies registered CP when CQEs are posted
  - [x] SYS_IO_RING_SETUP (260), SYS_IO_RING_ENTER (261), SYS_IO_RING_DESTROY (262)
  - [x] 5 self-tests: create/destroy, NOP submission, console write batch, file handle read/write, timeout + service connect
- [x] Futexes (for userspace synchronization without syscall in uncontended case)
  - [x] Timed futex wait: SYS_FUTEX_WAIT_TIMEOUT (214) — nanosecond-precision deadline for pthread_mutex_timedlock/condvar
- [x] Timeout variants for all blocking IPC:
  - [x] channel::recv_timeout (SYS_CHANNEL_RECV_TIMEOUT 205)
  - [x] pipe::read_timeout (SYS_PIPE_READ_TIMEOUT 226)
  - [x] semaphore::wait_timeout (SYS_SEM_WAIT_TIMEOUT 275)
  - [x] eventfd::read_timeout (SYS_EVENTFD_READ_TIMEOUT 245)
  - [x] pipe::write_timeout (SYS_PIPE_WRITE_TIMEOUT 227)
  - [x] eventfd::write_timeout (SYS_EVENTFD_WRITE_TIMEOUT 246)
  - [x] channel::send_timeout (SYS_CHANNEL_SEND_TIMEOUT 208)
  - [x] channel::send_blocking — blocks on full queue, wakes on consume
- [x] Capability transfer through channel messages (Fuchsia-style):
  - [x] SYS_CHANNEL_SEND_CAPS (206): move cap handles into message (sender loses access)
  - [x] SYS_CHANNEL_RECV_CAPS (207): extract caps into receiver's table (new handles)
  - [x] TransferredCap: detached entry in transit, MAX_CAPS_PER_MESSAGE = 64
  - [x] All-or-nothing send semantics (invalid handle → nothing sent)
- [x] Console I/O syscalls (SYS_CONSOLE_WRITE, SYS_CONSOLE_READ_CHAR — bootstrap console for early userspace)
- [x] Filesystem I/O syscalls (SYS_FS_READ_FILE through SYS_FS_STAT — stateless whole-file operations via VFS)
- [x] Timer syscalls (SYS_CLOCK_MONOTONIC, SYS_SLEEP — lock-free sleep queue, 10ms resolution)
- [x] Service registry (named service discovery + connection brokering):
  - [x] register(name) → listener handle, connect(name) → client channel
  - [x] accept / try_accept / accept_timeout for server-side connection pickup
  - [x] Unregister closes pending connections, wakes blocked acceptors
  - [x] SYS_SERVICE_REGISTER (280) through SYS_SERVICE_UNREGISTER (285)
- [x] Per-process namespace support (path isolation and remapping for sandboxing):
  - [x] Bind rules (remap path prefixes), hide rules (block path access)
  - [x] First-match-wins evaluation, root namespace fast passthrough
  - [x] Process attach/detach with refcount tracking
  - [x] SYS_NS_CREATE (290) through SYS_NS_QUERY (295)
  - [x] 6 self-tests: create/destroy, bind, hide, path boundary, clone, attach/detach

### 1.5 Capability / security model
- [x] Per-process capability table (unforgeable handles to kernel objects)
- [x] Capability delegation (parent passes subset to child, can't create new ones)
- [ ] User/group model with named capability groups
- [ ] File/directory capability tags (AND-composition between groups, OR within a group)
- [x] Resource limits as cgroup-like controls (set at launch, kernel-enforced)
  - [x] Per-process ResourceLimits struct (max_rss_frames, cpu_quota_pct, max_threads, max_handles)
  - [x] PID-indexed limits table with apply/query/update/remove API
  - [x] RSS limit enforcement wired through mm::accounting::try_charge
  - [x] CPU bandwidth throttling: per-task quota as % of core, 1-second period, park/unpark
  - [x] System load average (EWMA of runnable tasks, BSP-driven 1Hz sample)
  - [x] Per-CPU utilization tracking (total/idle tick counters, cpuinfo command)
- [x] Capability-gated syscalls
- [ ] "Request capability from user" dialog mechanism
- [ ] Enable Intel CET (shadow stack + indirect branch tracking) on supporting hardware
- [ ] Enable LLVM CFI as default for C/C++ compilation

### 1.6 Process management
- [x] ELF binary loader
- [x] Process creation / destruction
- [x] Thread creation / destruction
  - [x] SYS_THREAD_CREATE (510): userspace thread spawn with user-provided RIP/RSP
  - [x] SYS_THREAD_EXIT (511): thread exit with value, join support
  - [x] SYS_THREAD_JOIN (512): blocking wait for thread completion with exit value retrieval
- [x] fork equivalent (or better: posix_spawn-style that avoids fork's problems)
- [x] exec equivalent
- [x] Hardware exception → language-level exception (SEH-style, not Unix signals)
- [x] Structured shutdown via IPC message, not Unix signals
- [x] Process credential / capability management

---

## Phase 2: Basic Userspace

_Depends on: Phase 1 complete. Goal: boot to a shell prompt._

### 2.1 Driver framework
- [-] Userspace driver framework:
  - [x] MMIO mapping into driver process address space (SYS_MMAP + MAP_MMIO)
  - [x] Interrupt delivery from kernel to driver (IOAPIC + IRQ syscalls)
  - [x] Port I/O syscalls for legacy devices (SYS_PORT_READ/WRITE)
  - [x] DMA mapping setup syscalls (mm::dma — alloc/free, user mapping, constraint support)
  - [ ] Driver crash detection and automatic restart
- [ ] IOMMU setup and sandboxing (detect disabled IOMMU, prompt user)
- [ ] Ada/SPARK FFI bridge for kernel-space drivers
- [-] virtio drivers (disk, network, GPU) for VM development/testing
  - [x] virtio-blk driver (legacy PCI transport, synchronous sector I/O, interrupt-driven completion with polling fallback)
  - [x] virtio-net driver (legacy PCI transport, RX/TX queues, MAC read, interrupt acknowledgment)
  - [x] Shared PCI IRQ handling (level-triggered, ISR reads device status to deassert)

### 2.2 Essential drivers
- [-] Keyboard (PS/2 and USB HID)
  - [x] PS/2 keyboard driver (scan code set 1, IRQ 1, modifier tracking, ASCII echo)
- [x] Framebuffer / basic display (UEFI GOP framebuffer initially)
  - [x] 8x16 VGA bitmap font, framebuffer text console (160x50 @ 1280x800)
- [ ] Storage (NVMe, AHCI/SATA)
- [ ] USB host controller (xHCI)
- [ ] Network (Intel e1000/e1000e for VMs, basic realtek for real hardware)
- [x] Timer (HPET, APIC timer)
  - [x] Local APIC timer (calibrated via PIT, 100 Hz periodic, preemptive scheduling)
  - [x] HPET (High Precision Event Timer): ACPI table discovery, MMIO mapping, 100 MHz monotonic counter, ticks_to_ns conversion, self-test
  - [x] High-resolution timers (hrtimer): per-CPU sorted timer lists, nanosecond scheduling via HPET, repeating timers, cancel API, process_expired() from APIC ISR
  - [x] hrtimer IRQ safety: all CPU_TIMERS lock acquisitions wrapped in without_interrupts() to prevent ISR deadlock
  - [x] Scheduler integration: sleep_ns() for nanosecond-precision task sleep (hrtimer-backed, tick-based fallback for >100ms)
  - [x] Deferred wake queue: 32-slot lock-free retry mechanism for ISR-context try_wake failures, drained by schedule_inner + softirq + idle loop
  - [x] Deferred wake sentinel fix: use u64::MAX (not 0) as empty slot marker — task 0 wakes were silently dropped
  - [x] Nanosecond timeout variants for all blocking primitives (waitqueue, kmutex, semaphore, condvar, once_event, kchannel)
  - [x] ktrace timer tracepoints: TIMER_SCHEDULE, TIMER_FIRE, TIMER_CANCEL, TIMER_TICK_SHORT
- [x] RTC (real-time clock)

### 2.3 Filesystem
_Port ext4 first. Don't write a custom filesystem._
- [x] VFS (virtual filesystem) layer — FileSystem trait, mount table, path resolution
- [x] Port ext4 (primary filesystem, read-write)
  - [x] On-disk structure definitions (superblock, group descriptors, inodes, extents, directory entries)
  - [x] Superblock parser with feature flag validation and derived value computation
  - [x] Block group descriptor table reading
  - [x] Inode lookup (inode number → on-disk inode)
  - [x] Directory traversal (linear entries + htree hash-tree index for O(1) lookups on large directories)
  - [x] Extent tree walking (logical → physical block mapping, multi-level)
  - [x] Read-only FileSystem trait implementation (VFS integration, mount, probe, self-test)
  - [x] Block allocation (bitmap-based, goal-directed, contiguous-run)
  - [x] Inode allocation (bitmap-based, group-preference)
  - [x] File creation/deletion (write_file, remove via VFS)
  - [x] Directory entry insertion/removal (add_dir_entry, mkdir, rmdir)
  - [x] Journal (jbd2 write-ahead log: transaction begin/log/commit, circular replay)
  - [x] Block/inode reclamation on delete and overwrite (no resource leaks)
  - [x] Crash-safe overwrite (allocate new → update inode → free old)
  - [x] Rename/move (files and directories, cross-directory with ".." update)
  - [x] Rich metadata (permissions, ownership, timestamps, immutable/append-only flags)
  - [x] Symlink create/read (fast symlinks ≤60 bytes in inode, slow via data blocks), symlink-following path resolution with depth limit
  - [x] Efficient partial I/O (extent-aware read_at, in-place write_at via block lookup)
  - [x] Extended attributes (xattr): external block storage, namespace indices (user/trusted/security/system), get/set/remove/list
  - [x] Timestamp management (set_times with nanosecond-to-second conversion)
  - [x] Truncate optimization (zero-size fast path: free blocks + reset extent header)
  - [x] Directory entry cache (512-entry LRU, (dir_inode, name) → child_inode, avoids O(n) linear scans on repeated lookups)
- [x] FAT (USB drives, EFI System Partition — essential)
  - [x] Unified FAT16/FAT32 driver (auto-detect, BPB parsing, FAT chain, readdir, file read/write/delete, subdirectories, mkdir, rmdir)
  - [x] Long Filename (LFN/VFAT) support: read/write/delete with UCS-2↔UTF-8 conversion, basis ~N name generation, contiguous slot allocation, checksum validation
  - [x] Rich metadata: DOS timestamp parsing (create/write/access), RTC-stamped writes, attribute mapping (read-only→immutable, hidden, system)
  - [x] Tested with both FAT16 (4 MiB) and FAT32 (64 MiB) disk images
- [x] ISO 9660 (optical media, read-only, Joliet detection)
- [x] Filesystem infrastructure:
  - [x] Buffer cache (2048-sector LRU, 1 MiB, write-back, BTreeMap index for O(log n) sector lookup, O(1) free-slot allocation, sequential read-ahead with 8-sector prefetch)
  - [x] File handle system (open/close/read/write/seek/fstat/ftruncate/dup/handle_path, symlink-resolved at open time, lock-on-close auto-release, handle enumeration for /proc/fdinfo)
  - [x] Path resolution cache: FAT dcache (256-entry LRU) + VFS-level dcache (256-entry LRU with prefix invalidation, caches resolved paths to skip per-component lstat walk)
  - [x] Efficient partial I/O (FAT read_at/write_at/truncate override default read-all-rewrite-all)
  - [x] Filesystem change notification system (inotify equivalent, async watches with bounded queues)
  - [x] Recycle bin (per-filesystem /_TRASH with _INDEX metadata file, trash/list/restore/empty, recursive directory trash/purge)
  - [x] 30 filesystem syscalls (600-639): file handles, trash, watch/notify, journal, metadata/xattrs, symlinks
  - [x] Change journal (persistent across reboots, JSON-lines /_JOURNAL file, 1024-entry ring buffer, 3 syscalls)
  - [x] In-memory filesystem (memfs/ramfs) for /tmp and pseudo-FS foundation, mounted at /tmp during boot
  - [x] Multi-mount VFS with longest-prefix path-boundary matching, mount-point synthesis in readdir
  - [x] Procfs virtual filesystem at /proc (version, uptime, meminfo, cpuinfo, mounts, task stats, per-PID status, filesystems, cmdline, loadavg, vmstat, buddyinfo, swaps)
  - [x] Devfs virtual filesystem at /dev (null, zero, full, random, urandom, console, stdin, stdout, stderr, kmsg, uptime)
  - [x] Sysfs virtual filesystem at /sys (kernel info, sysctl params, PCI devices, buffer cache stats) with read/write for tunables
- [x] Filesystem features:
  - [x] Case-sensitive paths, forward slash separator (VFS is case-sensitive; FAT case-insensitive by nature)
  - [x] Path validation: reject null bytes, enforce 255-byte component limit, require absolute paths
  - [x] Journaling (via ext4 jbd2)
  - [x] File metadata: owner, group, permissions, created/modified/accessed (relatime), immutable flag, append-only flag, extended attributes (key-value, 255-byte key / 64 KiB value)
  - [x] Symbolic links: create/readlink/lstat, iterative resolution (depth 40), follow-last semantics, circular detection (TooManyLinks), 3 syscalls (637-639), VFS-level cross-mount symlink resolution
  - [-] File metadata: capabilities per file (needs security zone)
  - [x] Content hash (SHA-256 via Vfs::content_hash)
  - [x] Filesystem space query (statvfs: FsInfo struct, block/inode counts, FAT scan, ext4 superblock, memfs node count)
  - [x] Hard links (VFS link() with same-mount enforcement, ext4 impl with i_links_count, nlinks in FileMeta, comprehensive self-test)
  - [x] Advisory file locking (flock: per-path shared/exclusive locks, owner tracking, upgrade/downgrade, process cleanup, 3 syscalls)
  - [x] Filesystem sync (VFS sync/sync_path, ext4 driver flush, FAT cache flush, SYS_FS_SYNC syscall, kshell `sync` command)
  - [x] Syscalls: SYS_FS_LINK (607), SYS_FS_STATVFS (608), SYS_FS_FLOCK (609), SYS_FS_FUNLOCK (640), SYS_FS_SYNC (641)
  - [x] Kshell filesystem commands: stat, ln, cp, mv, chmod, chown, touch, tree, du, find, df, sync, mount, umount, wc, head, tail, hexdump
  - [x] VFS unmount with sync + sub-mount safety + advisory lock cleanup
  - [x] Vfs::copy() for cross-mount file copying
  - [x] Rich metadata for all filesystems (devfs: device perms, iso9660: read-only perms + volume stats, procfs: read-only perms + inode counts)
  - [x] Procfs /proc/cacheinfo (buffer cache hit/miss/writeback stats) and /proc/locks (advisory lock dump)
  - [x] fstat nlinks: file handles now report hard link count via metadata()
  - [x] Syscalls: SYS_FS_COPY (642), SYS_FS_APPEND (643), SYS_FS_FTRUNCATE (644), SYS_FS_DUP (645), SYS_FS_HANDLE_PATH (646)
  - [x] Cross-mount rename (copy+delete fallback for files across different filesystems)
  - [x] Recursive copy (Vfs::copy_recursive, depth-limited, preserves permissions)
  - [x] Recursive remove (Vfs::remove_recursive, depth-first, returns item count)
  - [x] Procfs /proc/fdinfo (open file handles with flags/offset/size/path) and /proc/diskstats (block device info + cache stats)
  - [x] Kshell commands: lsof, grep, cp -r, rm -r
  - [x] VFS-level path resolution cache (dcache): 256-entry LRU caching (normalized_path, follow_last) → resolved_path, prefix-based invalidation on remove/rmdir/rename/symlink, full invalidation on mount/unmount, stats in /proc/cacheinfo
  - [x] Paginated directory listing: FileSystem::readdir_at() trait method, Vfs::readdir_at() with submount injection, SYS_FS_READDIR_AT (647) syscall with serialized output buffer
  - [x] Temporary file creation: SYS_FS_TMPFILE (648) syscall, TSC-based unique naming
  - [x] Space pre-allocation: FileSystem::fallocate() trait method, Vfs::fallocate(), SYS_FS_FALLOCATE (649) syscall
  - [x] Sparse file support: SeekFrom::Data and SeekFrom::Hole variants, SYS_FS_SEEK_DATA (650) and SYS_FS_SEEK_HOLE (651) syscalls (non-sparse default: data=offset, hole=EOF)
  - [x] ext4 directory entry cache: 512-entry LRU keyed by (dir_inode, name) → child_inode, avoids linear O(n) directory scans on repeated lookups, invalidation on add/remove
  - [x] ext4 htree (hash-tree) directory index: half_md4/TEA/legacy hash functions, dx_root/dx_node parsing, binary search, O(1) amortized lookups for INDEX directories; write-side: hash-aware insertion finds correct leaf block via tree probe, leaf splitting distributes entries by hash between old/new blocks and updates dx_root index, extent tree extension for new leaf blocks
  - [x] ext4 extent range cache: 256-entry LRU keyed by (inode, logical_block_range) → physical_block, interior-mutable via spin::Mutex for use through &self references, invalidation on all write paths (write_file, truncate, remove, rmdir, rename), stats in debug_stats
  - [x] ext4 inode cache: 128-entry LRU keyed by inode_nr → Ext4Inode, interior-mutable via spin::Mutex, read_inode() cache-first with disk fallback, write_inode() updates cache after write, stats in debug_stats
  - [x] ext4 journal recovery on mount: checks RECOVER incompat flag, reads journal inode, resolves extent tree to physical blocks, replays committed transactions, clears RECOVER flag; leaf extent collector with binary search for journal block mapping
  - [x] ext4 indirect block mapping (ext2/ext3 compat): read-only support for classic 12-direct + single/double/triple indirect block pointers, lookup_physical_block dispatches by EXTENTS flag, sparse hole handling, full-file and range reads
  - [x] Procfs expansion: /proc/interrupts (APIC timer, ISR latency, IRQ state), /proc/devices (PCI bus scan), /proc/net (interface config snapshot)
  - [x] Kshell commands: lsp (paginated ls), cmp/diff (byte-by-byte file compare), fallocate (space reservation with K/M/G suffixes)
  - [x] Kshell commands: sort, uniq, tee, truncate, sha256 (text processing and file ops)
  - [x] Kshell commands: readlink, symlink, xattr, basename, dirname, realpath, pwd, id, mktemp (path utilities)
  - [x] Kshell commands: sysctl (list/get/set kernel params), hostname (get/set), dd (block copy), free (memory summary), lsblk (block devices), glob (pattern expansion)
  - [x] Glob pattern matching: *, ?, [abc], [a-z], [!abc], \\ escape, case-insensitive mode; VFS glob_match() for filename matching, Vfs::glob() for path-level expansion
  - [x] Procfs expansion: /proc/vmstat (frames, swap, zram, kswapd, OOM), /proc/buddyinfo (buddy allocator per-order), /proc/swaps (swap devices with priority)
  - [x] Sysctl name-based API: list_all(), find_by_name(), set_by_name() for filesystem and shell access
  - [x] Self-tests: recursive copy/remove (3-level directory tree), cross-mount rename (memfs↔ext4), paginated readdir_at (page boundary, overlap, tail, past-end), VFS dcache (hit verification, invalidation, prefix matching), htree hash functions (determinism, divergence, scan_leaf_block), glob matching (wildcards, char classes, ranges, negation, escaping, edge cases), sysfs (directory layout, read/write, permissions)
  - [x] Kshell output redirection (`> file`, `>> file`) and piping (`cmd1 | cmd2`): capture buffer, pipe-input variants for sort/uniq/grep/head/tail/wc/cat/nl/rev
  - [x] Kshell commands: source (script execution with #comments, 8-level depth), seq, nl, rev, sleep, printenv, true/false
  - [x] Kshell command history: 64-entry ring buffer, Up/Down arrow browsing, duplicate suppression, saved live-line restoration
  - [x] Kshell cursor-aware line editing: Left/Right arrow, Home/End, insert/delete at cursor, readline shortcuts (Ctrl+A/E/C/K/U/W/L), keyboard echo control
  - [x] Kshell tab completion: command names (80+ built-in commands) and file paths (VFS directory listing), longest-common-prefix for multi-candidate, inline insertion
  - [x] Kshell relative path support: working directory (cd), resolve_path() for all commands, all old-style path resolution patterns migrated
  - [x] Kshell environment variables: BTreeMap storage, $VAR/${VAR} expansion, export/unset commands, default variables (PWD, HOME, SHELL, USER, PATH), PWD auto-sync
  - [x] Kshell aliases: alias/unalias commands, first-word expansion with loop detection, strip_quotes for alias values
  - [x] Kshell command chaining: ; (sequential), && (on success), || (on failure), exit status tracking ($?), test command (POSIX-compatible file/string/integer tests)
  - [x] CRC32C (Castagnoli) implementation: 256-entry lookup table (compile-time), crc32c/crc32c_seed/crc32c_raw API, self-test with 4 vectors
  - [x] ext4 metadata checksum validation: superblock CRC32C on mount, group descriptor checksums (all groups validated), inode checksums (validated on every read), CorruptedData error on mismatch
  - [x] ext4 write-path checksums: superblock, group descriptor, and inode checksums computed and embedded on all write operations when metadata_csum is enabled
  - [x] Kshell if/then/elif/else/fi: stack-based control flow (16 nesting levels), condition evaluation via exit status
  - [x] Kshell while/do/done: line-buffering loop body, nested while tracking, 1000-iteration safety limit
  - [x] Kshell arithmetic: $((expr)) expansion and expr command, recursive-descent parser (+ - * / % parentheses, i64 wrapping)
  - [x] ext4 inline data: read support for small files/dirs with INLINE_DATA flag (up to 60 bytes in i_block[]), SUPPORTED_INCOMPAT updated for INLINE_DATA and CSUM_SEED
  - [x] Kshell for/in/do/done loops: unified LoopCollector for while+for, variable expansion in word lists, quote-aware word splitting, 1000-word safety limit
  - [x] Kshell user-defined functions: name() { body }, function name { body }, positional params ($1-$9, $#, $@), return, declare -f, unset -f, 32-level recursion limit
  - [x] Kshell case/esac: glob-style pattern matching (*, ?, [abc]), pipe-separated alternatives, nested case/esac, multi-line clauses
  - [x] ext4 unwritten extents: read paths return zeros for uninitialized (pre-allocated) extents instead of reading actual block data
  - [x] Kshell $(command) substitution: capture command output inline, recursive capture support, POSIX trailing-newline stripping
  - [x] Kshell break/continue: loop control for while and for loops, interacts correctly with nested function return
  - [x] Kshell read command: interactive keyboard input into variables, -p prompt flag, backspace editing, Ctrl+C cancel
  - [x] Kshell shift command: discard positional params, which/typeof for command type inspection, expanded tab completion
  - [x] Kshell printf: formatted output (%s %d %u %x %X %o %c, zero-padding, width, escape sequences)
  - [x] Kshell ${} parameter expansion: ${#VAR} length, ${VAR:-default}, ${VAR:+alt}, ${VAR:=default}, ${VAR:?msg}, ${VAR%pat}, ${VAR%%pat}, ${VAR#pat}, ${VAR##pat}
  - [x] ext4 directory block checksums: per-block CRC32C validation in read_dir_entries and htree leaf scan, Ext4DirEntryTail struct, stamp_dirent_checksum for write paths
  - [x] ext4 extent block checksums: CRC32C validation of non-root extent tree blocks via Ext4ExtentTail, inode_csum_seed(), threaded inode_nr through all read/free paths, validation on all 5 recursive extent tree walkers
  - [x] ext4 write-path directory checksums: stamp_dir_data_checksums on add/remove/rename, init_dirent_tail for new blocks, find_dir_insert_point skips dirent tail to prevent checksum corruption
  - [x] Kshell here-documents: <<DELIM (collect+expand), <<-DELIM (tab-strip), <<'DELIM' (literal), suffix pipes/redirects, continuation prompt, source-aware, history-excluded
  - [x] Kshell array variables: arr=(words), ${arr[N]}, ${arr[@]}, ${#arr[@]}, arr[N]=val, arr+=(more), unset arr/arr[N], declare -a, type recognition
  - [x] Kshell local variable scoping: proper save/restore in functions via LOCAL_VARS stack, `local VAR=VALUE` saves and restores on function return
  - [x] Kshell string expansions: ${VAR:N:L} substring, ${VAR/pat/rep} replace first, ${VAR//pat/rep} replace all, ${VAR^}/${VAR^^} uppercase, ${VAR,}/${VAR,,} lowercase
  - [x] Kshell input redirection: `cmd < file` reads file as piped input, combines with output redirection
  - [x] Kshell shell options: `set -e` (errexit: abort scripts/functions on failure), `set -x` (xtrace: print commands before execution), `set +e`/`+x` to disable
  - [x] Kshell test command extensions: -L/-h (symlink), -r/-w/-x (permissions), -v (variable set), < > (string comparison)
  - [x] Kshell read -a: split input into array variables
  - [x] Kshell multi-pipe chains: `cmd1 | cmd2 | cmd3 | ... | cmdN` (was single-pipe only)
  - [x] Kshell tilde expansion: `~` and `~/path` expand to $HOME
  - [x] Kshell eval, mapfile/readarray, readonly, let commands
  - [x] Kshell C-style for loops: `for ((i=0; i<10; i=i+1)); do ... done` with enhanced arithmetic (comparisons, logical ops, variable names)
  - [x] Kshell brace expansion: `{a,b,c}` alternatives, `{1..10}` and `{1..10..2}` numeric ranges
  - [x] Kshell here-strings: `cmd <<< word`, `(( expr ))` arithmetic command, inline `VAR=value command`, bare `VAR=value` assignment
  - [x] Kshell until loops, trap handlers (EXIT/ERR/INT), command builtin (bypass aliases/functions)
  - [x] Kshell text processing: cut (-d/-f/-c), tr (translate/delete with ranges), tac (reverse cat), fold (-w), paste, yes, xargs (-n)
  - [x] Kshell echo -n/-e (no newline, escape sequences), select menus (interactive numbered choice)
  - [x] VFS dcache expansion: 256→1024 entries, negative cache entries (DcacheLookup enum, insert_negative, NegativeHit short-circuits NotFound, invalidate_negative_prefix on creation ops)
  - [x] ext4 journal revoke block support: two-pass recovery (scan revokes + replay with skip), JournalRevokeHeader, BTreeMap<u64,u32> revoke table, 64-bit block number support
  - [x] ext4 add_dir_entry fix: new-block path now properly rebuilds extent tree (was orphaning allocated blocks), insert path uses write_to_existing_blocks (avoids block leak)
  - [x] ext4 block leak fixes: rename ".." update and remove_dir_entry use write_to_existing_blocks instead of write_file_data (prevents O(dir_blocks) leak per operation)
  - [x] ext4 fallocate: real block pre-allocation with UNWRITTEN extents (empty files only; reads return zeros, blocks reserved on disk)
  - [x] VFS access() check: POSIX-style access(path, mode) with F_OK/R_OK/W_OK/X_OK, is_readable(), is_writable() convenience helpers, immutable-file check, self-test
  - [x] Kshell `uname` command: POSIX-compatible flags (-s/-n/-r/-v/-m/-o/-a), combined flags, RTC-based version date
  - [x] ext4 efficient append (extend_file_data): patches last partial block in place, allocates adjacent blocks to extend existing extent or adds new extent entry (depth-0 trees), avoids O(file_size) read-modify-write
  - [x] ext4 mixed write_at optimization: writes crossing EOF split into in-place write + extend_file_data, avoiding full file read for overwrite-and-append pattern
  - [x] Kshell `ls -l` long format: type + rwxrwxrwx permissions, link count, uid, gid, size (with -h human readable), modification time, -a for hidden files
  - [x] Procfs `/proc/config`: kernel build configuration (arch, page size, max CPUs, enabled subsystems, limits)
  - [x] Kshell `file` command: file type identification using lstat (directory, symlink with target, regular file with extension-based type hints, ~40 extensions)
  - [x] ext4 journal byte order fix: all jbd2 fields now correctly read/written as big-endian (network byte order) per spec, compatible with Linux-formatted ext4 images
  - [x] Procfs `/proc/fsstats`: per-filesystem debug statistics, df -v verbose mode
  - [x] Trash auto-prune: automatically delete oldest trash items when disk usage exceeds 90%, runs after each trash() operation
  - [x] Kshell `trash` command: trash FILE, --list, --restore, --empty, --purge, --prune
  - [x] Notify MetadataChanged events: VFS metadata operations (set_attributes, set_owner, set_permissions, set/remove_xattr) now emit MetadataChanged instead of Modified
  - [x] Kshell help text: added 20+ missing commands (dmesg, file, printf, trash, cut, tr, tac, fold, paste, yes, xargs, etc.) and expanded control flow section
  - [x] ext4 inline xattr support: reads xattrs from both inode body (inline) and external block, Linux compatibility for security.selinux and other inline attrs
  - [x] ext4 creation time (i_crtime): metadata() now reads creation time from extra inode fields
  - [x] Kshell timestamp formatting: stat and ls -l show YYYY-MM-DD HH:MM:SS via civil_from_days algorithm instead of raw epoch seconds
  - [x] Kshell diff command: line-level unified diff with LCS algorithm, 3-line context, hunk headers, 2000-line cap (separate from byte-level cmp)
  - [x] ext4 depth>0 extent tree write support: write_to_existing_blocks, extend_file_data, and last_extent_end now handle multi-level extent trees (leaf block read-modify-write with checksum stamping)
  - [x] Kshell watch command: real-time filesystem change monitoring via notify system, -r recursive flag, event display (CREATE/DELETE/MODIFY/RENAME/META)
  - [x] ext4 48-bit block count: i_blocks_lo + i_osd2[0..2] high bits, supports files >2TB; FileMeta.blocks field; stat shows block count
  - [x] ext4 i_file_acl_high offset fix: was reading/writing i_osd2[4..6] (i_uid_high) instead of [2..4] (i_file_acl_high), corrupting UIDs on xattr-heavy filesystems
  - [x] ext4 HUGE_FILE inode flag: inode_block_sectors() converts fs-block units to 512-byte sectors when EXT4_HUGE_FILE_FL (0x40000) is set; set_inode_blocks_48 clears flag (always stores sectors)
  - [x] ext4 xattr block checksums: CRC32C validation on read, stamping on write (was the only metadata type missing checksum support); uses csum_seed + block_nr as seed (shared blocks)
  - [x] ext4 i_file_acl_high offset fix (xattr free path): write_xattr_block cleared i_osd2[4..5] (i_uid_high) instead of [2..3] (i_file_acl_high) when freeing xattr blocks
  - [x] ext4 bitmap checksums: block/inode bitmap CRC32C validation on read and stamping on write via group descriptor bg_*_bitmap_csum_lo/hi fields; 16-bit or 32-bit depending on desc_size
  - [x] FAT set_attributes: VFS FileAttr → FAT attribute byte mapping (IMMUTABLE↔READ_ONLY, HIDDEN, SYSTEM); set_permissions/set_owner return NotSupported (no Unix model)
  - [x] Kshell `cal` command: monthly calendar display with Tomohiko Sakamoto day-of-week, today highlight, optional month/year args
  - [x] Kshell `ls -l` enhancements: "total N" blocks header, symlink target display (" -> target"), -R recursive, -S size-sort, -t time-sort, -r reverse-sort
  - [x] Kshell `chattr`/`lsattr` commands: set/clear/display file attributes (immutable, append-only, hidden, system) on ext4 and FAT
  - [x] Kshell `stat` enhanced output: symbolic permission display, human-readable attributes, filesystem type from statvfs, extended attribute listing
  - [x] FAT fallocate: cluster pre-allocation without changing file size, zero-fill new clusters, extends existing chains
  - [x] FAT FSInfo sector: read/write FAT32 FSInfo for cached free cluster count and next-free hint; O(1) statvfs after first call; alloc_cluster starts from hint instead of scanning from cluster 2
  - [x] Kshell `grep` enhancements: -r recursive directory search (depth 16), -v invert, -c count-only, -w whole-word, -l files-only, -I case-sensitive, multi-file output with filename prefix
  - [x] VFS mount options: MountOptions struct (ro/noatime/noexec/nosuid), mount_with_options(), remount(), read-only enforcement on all write-path VFS methods, ReadOnlyFilesystem error
  - [x] Kshell `mount -o` options: mount -o ro,noatime, mount -o remount,ro, bare mount shows options column, /proc/mounts shows Linux-compatible format
  - [x] Kshell `du` enhancements: -s summary-only, -dN max depth flags
  - [x] Kshell `date` with strftime-like +FORMAT (%Y %m %d %H %M %S %a %A %b %B %j %u %Z %F %T %D %s), `time CMD` for command timing with HPET nanosecond clock
  - [x] Kshell utilities: `strings [-n N]` (extract ASCII from binary files), `nproc` (CPU count), `column [-t]` (tabular formatting with pipe support), `cpuinfo` (per-CPU APIC IDs)
  - [x] Kshell `comm [-123]` (compare sorted files, 3-column output), `od [-A radix] [-t type] [-N count]` (octal/hex/decimal dump with POSIX-style options)
  - [x] Buffer cache O(1) dirty/valid counting: pre-maintained counters instead of O(n) scan in stats(), maintained on all state transitions
  - [x] FAT clean-shutdown bit: read/clear on mount (warn if unclean), set on sync; FAT16 bit 15, FAT32 bit 27 of cluster 1 entry; per-spec dirty tracking across both FAT copies
  - [x] FAT volume label write: set_volume_label() updates BPB boot sector + root directory ATTR_VOLUME_ID entry; kshell `label PATH [NAME]` command; FileSystem trait method with default NotSupported
  - [x] FAT mkfs: format block devices as FAT16 (≤32 MiB) or FAT32 with BPB, dual FATs, FSInfo, backup boot sector, root directory with volume label
  - [x] FAT fsck: 5-phase consistency checker — clean-shutdown bit, FAT copy comparison, directory tree walk with cluster ownership map, cross-link and lost cluster detection, file size vs chain validation; -a flag for repair (copies FAT1→FAT2, frees lost clusters, sets clean bit)
  - [x] Kshell `dd` enhancements: skip=N (input offset), seek=N (output offset via write_at), HPET-timed throughput display (KiB/s)
  - [x] Kshell `flock` command: query/acquire/release advisory file locks (-s shared, -x exclusive, -u unlock, -q query)
  - [x] Kshell `split` command: split files by line count (-l N, default 1000) or byte size (-b SIZE with K/M/G); xaa/xab/... output naming
  - [x] Procfs `/proc/bcache`: buffer cache statistics (hit rate, dirty count, utilization, read-ahead, expired flush counts)
  - [x] Buffer cache age-based writeback: dirty_since_ns timestamps, flush_expired() writes back entries >5s old, expired_flushes counter; wired into `sync` command
  - [x] FAT LFN operation fix: rename, rmdir, mkdir, fallocate, write_at, truncate all use resolve_path() instead of to_83_name() (which silently failed for any long filename)
  - [x] FAT self-test expansion: LFN operation tests (mkdir/rmdir/rename/write_at/truncate/fallocate with long names), fsck consistency check after test suite
  - [x] Kshell `tar` command: USTAR archive create (-cf), extract (-xf [-C dir]), list (-tf), verbose (-v), recursive directories, symlink support, proper padding and checksum
  - [x] Kshell `crc32` command: CRC32C file checksum using hardware-compatible Castagnoli polynomial
  - [x] Kshell `base64` command: RFC 4648 Base64 encode/decode with 76-char line wrap
  - [x] Kshell `checksum` command: unified checksum utility supporting CRC32C and SHA-256
  - [x] Kshell `wipe` command: secure delete (zero-fill file contents, sync to disk, then remove)
  - [x] Buffer cache adaptive read-ahead: window starts at 4 sectors, doubles per sequential batch (up to 128), resets on random access (based on Linux mm/readahead.c)
  - [x] Kshell `sed` command: stream editor with s/old/new/[g] substitution, /pattern/d delete, /pattern/p print, -i in-place, -n suppress, pipe input support
  - [x] Tar archive round-trip self-test: build archive with tar_build_header, write to disk, read back, parse with tar_parse_header, verify entry count
  - [x] Kshell `awk` command: pattern-action text processing with field splitting ($1..$N, $NF), -F separator, /pattern/ matching, BEGIN/END blocks, NR/NF variables, print expressions, pipe input
  - [x] ext4 POSIX rename fix: replace existing destination files instead of returning AlreadyExists; properly frees inode data/xattr/number when link count reaches zero
  - [x] FAT POSIX rename fix: replace existing destination files; frees cluster chain and LFN entries of overwritten file
  - [x] Notify event coalescing: duplicate events (same type + path) suppressed in watch queue; self-test for coalescing and overflow behavior
  - [x] Journal O(1) ring buffer: replaced Vec with VecDeque for O(1) oldest-entry eviction instead of O(n) Vec::remove(0)
  - [x] Kshell `journal` command: view filesystem change journal entries with -n N, --all, --since SEQ, --stats, --flush
  - [x] Kshell `file` magic bytes detection: reads first 512 bytes and matches 30+ binary signatures (ELF, PE, PNG, JPEG, PDF, ZIP, gzip, tar, WASM, SQLite, audio/video formats, shebang scripts, Unicode BOMs); falls back to text-vs-binary heuristic
  - [x] DEFLATE/gzip decompression (fs::compress): RFC 1951 inflate (stored, fixed Huffman, dynamic Huffman blocks), RFC 1952 gzip wrapper with CRC-32 ISO verification
  - [x] Kshell `gunzip`/`gzip` command: decompress .gz files with -t test, -l list sizes, -o explicit output; auto-strips .gz extension
  - [x] Tar .tar.gz transparency: auto-detects gzip magic bytes and decompresses before extracting/listing
  - [x] Kshell `unzip` command: ZIP archive listing and extraction with stored and deflated entries, CRC-32 verification, automatic directory creation
  - [x] DEFLATE compression (fs::compress::deflate): LZ77 with hash-chain + fixed Huffman encoding; gzip() wrapper creates RFC 1952 streams; round-trip self-tests
  - [x] Kshell `gzip` compression mode: auto-detects non-gzip input and compresses; tar -czf creates gzip-compressed archives
  - [x] Buffer cache runtime-tunable parameters: readahead_max, readahead_initial, dirty_expire_secs backed by AtomicU32/AtomicU64, sysctl callback dispatch (notify_subsystem) propagates changes to cache atomics, seconds→nanoseconds conversion, self-tests for full sysctl→cache propagation path
  - [x] Kshell `zip` command: create ZIP archives with local file headers, central directory, EOCD record; deflate compression (method 8) with stored fallback (method 0); recursive directory inclusion; `-0` store-only mode
  - [x] ext4 fsck: four-phase read-only consistency checker — superblock validation, group descriptor free counts vs actual bitmaps (block + inode), inode scan with type classification, directory tree walk with link count verification; `fsck` auto-detects ext4 vs FAT; verbose mode
  - [x] DEFLATE dynamic Huffman encoding: LZ77 tokenizer, optimal Huffman tree construction (build_code_lengths, build_canonical_codes), RLE code length encoding (symbols 16/17/18), tries both fixed and dynamic and picks smaller output; 20-40% improvement on text data
  - [x] Bzip2 decompression (fs::bzip2): MSB-first bit reader, Huffman decode with grouped selectors, MTF encode/decode, BWT inverse (O(n) LF-mapping), two-layer RLE, unreflected CRC-32, multi-block streams; `bunzip2`/`bzcat` commands, tar .tar.bz2 auto-detection, `file` magic byte recognition
  - [x] XZ/LZMA2 decompression (fs::xz): XZ container parser (header/blocks/index/footer, CRC-64 ECMA-182), LZMA2 chunk decoder (uncompressed/LZMA with state/props/full reset), core LZMA decoder (range coder, adaptive probabilities, 12-state Markov model, literal/match/rep/shortrep, slot-based distances with context/direct/align bits); `unxz`/`xzcat` commands, tar .tar.xz auto-detection
  - [x] Zstandard decompression (fs::zstd): RFC 8478 frame/block parser (raw/RLE/compressed), FSE (tANS) decoding tables with predefined/RLE/compressed/repeat modes, Huffman literal decoding (single + 4-stream), sequence decoder (literal-length/match-length/offset triplets with repeat offset tracking), xxHash-64 content checksums; `unzstd`/`zstdcat` commands, tar .tar.zst auto-detection
  - [x] Zstandard compression (fs::zstd): store mode (raw/RLE blocks), LZ77 mode with hash-chain matching (MIN_MATCH=4, WINDOW_SIZE=64K) and proper FSE state-machine encoding using predefined tables (backward sequence processing to resolve state transitions); `zstd` command with `-s` store flag; round-trip verified (repetitive data: 11%, text: 76%)
  - [x] ext4 extent tree node splitting: depth-0→depth-1 promotion (4→~340 extents per leaf), add_leaf_to_tree for depth-1 leaf splitting (up to 4 leaves = ~1360 extents), fix dormant ei_leaf_hi<<16→<<32 bug in 2 index traversal sites; proper error cleanup and metadata checksum stamping
- [ ] Later: NTFS read support, Btrfs/ZFS CoW support, F2FS

### 2.4 Networking stack (userspace)
- [-] TCP/IP stack (kernel-resident prototype, will move to userspace)
  - [x] Ethernet frame parsing/building
  - [x] ARP request/reply with cache
  - [x] IPv4 packet parsing/building with checksum
  - [x] UDP datagram send/receive with socket layer
  - [x] DHCP client (auto-configure IP/mask/gateway/DNS at boot)
  - [x] TCP client (3-way handshake, data transfer, FIN teardown)
  - [x] DNS resolver (A record queries via UDP)
  - [ ] Move to userspace service
- [x] Sockets API (not file descriptors — dedicated socket handles)
  - [x] TCP syscalls: connect, send, recv, close (SYS_TCP_CONNECT through SYS_TCP_CLOSE)
  - [x] UDP syscalls: bind, send, recv, close (SYS_UDP_BIND through SYS_UDP_CLOSE)
  - [x] DNS resolution syscall (SYS_DNS_RESOLVE)
- [ ] Firewall (basic packet filtering)
- [ ] Later: WiFi (requires wireless driver + wpa_supplicant port)

### 2.5 POSIX compatibility layer
- [ ] Enough of POSIX libc for: gcc, coreutils, bash, Python (CPython)
- [ ] Translate POSIX calls to native syscalls
- [ ] /proc, /sys equivalents (for programs that need them)
- [ ] POSIX signals → translate to native IPC messages

### 2.6 Init / service manager
- [-] PID 1 init process
  - [x] Minimal userspace init binary (ring 3, SYSCALL-based I/O, embedded in kernel ELF)
  - [x] Interactive shell: console read/write, echo command, help, exit
  - [x] Filesystem shell commands: ls, cat, write, stat, mkdir, rmdir, rm
  - [x] System info commands: pid, uptime
  - [x] Userspace pointer validation on all syscall handlers
  - [x] Service spawn/management (init reads ELF from VFS, spawns child, waits for exit)
- [x] Dependency-based parallel service startup
- [ ] Socket activation
- [x] Automatic crash restart with backoff
- [ ] Resource limits per service (cgroup-equivalent)
- [x] JSON-lines structured logging (text-based, not binary)
- [x] "Service ready" notification API
- [x] Startup app list (simple serial list, separate from service manager)
  - [ ] Disk-idle heuristic for "app is loaded, start next one" (2-3 sec timeout)
  - [x] Explicit readiness notification API

### 2.7 Shell and basic userspace tools
- [ ] Port bash (POSIX compatibility)
- [ ] Port or adopt Nushell as default shell (Rust, structured data piping)
- [ ] Port coreutils (ls, cp, mv, rm, mkdir, cat, etc.)
- [ ] Port rsync (replaces robocopy need)
- [ ] Port curl
- [ ] Port ssh/sshd
- [ ] Build custom grep (Rust, with unique features from Python grep)
- [ ] Port find
- [ ] Terminal emulator (basic, serial/framebuffer):
  - [ ] Persistent searchable history, tab completion
  - [ ] Unicode and ANSI support
  - [ ] Configurable colors and font
  - [ ] tmux-like session detach/reattach
  - [ ] Word wrap option, find in backscroll (Ctrl+F)

### 2.8 I/O scheduler
- [x] BFQ-style I/O scheduler:
  - [x] Realtime priority (audio/video)
  - [x] Best-effort with priority levels
  - [x] Idle priority (background indexing, backup, dedup)
  - [x] Per-process queues with elevator (C-SCAN) sector ordering
  - [x] Budget-based fairness with two-pass rotation
  - [x] Adjacent request merging
  - [x] Capability-gated realtime I/O priority

---

## Phase 3: Graphics and GUI

_Depends on: Phase 2 (drivers, filesystem, basic userspace). Goal: boot to a graphical desktop._

### 3.1 GPU drivers
- [ ] Port AMDGPU driver (open source, well-documented — first priority)
- [ ] Port Intel i915/xe driver (integrated graphics — covers most laptops)
- [ ] NVIDIA: defer until open-source driver matures, or use Linux compat layer later

### 3.2 Graphics stack
- [ ] DRM/KMS equivalent (kernel mode setting, GPU memory management)
- [ ] Vulkan loader and basic GPU command submission
- [ ] OpenGL via Mesa (port Mesa's Vulkan and OpenGL drivers)
- [ ] 2D drawing library: Vello (Rust-native, GPU compute shaders) + HarfBuzz via FFI for complex text shaping

### 3.3 Compositor
- [ ] Wayland-inspired compositor (userspace):
  - [ ] Window compositing with GPU acceleration
  - [ ] DMA-BUF buffer sharing between apps and compositor
  - [ ] Fullscreen bypass (direct scanout for games)
  - [ ] Native remote desktop streaming (compositor knows draw commands — most efficient option)
  - [ ] Video-encoded capture fallback (H.264/VP9 for games/video)

### 3.4 Window manager / desktop shell
- [ ] Desktop with draggable icons (snap-to-grid or free placement)
- [ ] Taskbar:
  - [ ] Pinned apps on left, running apps on right, divider between sections
  - [ ] Drag to reorder, drag to/from desktop and start menu
  - [ ] Optional app name alongside icon
  - [ ] Aero-style blurry transparency
- [ ] System tray (drag icons in/out, start-in-tray option)
- [ ] Notification pane (per-app disable option)
- [ ] Start menu (app tree, settings, terminal, power options, search)
- [ ] Ctrl+R run dialog (completion, recent commands)
- [ ] System tray icons: clock, wifi, volume, battery, etc.
- [ ] Sound mixer (per-app volume, show currently-playing apps first)
- [ ] Light / dark / custom theme support
- [ ] Theme color API for applications
- [ ] Multi-monitor support (deferred but planned)

### 3.5 GUI toolkit / widget API
- [ ] Layout engine (Flexbox/Grid-based, not CSS — native implementation)
- [ ] Styling system (subset of CSS properties without cascade complexity)
- [ ] Signals and slots (Rust channels or callbacks)
- [ ] Core widgets:
  - [ ] Buttons (text, graphic), labels, menus
  - [ ] Input fields (single/multiline, placeholder text, word wrap option)
  - [ ] Checkboxes, tristate checkboxes
  - [ ] Radio buttons (grouped, with deselect option)
  - [ ] Treeview, tristate checkbox treeview
  - [ ] Tabs view, grid view
  - [ ] Scroll bars (auto-hide), tooltips
  - [ ] Color picker
  - [ ] Modal and non-modal dialogs, alert popups
- [ ] Text views:
  - [ ] Simple text (plain text, ANSI colors, single font)
  - [ ] Rich text (fonts, sizes, colors, inline images — NOT HTML)
  - [ ] Scroll-to-bottom / stay-at-bottom when new text added
- [ ] Advanced features:
  - [ ] Clipboard (multi-format: text, HTML, image, structured data, history)
  - [ ] Drag-and-drop (OLE-style multi-format data transfer)
  - [ ] File picker / save dialog (reuses file explorer component)
  - [ ] DPI/scaling awareness, automatic image scaling
  - [ ] Enable/disable controls with optional reason tooltip
  - [ ] SVG rendering support
  - [ ] Context menu extension API (capability-gated, lazy-loading, 200ms timeout)
- [ ] Credential manager service (factotum-like):
  - [ ] Central credential storage, apps never see raw passwords
  - [ ] API for username/password fields with autofill
  - [ ] User identity verification with debounce

### 3.6 Audio
- [ ] Audio driver framework
- [ ] Audio mixing (per-app volume control)
- [ ] System notification sounds
- [ ] Sound history (which apps played/are playing sound)

### 3.7 File type associations
- [ ] Extension → default app mapping
- [ ] Per-app icons per extension
- [ ] Easily discoverable UI to change associations
- [ ] Fallback to previous app when handler is uninstalled
- [ ] File extensions: .nx (executable), .dso (shared library), .slib (static library)

---

## Phase 4: Applications

_Depends on: Phase 3 (GUI toolkit and desktop shell). Goal: usable daily-driver desktop._

### 4.1 Core applications
- [ ] File explorer:
  - [ ] Path bar with autocomplete
  - [ ] Thumbnails (images, video, PDF)
  - [ ] Detail columns (union of relevant columns per file type in folder)
  - [ ] Custom columns per file type, app-extensible columns
  - [ ] Drop zones for drag-and-drop (empty space = this dir, folder = into folder)
  - [ ] Atomic copy/move/delete with undo, resume on interruption
- [ ] Text editor (port Helix initially — Rust, easy to port; consider Neovim)
- [ ] Process explorer:
  - [ ] Identify process by clicking window, kill it
  - [ ] Find by name, show subprocesses/threads/libraries/capabilities
  - [ ] Pause, resume, kill, change priority, restart
  - [ ] Show what's blocking a process, what's waiting on its locks
  - [ ] System resource graphs (CPU, RAM, disk, network over time)
- [ ] Photo/video viewer
- [ ] Music player
- [ ] Settings/configuration UI (comprehensive, centralized, Windows-inspired layout) — Python/fastpy candidate
- [ ] System information explorer (hardware info + OS info + tuning params) — Python/fastpy candidate
- [ ] Backup program (snapshot-based, with all common backup types) — Python/fastpy candidate
- [ ] Background file indexer (configurable paths/extensions, off by default) — Python/fastpy candidate

### 4.2 Package manager — Python/fastpy candidate
- [ ] Content-addressed immutable store (Nix model)
- [ ] Shared dynamic linking within a generation (fast security patches)
- [ ] Atomic updates and rollback (generation pointer swap)
- [ ] File-level deduplication via hardlinks within the store
- [ ] Binary packages (preferred) with source build option
- [ ] Show requested capabilities before install (Android-style)
- [ ] Repository model:
  - [ ] Official curated repository
  - [ ] Third-party repository support (user adds URL)
  - [ ] Direct .pkg installation from anywhere

### 4.3 Port Chromium
_This is the biggest single porting effort. Unlocks browser, web apps, and VS Code._
- [ ] Port Chromium (~35M lines C++) — requires functional POSIX layer, GPU, audio, networking
- [ ] System web app framework (Chromium as shared system component, not per-app Electron)
- [ ] Port VS Code (runs on Chromium + Node.js)
- [ ] Port Thunderbird (email client)

### 4.4 Development tools
- [ ] gcc, cmake, make, pkg-config (via POSIX layer)
- [ ] Rust toolchain (for kernel recompilation)
- [ ] CPython (latest, for ecosystem compatibility and fastpy bootstrapping)
- [ ] fastpy compiler (Python AOT compiler — first-class language for OS userspace components)
- [ ] Custom Rust target for the OS (`x86_64-unknown-youros`)
- [ ] Port Rust std library to native syscalls

### 4.5 Remote desktop
- [ ] Port FreeRDP (working remote desktop early)
- [ ] Native compositor-level streaming (efficient draw-command forwarding)
- [ ] Video-encoded capture fallback for fullscreen games/video
- [ ] DynDNS setup helper in settings

### 4.6 System snapshots
- [ ] Package snapshots (manifest of active store paths — essentially free)
- [ ] Mutable data snapshots (CoW at filesystem level, 64 KiB block default)
- [ ] Snapshot tree (branch like VMs, select what to include)
- [ ] Rollback any OS update, permanently disable it or retry later

### 4.7 Service discovery / RPC — Python/fastpy candidate
- [ ] D-Bus-like named service registry (simpler binary protocol, not XML)
- [ ] Programs register named services with typed interfaces
- [ ] Service discovery by name, typed RPC calls over channel IPC
- [ ] Standard event loop integration API ("give me the waitable handle")

---

## Phase 5: Advanced Features and Ecosystem

_Depends on: Phase 4 (working daily-driver desktop). Goal: competitive OS._

### 5.1 Linux compatibility layer
- [ ] Linux syscall translation layer (like FreeBSD's Linuxulator)
- [ ] epoll, eventfd, signalfd emulation
- [ ] /proc emulation (enough for WINE and common Linux apps)
- [ ] Linux threading model (clone, futex)
- [ ] Linux DRM/KMS compatibility (for NVIDIA proprietary driver userspace)
- [ ] ALSA/PulseAudio compatibility shim
- [ ] Result: WINE runs unmodified (or with minimal patches) → Windows app support

### 5.2 Additional filesystems
- [ ] Port Btrfs (CoW, snapshots, checksums)
- [ ] Port F2FS (SSD optimization)
- [ ] NTFS read/write support
- [ ] Queryable file metadata / indexed attributes (BeOS BFS-inspired)
- [ ] Application-level atomic write transactions

### 5.3 Additional schedulers (if needed)
- [ ] EEVDF-style scheduler (for users wanting sophisticated fairness)
- [ ] Deadline scheduler (for real-time/audio workloads)
- [ ] Selectable in settings, requires reboot to switch

### 5.4 Advanced security
- [ ] Per-process filesystem namespaces for sandboxing
- [ ] Interceptor hooks (synchronous, capability-gated, 100ms timeout)
- [ ] Async notification hooks / tracing subsystem
- [ ] Profiling mode for high-frequency events (alloc/dealloc tracing)

### 5.5 Container support
- [ ] Namespace primitives (PID, network, mount, user)
- [ ] Resource control groups (CPU, memory, I/O limits per group)
- [ ] Port Docker (or equivalent container runtime)

### 5.6 Additional software
- [ ] Archive support (zip, 7z, tar.gz, rar)
- [ ] Speech input / speech output
- [ ] Cellphone camera/microphone integration
- [ ] Scripting language registration (Lua and/or WASM runtime for app extensibility)
- [ ] Keyboard layout customizer (arbitrary remap, save named layouts)
- [ ] Let's Encrypt SSL certificate helper
- [ ] Optimized keyboard layouts (Dvorak, Colemak, others)

### 5.7 Installation wizard — Python/fastpy candidate
- [ ] Easy install (automatic partitioning) and manual install (partition manager)
- [ ] Keyboard/layout selection
- [ ] Auto-detect monitor DPI, let user adjust scaling
- [ ] Workload type selection (populates tuning presets, changeable later)
- [ ] Swap file sizing
- [ ] Post-reboot setup: audio device, timezone, user/password, WiFi, theme, browser choice
- [ ] Unattended install via YAML configuration file
- [ ] GRUB integration for dual-boot (add menu entry, don't replace GRUB)

---

## Dependency Graph (critical path)

```
Phase 0 (setup)
    │
Phase 1.1 (boot) ──→ 1.2 (memory) ──→ 1.3 (scheduler) ──→ 1.4 (IPC/syscalls)
                                                                    │
                                            1.5 (capabilities) ←───┤
                                                                    │
                                            1.6 (processes) ←──────┘
                                                    │
                    ┌───────────────────────────────┤
                    │                               │
            2.1-2.2 (drivers)              2.5 (POSIX layer)
                    │                               │
            2.3 (filesystem)               2.7 (shell, tools)
                    │                               │
            2.4 (networking)               2.6 (service manager)
                    │                               │
                    └───────────┬───────────────────┘
                                │
                    3.1 (GPU drivers) ──→ 3.2 (graphics stack)
                                                    │
                                        3.3 (compositor) ──→ 3.4 (desktop shell)
                                                                    │
                                                            3.5 (GUI toolkit)
                                                                    │
                                                    ┌───────────────┤
                                                    │               │
                                            4.1 (core apps)  4.2 (package mgr)
                                                    │
                                            4.3 (Chromium)
                                                    │
                                        5.1 (Linux compat) ──→ WINE ──→ Windows apps
```

---

## Notes

- **Don't write custom filesystems early.** Port ext4. Data loss bugs are unforgivable.
- **Don't write a custom browser.** Port Chromium. It's huge but unlocks three things at once.
- **GPU drivers are the hardest part.** Start with AMD (open source). Intel second. NVIDIA last (via Linux compat layer).
- **Benchmark everything.** Write benchmarks before optimizing. AI is good at optimization with a concrete target.
- **One scheduler is enough initially.** Define the trait interface, implement priority round-robin, add alternatives only if users need them.
- **Security model from day one.** Capabilities, IOMMU, CFI — bake these in early. Retrofitting security is much harder.
