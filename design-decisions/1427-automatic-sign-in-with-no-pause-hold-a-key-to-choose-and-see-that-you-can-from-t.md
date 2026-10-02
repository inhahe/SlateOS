## 1427. Automatic sign-in with no pause; hold a key to choose, and see that you can from the first moment

**Date:** 2026-09-27 &middot; **Decided by:** Operator (Claude recommended B; the operator chose A, and added showing the key) &middot; **Lane:** C (the login screen), A (the earliest screens)

**In short:** An account set to sign in by itself will do so, with no pause at
start-up. To choose a different account, hold a key while the machine starts.
That the key exists is shown on the screen from the moment the system has it
until automatic sign-in begins -- and, where the firmware lets the system draw
over its own start-up logo, there too. Starting for repair skips automatic
sign-in.

**The question:** `open-questions.md` C-Q22 (now resolved). **Verbatim (the
decision):** "The user can always just use the menu to logout and login/switch
accounts after it autologs in, so it's not that crucial, and I definitely don't
want an unnecessary pause during bootup. But also provide the key they can hold
during startup and show that it's available as soon as the OS gets control of the
screen up until it starts the automatic login process. Also, either I'm crazy,
or some BIOSes allow the OS to show a little OS-loading widget ON the BIOS
screen, maybe you could show that the key is available even there." And on
repair: "I guess it would be nice to skip auto-login during recovery anyway."

| Part | Whose |
|---|---|
| Sign in automatically; held key shows the chooser; no automatic sign-in when starting for repair | lane C, `gui/desktop/src/login_screen.rs` |
| The hint on the system's first screens, and over the firmware's logo where UEFI leaves it up (the boot graphics table) | lane A |

**As built (lane C, 2026-09-27).** `gui/desktop/src/autologin.rs`, with the rule
for *which* account in `gui/loginusers` (`automatic_account`), so that the
earlier screens' hint can use the same rule instead of restating it.

- **Which account:** exactly one account marked `auto_login: true`, and that one
  not locked.
- **The key is Shift**, either one, read from the modifiers held as the desktop
  starts; another modifier held beside it does not cancel it.
- **A start for repair** is the word `recovery` or `single` on the kernel
  command line (`/proc/cmdline`), whole words only -- the words the recovery
  entry of the kernel's own boot configuration model writes.
- **Before the first frame**, so the login screen is never drawn, and **only at
  start**: logging out returns to the login screen and stays there.
- **Locking follows the account:** a desktop that signed in by itself locks if
  the account has a password and never if it has none (818's rule, reached from
  the database because no password was checked).

Three smaller calls inside the operator's decision are lane C's own (Decided
by: Claude, autonomous), each chosen to fail toward *showing* the login screen,
which is always safe, rather than toward signing somebody in:

| Call | Why | The other way |
|---|---|---|
| A locked account never signs in by itself | nothing is typed, so an administrator's lock is all that stands between the account and its desktop | honour the mark regardless -- which makes `usermod -L` a suggestion |
| Two marked accounts sign neither in | which was meant is written down nowhere; the file's order could open one person's desktop for another | take the first -- simpler, and wrong half the time for the person at the machine |
| An unreadable command line is an ordinary start | skipping the sign-in for repair is a convenience the operator asked for as "nice", not a lock | refuse to sign in when unsure -- which, on a machine where `/proc` is not mounted yet, turns off the feature everywhere |

Two halves wait on other lanes. The key cannot be read until the compositor
knows about a key held before it opened the keyboard and can say so (lane F,
`requests/c-f-which-keys-are-held-when-the-desktop-starts.md`); until then an
account set to sign in by itself always does, and logging out is the way to
another account, as the operator said it would suffice. A start for repair is
not visible until `/proc/cmdline` is the real command line and a boot entry
carries the word (lane A, with the hint:
`requests/c-a-the-kernels-app-registry-and-the-first-screen-hint.md`).
