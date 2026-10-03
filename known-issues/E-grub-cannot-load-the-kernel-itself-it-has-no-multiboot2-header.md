### [E] GRUB cannot load the kernel itself: it has no multiboot2 header -- 2026-09-28

**Status:** open, and lane A's to decide (it is the kernel's boot protocol).
Nothing is blocked: the installer's GRUB entry chainloads Limine instead,
which works today.

**In short:** GRUB can start an operating system in two ways -- load its
kernel itself (the `multiboot2` command, which needs a small header in the
kernel file saying it can be started that way), or hand over to another
bootloader (`chainloader`). Slate OS's kernel is started by Limine, through
Limine's own protocol, and carries no multiboot2 header, so only the second
works: a GRUB menu entry for Slate OS starts Limine, which starts the kernel.
That takes an EFI system partition with Limine on it, and a machine started
through UEFI -- GRUB started through the BIOS cannot run an EFI program.

**Where:** `apps/installer/src/grub.rs` keeps both strategies
(`GrubEntryType::Direct` renders a `multiboot2` entry, and its tests hold it
to GRUB's quoting rules). The configuration (`bootloader:` with
`strategy: direct`, `lib.rs` `parse_bootloader`) and the command line
(`--direct`, `grubcmd.rs` `DIRECT_REFUSED`) refuse it, saying why. The kernel's
entry (lane A, `kernel/`) reads Limine's boot information.

**What the fix would be:** a multiboot2 header in the kernel image, and an
entry path that takes multiboot2's boot information (memory map, framebuffer,
modules) as well as Limine's -- a second boot protocol, which is lane A's
call. If it lands, the two refusals go, and the installer can offer an entry
that loads the kernel directly -- the one way to boot Slate OS from a GRUB
started through the BIOS. Not filed as a request: nothing waits on it.
