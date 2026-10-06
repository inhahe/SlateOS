## TD-C-THE-DESKTOP-CANNOT-SET-THE-BRIGHTNESS-IT-SHOWS (lane C, 2026-10-06)

**Status:** OPEN -- the kernel's call exists and the desktop makes it; the
right to make it is not given to the desktop.

**In short:** the notification pane shows the screen's brightness as the
kernel reports it and says "Can't be changed yet" where its slider would
be (design-decisions §1485). The kernel can change it -- lane A built
`SYS_BRIGHTNESS_SET` (1075) for exactly this -- but only for a process
holding the `SET_BRIGHTNESS` right, and nothing gives the desktop that
right. A user cannot dim the screen from the desktop.

**Where:** `gui/desktop/src/backlight.rs` (reads `/proc/brightness`; each
time the pane opens, sets the first display to the level it has, which
no one sees and which answers whether the desktop may -- `KernelSetter`,
a raw `syscall` on SlateOS); `kernel/src/syscall/number.rs`
(`SYS_BRIGHTNESS_SET`, gated on `Rights::SET_BRIGHTNESS` on
`ResourceType::Process`); `requests/c-a-brightness-has-setters-and-no-door.md`
(lane A's answer).

**To reproduce:** open the notification pane on SlateOS: the brightness row
shows a level and "Can't be changed yet".

**The proper fix:** whoever starts the desktop gives it `SET_BRIGHTNESS`
(an init `caps:` entry for the desktop's line, design-decisions §1174, or
the session that starts it). Not lane C's tree, and nothing on the image
starts the desktop yet. Lane C's half is done (2026-10-06): the moment the
desktop holds the right, its probe is accepted and the pane's slider sets
the screen, with no change to the desktop.

**If never fixed:** the screen's brightness can be changed only from
`kshell`; the pane says so rather than pretending.
