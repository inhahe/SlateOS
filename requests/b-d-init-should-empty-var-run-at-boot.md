# B → D: init should empty `/var/run` when the machine starts

**Filed:** 2026-10-16 by lane B. **Owner of the fix:** lane D (`services/init`).
**Status:** OPEN

## In short

`/var/run` holds what describes the running system -- who is signed in, how
many wrong passwords were typed lately, which programs have started -- and
every Unix-like system empties it when it starts, because none of that is
true of the next boot. SlateOS does not: init empties `/var/run/utmp` and
leaves everything else, and the root file system is ext4, so the rest
survives a reboot.

## What is wrong now

* `services/init` (`main.rs`, the login-records step) truncates
  `/var/run/utmp` and writes this boot's record -- the right thing, for one
  file.
* Nothing else under `/var/run` is cleared, and nothing mounts a tmpfs there.
* Two lane-B programs assumed otherwise, both in writing:
  * `authlib`'s tally of failed sign-ins, `/var/run/authlib/tally`
    (`userspace/authlib/src/lib.rs`, `DEFAULT_FAILLOCK`): a lock-out earned
    before a reboot outlives it.
  * `loginmgr`'s mark that someone has signed in since the machine started,
    `/var/run/loginmgr/signed-in`: before 2026-10-16 it made the automatic
    account never sign in by itself again after the first boot. `loginmgr`
    now writes this boot's stamp (utmp's boot record) into the mark and
    ignores a stale one, so it no longer depends on this -- but the tally
    still does, and so will the next program that puts state there.

The Filesystem Hierarchy Standard says it plainly: files under `/var/run`
"must be cleared (removed or truncated as appropriate) at the beginning of
the boot process".

## What is asked

Early in boot -- before any service starts, and before init writes utmp's
boot record -- remove everything under `/var/run` except what init itself
recreates there (`utmp`), or mount a tmpfs on `/var/run` and `/run` if the
kernel offers one. Then say so in init's module docs, beside the utmp step.

## How to check it

Boot, `touch /var/run/x` and `mkdir -p /var/run/y/z`, reboot: both are gone,
and `/var/run/utmp` holds one boot record. `loginmgr`'s and `authlib`'s
behaviour needs no change for this.
