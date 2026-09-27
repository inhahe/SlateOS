# C -> A -- two things from the operator's answers: the kernel's program list goes (later), and a hint on the first screens

**From:** Lane C. **To:** Lane A (`kernel/**`, the boot path).
**Filed:** 2026-09-27. **Status:** OPEN -- both decided by the operator; the
first waits on lane C.

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

## 2. "Hold a key to choose an account", shown from the first screen (C-Q22, §1427)

**In short:** an account set to sign in automatically will do so with no pause;
holding a key at start-up shows the account chooser instead (lane C, the login
screen). The operator asked that the key's existence be shown "as soon as the OS
gets control of the screen up until it starts the automatic login process", and
added: "some BIOSes allow the OS to show a little OS-loading widget ON the BIOS
screen, maybe you could show that the key is available even there."

**What is asked:**
- The hint (lane C will propose the wording and the key -- Shift, as elsewhere)
  on whatever the kernel and early boot draw before the desktop starts, when an
  account on this machine signs in automatically.
- **Over the firmware's logo** where UEFI leaves it on screen: the firmware
  publishes the logo's position in the ACPI boot graphics table (BGRT); Windows
  draws its spinning dots under that logo without clearing it. The same place
  could carry the hint.
- The key has to be *read* early enough to matter: a press held from power-on
  is a key already down when the login screen starts, so the login screen needs
  to learn which keys are held (evdev's `EVIOCGKEY`, which the kernel backs,
  answers exactly that).

**Also decided:** starting for repair skips automatic sign-in (lane C's part).

## If this is never done

(1) The kernel's list stays beside the new library -- which is the four lists
still being two, the thing the decision exists to end. (2) Automatic sign-in
still works and the key still works; only the hint is missing on the screens
before the login screen, so someone has to be told about the key.
