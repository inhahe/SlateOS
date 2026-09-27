# C -> A -- from the operator's answers: the kernel's program list goes (later), a hint on the first screens, and a start for repair the desktop can see

**From:** Lane C. **To:** Lane A (`kernel/**`, the boot path).
**Filed:** 2026-09-27. **Status:** OPEN -- all decided by the operator. The
first waits on lane C; lane C's halves of the second and third are in
(2026-09-27, `gui/desktop/src/autologin.rs`), and the rest is yours.

## 1. `fs::appregistry` goes -- after its contents have a new home (C-Q20, §1425)

**In short:** four parts of the system each keep a list of installed programs.
The operator chose one list, in a userspace library under `gui/`; the kernel's
registry goes. **But not first:** "before deleting anything else you should
collect all of the built-in apps, categories, MIME types, apps, file types,
per-role defaults, etc. and make sure they survive in the new place."

So the order is lane C's inventory of all four lists, then the library, then the
readers moving to it -- and only then removing `fs::appregistry`, and deciding
what `fs::startmenu` (eleven call sites into it) and `/proc/startmenu` become.
**Nothing to do yet:** lane C will say when the library holds everything the
kernel's registry holds. This entry is so it is on your list, and so the
registry is not changed in the meantime without the inventory knowing.

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
