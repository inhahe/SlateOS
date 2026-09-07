# Studying existing implementations


Extracted from `CLAUDE.md` on 2026-09-06. It lived there, resident in every
lane's context on every turn, and is needed only when you are actually doing
the thing it describes -- which announces itself, so a pointer suffices.
`CLAUDE.md` points here.



Before implementing any major subsystem, study how proven OSes do it. This is not optional — it is the primary mitigation against writing naive code.

### What to Study and Where

| Subsystem | Study these | Where to find them |
|-----------|-------------|--------------------|
| Scheduler | Linux EEVDF, BFS/MuQSS (for desktop ideas), Fuchsia fair scheduler | Linux `kernel/sched/`, BFS patch set, Fuchsia `zircon/kernel/sched/` |
| Memory manager | Linux buddy allocator + SLUB, Fuchsia PMM | Linux `mm/`, Fuchsia `zircon/kernel/phys/` |
| IPC | Fuchsia channels, seL4 IPC, L4 family | Fuchsia `zircon/kernel/object/channel_dispatcher.cc`, seL4 source |
| VFS / filesystem | Linux VFS, ext4 | Linux `fs/`, `fs/ext4/` |
| Capability system | Fuchsia handles, seL4 capabilities, Capsicum | Fuchsia `zircon/kernel/object/`, seL4 source |
| I/O scheduler | Linux BFQ | Linux `block/bfq-*` |
| Graphics compositor | wlroots, Smithay (Rust Wayland), KWin | wlroots source, Smithay crate |
| GUI toolkit | Iced, Slint, egui (Rust), Qt (for widget design ideas) | Respective repos |

### How to Study

1. Read the source for the specific algorithm or data structure, not the entire subsystem.
2. Understand the *invariants* and *design tradeoffs*, not just the code.
3. Adapt the approach to our architecture (e.g., Linux's scheduler assumes CFS/EEVDF semantics; ours uses priority round-robin. Take the per-CPU queue and work-stealing design; drop the virtual-runtime fairness math).
4. Cite your references in code comments: `// Based on Linux's buddy allocator (mm/page_alloc.c) with 16KiB base page adaptation.`

---
