### [A] A kernel started by `kexec` has no display -- the handoff answers no framebuffer request -- 2026-10-08

**Status:** OPEN -- fixed on lane-a-wip 2026-10-08, awaiting a boot on main.
The handoff now passes the running kernel's framebuffers
(`kexec::build_framebuffer_response`, `FramebufferDesc`) and maps the
`FRAMEBUFFER` memory-map entries write-combining in the new direct map
(`framebuffer_ranges`); a framebuffer outside those entries is not passed on
(`passable_framebuffers`). A self-reload under QEMU reports the same
`[boot] Framebuffer: 1280x800` in both kernels and no warning the first
lacks. Self-tests: the response's layout, the filter, the mapping's memory
type. Was: the gap left before `power.reload` could be what lane C's
"Restart OS -- keep the computer on" needs
(`requests/c-ab-a-restart-that-keeps-the-computer-on.md`).

**In short:** restarting SlateOS without going back through the firmware
(`kexec`: the running kernel loads the new one and jumps to it) gives the new
kernel everything it needs to boot except the screen. It comes up with a
serial console only: no desktop, no text on the monitor. For the start menu's
"Restart OS" that is the difference between a restart and a machine that
seems to have hung.

**Why:** the kernel learns about the screen from the bootloader's framebuffer
response (`boot.rs`, `FRAMEBUFFER_REQUEST`). On a firmware boot Limine
answers it; on a `kexec` restart the running kernel stands in for Limine
(`kexec::prepare_handoff` builds the memory-map, HHDM, executable-address,
RSDP and -- since 2026-10-08 -- kernel-file responses) but builds no
framebuffer response, so the new kernel finds none and treats the display as
absent.

**What the fix needs** (`kernel/src/kexec.rs`):

1. A framebuffer response in the arena: the running kernel's own framebuffer
   descriptors (`boot::framebuffer` -- address, size, pitch, bpp, memory model,
   masks) copied in, an array of pointers to them, and the response
   (`revision`, `framebuffer_count`, `framebuffers_ptr`), as
   `build_memmap_response` builds its three levels.
2. The framebuffer mapped in the handoff page tables' HHDM. The handoff maps
   the direct map only up to the top of managed RAM
   (`top_of_managed_ram`), and a framebuffer is device memory, normally above
   it; Limine maps it into the HHDM (`FRAMEBUFFER` memory-map entries), and the
   new kernel writes pixels through the address it is given. The mapping
   wants the caching Limine gives it (write-combining through the PAT where
   the running kernel set that up), or drawing is slow, not wrong.
3. A self-test beside `build_kernel_file_response`'s: the response's three
   levels round-trip in a `Vec`-backed arena, and `build_handoff_tables`
   maps a fabricated framebuffer range.

**Also not carried, and why that is acceptable for now:** the kernel file's
bytes (`build_kernel_file_response` passes the command line with no file), so a
restarted kernel prints backtraces without function names and cannot
self-reload from its own image; `power.reload`'s caller hands its image in.
