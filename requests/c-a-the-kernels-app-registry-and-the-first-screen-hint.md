# C -> A -- from the operator's answers: the kernel's program list goes (later), a hint on the first screens, and a start for repair the desktop can see

**From:** Lane C. **To:** Lane A (`kernel/**`, the boot path).
**Filed:** 2026-09-27. **Status:** OPEN -- all decided by the operator, and
lane C's halves of all three are in (2026-09-27: `gui/programs`,
`gui/desktop/src/autologin.rs`); the rest is yours.

## 1. `fs::appregistry` goes -- after its contents have a new home (C-Q20, §1425)

**In short:** four parts of the system each keep a list of installed programs.
The operator chose one list, in a userspace library under `gui/`; the kernel's
registry goes. **But not first:** "before deleting anything else you should
collect all of the built-in apps, categories, MIME types, apps, file types,
per-role defaults, etc. and make sure they survive in the new place."

**Update 2026-09-27: the inventory and the library are done.** There were
fourteen lists, not four -- nine of them yours: `fs::appregistry`,
`fs::defaultapps`, `fs::associations`, `fs::mime`, `fs::filetype`,
`fs::pinnedapps`, `fs::startmenu` (its favourites and quick links),
`fs::applaunch`, and the empty `fs::openwith` and `fs::appstore`.
`gui/programs/INVENTORY.md` lists every item each held and where it now lives
-- the programs and defaults in `gui/programs`, the file types and content
signatures in `guitk::filetypes`, the default pins in the shell's first start --
or why it was not carried (nine invented program paths, priorities between a
program and one that does not exist, and so on). `gui/programs/tests/inventory.rs`
holds that to the code.

**So they can go, as far as their contents are concerned, whenever suits you.**
Nothing outside the kernel reads any of them -- the shell, File Associations and
Settings each kept their own list, which is what the decision was about -- so
there is no reader to move first. What is yours to decide is the kernel shell's
commands over them (`appreg` and its neighbours) and what `/proc/startmenu`
becomes; lane E's programs moving onto the library
(`requests/c-e-read-the-one-list-of-programs.md`) does not wait on you either.

## 2. "Hold Shift to choose an account", shown from the first screen (C-Q22, §1427)

**In short:** an account set to sign in automatically will do so with no pause;
holding a key at start-up shows the account chooser instead (lane C, the login
screen). The operator asked that the key's existence be shown "as soon as the OS
gets control of the screen up until it starts the automatic login process", and
added: "some BIOSes allow the OS to show a little OS-loading widget ON the BIOS
screen, maybe you could show that the key is available even there."

**Lane C's half is in.** The account signs in before the desktop's first frame,
unless Shift is held or the machine was started for repair -- so the desktop
never has a screen to show the hint on, and everything the person sees between
power-on and their desktop is yours.

**What is asked:**
- **The hint, worded "Hold Shift to choose an account"**, on whatever the kernel
  and early boot draw before the desktop starts -- shown only when an account
  will sign in by itself. The rule for that is one function,
  `loginusers::automatic_account` (`gui/loginusers`): exactly one account
  marked `auto_login: true` in `/etc/users.yaml`, and that one not locked
  (`locked: true`, or a stored password entry starting `!` or `*`). A userspace
  screen can call it; the kernel cannot, and may want the answer another way --
  if a small file written whenever the accounts change would suit better, say
  so and lane C will propose who writes it. A hint shown when nobody will be
  signed in automatically promises something that does not happen, which is why
  the rule is shared rather than restated.
- **Over the firmware's logo** where UEFI leaves it on screen: the firmware
  publishes the logo's position in the ACPI boot graphics table (BGRT); Windows
  draws its spinning dots under that logo without clearing it. The same place
  could carry the hint.
- **A held key reported as held.** Lane F is asked to read `EVIOCGKEY` when the
  compositor opens a keyboard
  (`requests/c-f-which-keys-are-held-when-the-desktop-starts.md`). For that to
  see a Shift held since power-on, the kernel's answer has to include it: a
  PS/2 keyboard repeats a held key's make code, and a USB keyboard's first
  report carries the state -- please confirm both drivers get there before the
  compositor asks.

## 3. A start for repair, marked where the desktop can read it (C-Q22, §1427)

The operator: "I guess it would be nice to skip auto-login during recovery
anyway." The desktop now skips it when the kernel command line carries the word
`recovery` or `single` -- the words the recovery entry of the kernel's own boot
configuration writes (`kernel/src/fs/bootcfg.rs`: `root=/dev/sda2 single
recovery`). Two things stand between that and working:

- **`/proc/cmdline` is made up.** `procfs.rs`'s `gen_cmdline` returns
  `kernel=mintos cpus=N pages=16k`, not the line the machine was started with,
  which `boot::kernel_cmdline()` holds -- and whose own doc says "nothing should
  fabricate a command line". The desktop reads `/proc/cmdline`.
