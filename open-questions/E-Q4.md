## E-Q4 — [E] There are two recycle bins, and neither can see what the other holds. Which one is SlateOS's? — Status: OPEN (raised 2026-09-27)

**In short:** deleting a file in the file manager or the image viewer moves it
to a recycle bin in your home folder, and since today the file manager can show
that bin and put things back from it. The system has a second recycle bin of its
own, built into the kernel (the core of the OS that every program runs on),
which programs could use through a system call (the way a program asks the
kernel to do something) -- but no program does. A file in one is invisible to
the other. And the design asks for more than either does: *every* delete,
including one typed at the command line, should go to the bin; each drive
should keep its own bin; and old items should go only when space runs short.
The question is which bin to build that on, because the three answers put the
work -- and the rules about whose files are whose -- in different places.

**The two, side by side.**

| | The home bin (`~/.recycle`) | The kernel's bin (`/_TRASH`) |
|---|---|---|
| Used by | the file manager, the image viewer, and now Disk Cleanup | nothing but the kernel's own debug shell |
| Whose | one per user | one for the whole machine, shared by every user |
| File names | kept exactly, whatever bytes they hold | text only, and at most 255 bytes of path |
| Per drive | no: deleting from a USB stick copies the file onto the system disk | no: "one per filesystem" is planned, not built |
| Command-line deletes | cannot reach it | could, through the system call, once the shell's `rm` used it |

| Option | *What changes* for the user | What it needs | Cost / risk |
|---|---|---|---|
| **A. The kernel's bin, extended** | a file deleted at the command line appears in the file manager's bin | lane A: one bin per user and per drive, names kept exactly, no length limit; then lane E points the desktop at it | the kernel decides where each user's deleted files live -- policy in the core, where the design keeps as little as possible |
| **B. The home bin only**; the kernel's is retired | nothing, for the desktop; command-line deletes stay permanent unless `rm` is taught to recycle | lane A removes `/_TRASH`; lane E adds one bin per drive to `apps/recyclebin` | cheapest; leaves the design's "every delete goes to the bin" to each program choosing to |
| **C. A recycle-bin service** (a background program, like the backup service) that owns every user's bins on every drive; the kernel's delete call hands the file to it | as A: every delete reaches the one bin the file manager shows | a new service (lane D), the call redirected to it (lane A), the desktop asking it (lane E) | most work; keeps the policy out of the core, which is the design's microkernel rule |

**Recommendation:** C as the destination, reached through B: keep the home
bin as the one the desktop uses (it already is, and its format loses nothing),
add one bin per drive to it, and build no more on `/_TRASH`. When the service
exists it takes the home bin's format with it, so nothing a user has deleted is
stranded by the move.

**If never answered:** nothing breaks or gets worse. The desktop's deletes are
recoverable from the desktop's bin; command-line deletes are permanent, as on
most systems; and `/_TRASH` holds only what someone put there from the kernel
shell by hand.

**Where:** `apps/recyclebin/src/lib.rs` (the home bin); `kernel/src/fs/trash.rs`
and `SYS_FS_TRASH` .. `SYS_FS_TRASH_EMPTY` (618-621) in
`kernel/src/syscall/number.rs` (the kernel's); `design.txt`, "recycle bin".
