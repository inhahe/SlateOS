# Subsystem map: where things live in the tree

Extracted from `CLAUDE.md` on 2026-09-06. It is a **navigation aid**, not an
ownership boundary -- the lane table in `CLAUDE.md` is what says who may write
where. Consult this when you need to find a subsystem, which announces itself.

The zone names below are a **navigation aid** — they tell you where in
the tree a given subsystem lives. The lane table above, not this table,
is the ownership boundary.

| Zone | Covers | Typical paths |
|------|--------|---------------|
| **kernel-core** | boot, GDT/IDT, interrupts, memory manager, page allocator, heap, scheduler | `kernel/src/boot/`, `kernel/src/mm/`, `kernel/src/sched/` |
| **kernel-ipc** | syscall dispatch, channels, pipes, shared memory, futexes, io_uring, IOCP | `kernel/src/ipc/`, `kernel/src/syscall/` |
| **kernel-security** | capabilities, process namespaces, CFI setup, IOMMU | `kernel/src/cap/`, `kernel/src/security/` |
| **kernel-process** | process/thread lifecycle, ELF loader, exception handling | `kernel/src/proc/` |
| **drivers** | driver framework, USB, storage, network, keyboard, display, virtio | `drivers/` |
| **fs** | VFS, ext4 port, FAT32, recycle bin, change notifications | `fs/` |
| **net** | TCP/IP stack, UDP, DNS, DHCP, sockets, firewall | `net/` |
| **posix** | POSIX compatibility layer, libc translation | `posix/` |
| **init** | service manager, init, startup sequencing | `init/`, `services/` (bare-metal startup binaries) |
| **shell** | shell, coreutils, terminal emulator | `userspace/shell/`, `userspace/term/` |
| **gui-core** | compositor, DRM/KMS, GPU drivers, 2D drawing | `gui/compositor/`, `gui/gpu/` |
| **gui-toolkit** | widget library, layout engine, styling, clipboard, drag-drop | `gui/toolkit/` |
| **desktop** | window manager, taskbar, start menu, system tray, themes | `gui/desktop/` |
| **apps** | file explorer, process explorer, settings, text editor, etc. | `apps/` |
| **pkg** | package manager, content-addressed store, generations | `pkg/` |
| **bench** | all benchmarks and performance infrastructure | `bench/` |