- **No boot entry starts for repair.** `limine.conf` has none. An entry that
  adds `recovery` to the command line is what "starting for repair" means to
  the desktop; what else such a start should do (init, the services) is yours
  and lane B's to decide.

## If this is never done

(1) The kernel's list stays beside the new library -- which is the four lists
still being two, the thing the decision exists to end. (2) Automatic sign-in
still works, and once lane F's half lands so does the key; only the hint is
missing on the screens before the desktop, so someone has to be told about the
key. (3) A start for repair signs in by itself exactly as an ordinary start
does, which is how every start behaved before the decision.

---

## Reply, lane A — 2026-10-01: `/proc/cmdline` is the real line; the rest is on lane A's backlog

**Part 3, first half: done on `lane-a`.** `/proc/cmdline` serves the line the
machine was started with, as `boot::kernel_cmdline_bytes` holds it, plus a
newline as on Linux. It used to invent
`kernel=mintos cpus=N pages=16k`. A line that is not UTF-8 is shown as its
bytes. So the desktop's `recovery`/`single` test reads the truth from lane
A's next publish.

**Still open, in the order I expect to take them:**
1. A boot entry for repair in `limine.conf`, which is lane A's under §973.
2. The hint on the screens before the desktop.
3. Confirming that the PS/2 and USB drivers report a Shift held since
   power-on.
4. `fs::appregistry` and its eight neighbours going.

For the hint, a small file written whenever the accounts change would suit
the kernel better than calling `loginusers` before userspace exists. I will
propose its shape here before building it.

— lane A

## Reply, lane A — 2026-10-02: a boot entry starts for repair

**Part 3, second half: done on `lane-a`.** `limine.conf` (and the copy under
`esp/`) has a second entry, `OS Kernel (recovery)`: the same kernel, with
`single recovery` on its command line -- the words the kernel's own boot
configuration writes for its recovery entry. Choose it in Limine's menu during
the three-second timeout; the first entry stays the default. With the real
`/proc/cmdline` from the first reply, the desktop's test sees both words, so
your todo.txt step -- boot the repair entry with an account marked to sign in
by itself and confirm the login screen shows -- can be run once this reaches
`main`.

Nothing else differs for a start for repair yet: init and the services start
as they always do. Whether they should do less (no network, no automatic
mounts, a root shell) is lane B's to propose and mine to wire; nobody has asked
for it, so it is not on either backlog.

The boot test still boots the first entry. It used to append its own
`cmdline:` to the end of the file, which is now the recovery entry, so it puts
it after the first entry's `kernel_path:` instead, and refuses to run if that
entry ever gains a `cmdline:` of its own.

**Still open:** the hint, confirming PS/2 and USB report a Shift held since
power-on, and `fs::appregistry` with its eight neighbours going.

— lane A

## Reply, lane A — 2026-10-02: a held Shift -- USB yes, PS/2 only sometimes

**Part 2, the held key, answered.** What `EVIOCGKEY` on `/dev/input/event0`
says about a Shift held since power-on:

| Keyboard | Seen? | Why |
|---|---|---|
| USB | **Yes**, from `lane-a` | The driver now asks the keyboard which keys are down when it configures it (GET_REPORT, which every USB keyboard must answer). The answer is the first report the poller hands on, so a held Shift is a press on `event0` and a set bit in `EVIOCGKEY` within 8 ms, long before the compositor opens the device. |
| PS/2, including most laptops' built-in keyboards | **Only if the keyboard keeps repeating it** | PS/2 has no way to ask which keys are down. A held key is seen when the keyboard repeats it, and the kernel's "start scanning" command stops the repeat on a keyboard that follows IBM's specification. Linux sends the same command and has the same blind spot. |

So on a PS/2 keyboard, Shift held from power-on may read as up until it is let
go and pressed again. The options, and why none is clearly safe, are in
`known-issues.md` → `A-PS2-KEY-HELD-SINCE-POWER-ON-IS-NOT-SEEN`; they want a
real PS/2 keyboard to try them on. Holding Shift down *after* the kernel has
started, and still holding it when the desktop starts, works on both.

While doing this I found that the USB keyboard path lost keys in three ways
(a full report read as empty, a request queue that overflowed after half a
second without typing, reports taken by the wrong reader). Those are fixed on
`lane-a` (`A-USB-HID-RECEIVES-FLOOD-THE-RING-AND-FULL-REPORTS-READ-AS-EMPTY`).
Until it reaches `main`, a desktop typed on through USB rather than PS/2 may
drop keys.

**Still open:** the hint on the screens before the desktop, and
`fs::appregistry` with its eight neighbours going.

— lane A
