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
  - [x] Tested with 1, 2, and 4 CPUs under QEMU

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

### 1.4 IPC and syscalls
- [x] Syscall dispatch (many specialized syscalls, not few generic ones)
- [x] Versioned syscall tables
- [x] Channel IPC (Fuchsia-style, structured message-passing with capability transfer)
- [x] One-way pipes (byte streams)
- [x] Shared memory regions
- [x] Eventfd-like lightweight wake-up counters (kernel-managed integer, wait/wake)
- [x] IOCP-like completion port / unified wait:
  - [x] Register/unregister waitable objects with arbitrary user-data int
  - [-] Wait on: I/O completion, timers, process exit, eventfd counters, semaphores, channel messages (channels, pipes, eventfds, process exit done; timers/semaphores/IO TODO)
- [x] io_uring-style submission queue (optional async path for batch I/O)
  - [x] Shared-memory SQ/CQ ring buffers with atomic head/tail pointers
  - [x] 8 opcodes: NOP, console write, channel send/recv, pipe read/write, FS read/write
  - [x] SYS_IO_RING_SETUP (260), SYS_IO_RING_ENTER (261), SYS_IO_RING_DESTROY (262)
  - [x] 3 self-tests: create/destroy, NOP submission, console write batch
- [x] Futexes (for userspace synchronization without syscall in uncontended case)
- [x] Console I/O syscalls (SYS_CONSOLE_WRITE, SYS_CONSOLE_READ_CHAR — bootstrap console for early userspace)
- [x] Filesystem I/O syscalls (SYS_FS_READ_FILE through SYS_FS_STAT — stateless whole-file operations via VFS)
- [x] Timer syscalls (SYS_CLOCK_MONOTONIC, SYS_SLEEP — lock-free sleep queue, 10ms resolution)
- [ ] Per-process namespace support (mount table remapping for sandboxing)

### 1.5 Capability / security model
- [x] Per-process capability table (unforgeable handles to kernel objects)
- [x] Capability delegation (parent passes subset to child, can't create new ones)
- [ ] User/group model with named capability groups
- [ ] File/directory capability tags (AND-composition between groups, OR within a group)
- [ ] Resource limits as cgroup-like controls (set at launch, kernel-enforced)
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
- [-] Timer (HPET, APIC timer)
  - [x] Local APIC timer (calibrated via PIT, 100 Hz periodic, preemptive scheduling)
  - [x] HPET (High Precision Event Timer): ACPI table discovery, MMIO mapping, 100 MHz monotonic counter, ticks_to_ns conversion, self-test
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
  - [x] ext4 htree (hash-tree) directory index: half_md4/TEA/legacy hash functions, dx_root/dx_node parsing, binary search, O(1) amortized lookups for INDEX directories (read-only; write-side node splitting deferred)
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
