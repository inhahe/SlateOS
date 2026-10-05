## 1016. The console `login(1)` takes the name `login`; the display manager becomes `loginmgr`

**Date:** 2026-09-10
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** two different programs in this tree were both called "login" --
the text one that asks for your password on a terminal, and the graphical one
that draws a login screen. The graphical one owned the name. Since neither said
otherwise, the *binary* named `login` was the graphical one, and `getty` -- the
program that puts a login prompt on a terminal -- runs `/bin/login`. Nobody had
built a disk image yet, so nothing had gone wrong; whoever built the first one
would have found a graphical login manager starting on a serial console.

**What changed.** `init/login` (package `login`) is now `init/loginmgr`
(package `loginmgr`). `userspace/login` (package `login-cli`) is now the
package `login`. Both directories now match their package names.

**Why the console program wins the name.** `login(1)` is a POSIX program with
a manual page, and `getty` hardcodes `/bin/login`. The graphical one is a
display manager -- its own module doc calls it "Login Manager — Display Manager
and Session Launcher" -- and display managers are conventionally named for
themselves (`gdm`, `sddm`, `lightdm`), never `login`.

**How it was found**, which is the part that generalises. Not by looking for
it. §1014's tick ran `cargo test -p login` after every edit to
`userspace/login` and got a clean "46 passed" every time, from the *other*
crate. It surfaced because the count did not move after three tests were added.
The naming gate that came out of that (gate 18) then made this rename
mechanical -- and the gate's own shrink-only rule caught the half-done state
immediately: the moment the directories were renamed and the baseline was not,
it failed with "KNOWN_COLLISIONS lists crates that no longer collide".

**A cost estimate in the tracking entry was wrong, and cheaply checkable.** It
said the rename "touches every Cargo.toml that depends on it and is worth doing
deliberately rather than as a tail-end of an unrelated commit". Nothing depends
on either crate by path -- both are leaf binaries -- so it was two `name =`
lines and a `git mv`. The grep that establishes that takes a second, and it was
not run before the sentence was written. **A deferral justified by an unmeasured
cost is the same defect as a deferral justified by an unmeasured capability**
(§1013's `setuid` note, §1014's `getuid`), just pointing the other way.

**Not renamed:** the four collisions in `apps/**` are lane C's. They were
notified, with the second question this rename raises for each of them: which
crate currently owns the *binary* name, and is it the one that should?

**Note for anyone following an old path.** Entries above this one, and the
`requests/` archive, refer to `init/login`. They were true when written and are
left alone; the code is at `init/loginmgr`.
