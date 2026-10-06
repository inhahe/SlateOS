## TD-C-THE-DESKTOP-CANNOT-SET-THE-BRIGHTNESS-IT-SHOWS (lane C, 2026-10-06)

**Status:** OPEN -- the kernel's half exists; the desktop's call and the
right to make it do not.

**In short:** the notification pane shows the screen's brightness as the
kernel reports it and says "Can't be changed yet" where its slider would
be (design-decisions §1485). The kernel can change it -- lane A built
`SYS_BRIGHTNESS_SET` (1075) for exactly this -- but only for a process
holding the `SET_BRIGHTNESS` right, and nothing gives the desktop that
right, nor does the desktop make the call. A user cannot dim the screen
from the desktop.

**Where:** `gui/desktop/src/backlight.rs` (reads `/proc/brightness`, sets
nothing); `kernel/src/syscall/number.rs` (`SYS_BRIGHTNESS_SET`, gated on
`Rights::SET_BRIGHTNESS` on `ResourceType::Process`);
`requests/c-a-brightness-has-setters-and-no-door.md` (lane A's answer).

**To reproduce:** open the notification pane on SlateOS: the brightness row
shows a level and "Can't be changed yet".

**The proper fix, in two halves:**

1. *The right* -- whoever starts the desktop gives it `SET_BRIGHTNESS`
   (an init `caps:` entry for the desktop's line, design-decisions §1174,
   or the session that starts it). Not lane C's tree.
2. *The call* -- lane C's: `backlight` makes `SYS_BRIGHTNESS_SET` for the
   first display (a raw `syscall` on SlateOS, as `gui/remote`'s
   `channel.rs` makes its calls), the pane's slider comes back for a
   process that holds the right, and a refusal (`EPERM`) is shown as the
   reason in the slider's place, as now. A setting of the level the
   display already has is the probe: it changes nothing anyone sees.

**If never fixed:** the screen's brightness can be changed only from
`kshell`; the pane says so rather than pretending.
