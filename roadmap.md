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
- [ ] Set up benchmark infrastructure (for measuring performance as the OS grows)
- [ ] Integrate fastpy compiler into build system (Python AOT → native executables for OS components)

---

## Phase 1: Kernel Core

_The minimum kernel that can run a single userspace process._

### 1.1 Boot and hardware init
- [x] UEFI boot → enter kernel entry point (Limine bootloader, kmain entry)
- [ ] Parse ACPI tables (hardware discovery for x86) — IOAPIC address currently hardcoded
- [x] Initialize GDT, IDT, interrupt handlers
- [x] Set up 16 KiB page tables (4×4KiB hardware pages per logical frame)
- [x] Set up kernel heap allocator (slab allocator, power-of-2 size classes)
- [x] Initialize serial console for debug output (UART 16550, COM1)
- [x] Initialize PCI bus enumeration

### 1.2 Memory manager
- [x] Physical page allocator (buddy allocator for 16 KiB pages)
- [x] Virtual memory manager (page tables, mapping, unmapping)
- [x] Kernel virtual address space layout
- [x] Userspace virtual address space layout
- [x] Demand paging (page fault handler, lazy allocation)
- [x] Stack growth via page fault (guard page at bottom)
- [ ] Swap file support (not partition)
  - [ ] zswap/zram compressed swap (recommended for desktop)
  - [ ] Swappiness tunable (default 10-20 for desktop)
- [ ] Committed vs. lazy memory allocation modes
- [ ] Runtime-tunable memory parameters via sysctl-like interface
- [ ] Workload profiles (Desktop, Database, Development, Gaming) as presets

### 1.3 Scheduler
_Define scheduler trait interface first, implement one scheduler behind it._
- [x] Scheduler trait interface:
  - `pick_next_task(cpu) -> task`
  - `enqueue_task(task)`
  - `dequeue_task(task)`
  - `task_tick(task)` (timer interrupt)
  - `balance_load()` (periodic)
- [-] Priority round-robin scheduler (default):
  - [x] 32 or 64 priority levels, real-time levels at top
  - [x] Round-robin within each priority level
  - [x] Configurable time slices per level (shorter = higher priority)
  - [ ] Per-CPU run queues
  - [ ] Work stealing from longest queue when idle (prefer same NUMA node)
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
  - [ ] Runtime-tunable time slice durations
- [x] Process/thread pause, resume, priority change while running
  - [x] sched::suspend/resume for task pause/unpause
  - [x] sched::set_priority for runtime priority change
  - [x] SYS_THREAD_SUSPEND (513), SYS_THREAD_RESUME (514), SYS_THREAD_SET_PRIORITY (515)
  - [x] Same-process authority enforcement
- [ ] Workload profile presets for scheduler parameters

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
- [-] Capability-gated syscalls
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
  - [ ] DMA mapping setup syscalls
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
- [x] RTC (real-time clock)

### 2.3 Filesystem
_Port ext4 first. Don't write a custom filesystem._
- [x] VFS (virtual filesystem) layer — FileSystem trait, mount table, path resolution
- [ ] Port ext4 (primary filesystem, read-write)
- [x] FAT (USB drives, EFI System Partition — essential)
  - [x] Unified FAT16/FAT32 driver (auto-detect, BPB parsing, FAT chain, readdir, file read/write/delete, subdirectories, mkdir, rmdir)
  - [x] Tested with both FAT16 (4 MiB) and FAT32 (64 MiB) disk images
- [ ] ISO 9660 (optical media)
- [ ] Filesystem features:
  - [ ] Case-sensitive paths, forward slash separator
  - [ ] Filenames: allow everything except `/` and null byte, 255 byte max
  - [ ] Journaling (via ext4)
  - [ ] File metadata: owner, group, capabilities, created/modified/accessed (relatime), hash, size, immutable flag, append-only flag, arbitrary extended attributes
  - [ ] Filesystem change notification system (inotify equivalent)
  - [ ] Change journal for "what changed since timestamp X" queries (for backup programs)
  - [ ] Recycle bin (per-filesystem, trash-capable delete vs. permanent delete syscalls)
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
- [ ] PID 1 init process
- [ ] Dependency-based parallel service startup
- [ ] Socket activation
- [ ] Automatic crash restart with backoff
- [ ] Resource limits per service (cgroup-equivalent)
- [ ] JSON-lines structured logging (text-based, not binary)
- [ ] "Service ready" notification API
- [ ] Startup app list (simple serial list, separate from service manager)
  - [ ] Disk-idle heuristic for "app is loaded, start next one" (2-3 sec timeout)
  - [ ] Explicit readiness notification API

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
- [ ] BFQ-style I/O scheduler:
  - [ ] Realtime priority (audio/video)
  - [ ] Best-effort with priority levels
  - [ ] Idle priority (background indexing, backup, dedup)
  - [ ] Capability-gated realtime I/O priority

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
