## 812. A settings file is watched by comparing its contents, and the desktop is *not* put on a timer to do it

**Date:** 2026-09-06
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Change your accent colour in the Settings app and the running
desktop kept the old one until you logged out and back in. The two programs
share a file and nothing told the desktop it had changed. This adds the piece
that notices — it compares the file's *contents*, not its modification time —
but deliberately stops short of having the desktop check on a timer, because
an idle desktop that wakes up once a second to read a file it will almost
never find changed is a worse thing than the bug.

### Comparing contents rather than a timestamp

The obvious watcher stats the file and remembers the modification time. Here
that is wrong in both directions at once, which is unusual enough to be worth
writing down:

- **It misses changes.** Timestamps are coarse — a whole second on some
  filesystems — so two saves inside one tick carry one timestamp and the
  second is invisible. Dragging a colour slider produces exactly that pattern.
- **It invents changes.** `settingsfile::store` writes a temporary and renames
  it over the target, so *every* save lands a new inode with a new timestamp —
  including the save that happens when somebody opens Settings and closes it
  without touching anything. A timestamp watcher would repaint the desktop
  each time.

Comparing the bytes has neither failure and is *cheaper*, not dearer: these
files are a few kilobytes, so a look is one small read, where stat-then-read
is two syscalls in the case that matters. `settingsfile::Watcher` therefore
holds the contents it last saw. If these files ever grow past that reasoning,
the gate to add is a stat *in front of* the comparison, never in place of it.

### Why the desktop is not put on a timer

This is the half worth defending, because the feature looks unfinished
without it.

`ShellSession`'s `animations` field carries the comment that decided it: an
empty animation set means no wake-up is registered and the loop parks with no
bound at all, *which is what keeps an idle desktop idle*. A once-a-second
appearance check ends that property outright. The machine would wake, read a
file, find nothing, and sleep again, forever, on battery — so that a setting
nobody is changing could be noticed a second sooner. For a person moving a
slider in another window that is a bad trade, and it is not a trade any
polling cadence can avoid: the whole point of a poll is to happen when nothing
has happened.

I tried the intermediate position — poll only on wake-ups the shell was having
anyway, adding no timer — and backed it out. It broke two of the shell's own
tests, and they were right to break. `pump()` is the shell's loop, and a
`$HOME` read inside it makes every session test depend on the machine running
it: on this one there is a real `~/.config/slateos/appearance.yaml`, so two
tests that assert an idle shell repaints nothing began failing against the
developer's own accent colour. This codebase confines `$HOME` reads to
explicit, opted-into calls for precisely that reason — `DesktopShell::new` and
`Compositor::new` both document it — and the loop is not such a place.

So `poll_appearance` exists as an explicit call, on the same terms as the
`load_appearance` beside it, and nothing puts it on a clock.

### What the finished shape is — **built, in the commit after this one**

Not a cadence at all: the notification that already exists. An application
that rewrites `appearance.yaml` sends `guiremote`'s `ReloadAppearance`, and
the compositor re-reads. That design is already argued in
`RequestBody::ReloadAppearance`'s own documentation — it is a *notification*
rather than `SetAppearance(settings)`, so the receiver reads the user's file
itself and a hostile sender achieves at most a redundant re-read.

The shell needed the same message and had no way to receive it: the request
travelled client → compositor and stopped there. It is relayed now.
`Event::SettingsChanged { group }` (wire tag `0x0A`, `INPUT_VERSION` 3) is
announced to every window when the compositor handles either reload, and
`ShellSession` answers the appearance one by calling `poll_appearance`. The
event carries the *group* and no settings, for the reason the request does
not carry them either.

That makes the update immediate **and** keeps the idle desktop idle, which is
the combination no poll can reach — and it is why the polling this entry
declines was worth declining rather than shipping as a stopgap. The announcement
is per window, so the shell hears it once per surface; the second and later
copies are free, because `poll_appearance` compares settings rather than
trusting that being told means a difference.

### Why a settings change is compared, not a file change

`DesktopShell::poll_appearance` re-reads on a file change but returns `false`
unless the *settings* differ. The two come apart on every startup path — the
watcher's first look always reports, having seen nothing yet — and also when
a file changes without any setting changing, as when somebody adds a comment
or a newer desktop writes a key this one does not read. `AppearanceSettings`
derives `PartialEq` over the whole struct precisely so this comparison cannot
fall behind a newly added field, the way the hand-written `is_dirty` field
list it replaced did.

**Alternatives rejected.** A timer, for the reason above. Pushing the settings
themselves over the wire (`SetAppearance`), for the reason
`ReloadAppearance`'s documentation already gives: the settings are one
document with one owner, and a wire form would be a second copy of that model
free to drift. And doing nothing until the kernel grows the filesystem change
notification `design.txt` calls "kernel-level, essential" — which is the right
long-run answer for *watching*, but is lane A's, is not built, and would leave
this bug in place meanwhile for a mechanism that the existing `ReloadAppearance`
notification already makes unnecessary for this particular case.

**Where it lives:** `gui/settingsfile/src/lib.rs` (`Watcher`, `Seen`, 11
tests); `DesktopShell::{poll_appearance, load_appearance}` and the
`appearance_watch` field in `gui/desktop/src/lib.rs` (4 tests, both directions
mutation-checked). The relay: `guitk::event::{Event::SettingsChanged,
SettingsGroup}`, `guiremote::input` (tag `0x0A`, `INPUT_VERSION` 3,
`DecodeError::BadSettingsGroup`), `Compositor::announce_settings_change` and
the two request handlers in `gui/compositor`, and the `SettingsChanged` arm of
`ShellSession::dispatch`. Six further tests, three mutations checked — the
announcement deleted, the group ignored, and the repaint dropped.

One of those three is the reason to distrust a test that merely passes. The
first version of the "an input announcement does not touch the shell" test
announced `Input` against an *unchanged* appearance file, and the mutation that
deletes the group check survived it: with nothing to find, a handler that
ignored the group repainted nothing either. What separates them is announcing
one group while the *other* group's file is dirty.
