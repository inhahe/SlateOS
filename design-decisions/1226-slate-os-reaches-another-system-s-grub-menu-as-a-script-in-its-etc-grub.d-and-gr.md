## 1226. Slate OS reaches another system's GRUB menu as a script in its /etc/grub.d, and GRUB hands over to Limine

**Date:** 2026-09-28
**Lane:** E
**Decided by:** Claude (operator-approved scope) -- the operator decided that
the installer's GRUB support is wired up rather than deleted (§1423, C-Q17);
how it is wired is decided here.

**In short:** to start Slate OS from the menu of a GRUB another system
installed -- a Linux beside it, say -- the installer writes a small script
into that system's `/etc/grub.d/`, the folder GRUB's own tools gather menu
entries from, and runs that system's `update-grub` to rebuild the menu; then
it reads the menu back to see that the entry arrived. The entry hands over to
Limine (Slate OS's own bootloader), which starts Slate OS. When the other
system is not the one running -- its disk mounted from elsewhere -- the
installer writes the script and says how to rebuild the menu from that
system, rather than running this machine's tools on it.

| Call | Chosen | The other way, and why not |
|---|---|---|
| How the entry reaches the menu | a script, `/etc/grub.d/40_slateos`, which `grub-mkconfig` runs when it rebuilds `grub.cfg` | editing `grub.cfg`: the next rebuild -- a kernel update is enough -- writes over it. GRUB's `custom.cfg`, read at boot by the stock `41_custom` with no rebuild: it is the user's own file, so the installer would be managing a block inside someone else's edits, and not every system's `41_custom` reads it |
| Another system's root, mounted | write the script, and say how to rebuild that system's menu from it | run a rebuild here: this machine's tools rebuild this machine's menu, and Slate OS cannot run that system's programs to rebuild its |
| How GRUB starts Slate OS | chainload Limine | load the kernel (`multiboot2`): the kernel has no multiboot2 header (known-issues `[E] GRUB cannot load the kernel itself`), so `--direct` and `strategy: direct` are refused with the reason |
| Where Limine goes | `/EFI/slateos/limine.efi` | the fallback path `/EFI/BOOT/BOOTX64.EFI`, where a disk's own boot starts: another system on the machine may be using it. Limine finds its `limine.conf` from either place |
| After a rebuild | read `grub.cfg` back for the script's `### BEGIN` fence | trust the tool's exit status: `grub-mkconfig` skips a script that is not executable and still succeeds |
| A `40_slateos` without the installer's marker | left alone by add, update and remove, which say whose it is not | overwrite or delete it: it is someone else's |
| A machine started through the BIOS | adding refused, for the running system only | refuse from the absence of `/sys/firmware/efi` under any root: another system's root is mounted without its `/sys`, and its absence there says nothing |
| Finding GRUB's tools | a search of `PATH` in Rust, absolute directories only | asking `which`, as `grub.rs` did: minimal Fedora and Arch installs lack it, and GRUB's tools were reported missing there |
