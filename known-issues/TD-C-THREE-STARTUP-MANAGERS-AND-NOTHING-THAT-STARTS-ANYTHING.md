## TD-C-THREE-STARTUP-MANAGERS-AND-NOTHING-THAT-STARTS-ANYTHING

**Date:** 2026-09-14. **Lane:** C. **OPEN.**

**2026-10-06: two now, not three.** The desktop's own panel,
`gui/desktop/src/startup_settings.rs`, is deleted -- nothing ever put it
on screen, and under every answer C-Q33 offers it goes, `apps/startupmanager`
and the Settings page being the list's editor. The table below is as it
was found.

**2026-10-05: the fork is put to the operator as `open-questions.md` C-Q33.**
Lane C had started on the desktop session starting the Linux autostart
folders' entries at sign-in, and stopped at `design.txt`'s "only one or two
ways to load programs on startup": the roadmap's startup app list is the
kernel's `svcstart.rs`, which runs at boot, before anyone signs in -- so a
desktop-run list would be a second list beside it, and which one is *the*
list is the operator's to say. C-Q33 recommends one ordered list per user,
SlateOS's own, started by the desktop at sign-in, with a ported program's
autostart file offered into it switched off.

**In short:** you can tell this system which programs should open when you log
in, in three different places, and none of them has any effect. Nothing
launches user startup applications at all. The settings are recorded, the
switches move, and at the next login nothing happens.

**The three:**

| where | what it is |
|---|---|
| `gui/desktop/src/startup_settings.rs` | 2,129 lines: per-app enable switches, startup delay, boot-time measurement, impact assessment, auto-disable for failing apps |
| `apps/startupmanager` | a whole separate application for the same job, with its own `StartupEntry` type |
| `apps/settings` -> `StartupApps` | a placeholder page, and it stays one -- see below |

**What is missing.** A launcher. Checked 2026-09-14: the desktop session's
start-up path never mentions startup entries; `apps/startupmanager` spawns
nothing but its own window (`app::launch` is its own event loop);
`services/init` does read `/etc/startup.conf` and start things from it, but
those are **system services**, a different concept, and that tree is lane B's.

**Why the Settings port stopped here rather than adding a fourth.**
design-decisions 815 moves screens you open into `apps/settings`, so this panel
is due to move. It is not moving yet, because a ported page would draw
switches that save a value and change nothing -- which is what
`TD-C-THE-MOUSE-SETTINGS-PANEL-REACHES-NOTHING` was filed about, and what the
Mouse page's own comment already refuses in those words: *"Each gets its
control here when it gets a consumer, and not before."* The comment at the
dispatch's `_` arm says this so the next person to look does not port it by
default.

**What the proper fix looks like, in order.** A launcher first: something in
the login path that reads one list and spawns it, with the delay and the
enable flags the existing UIs already model. Then one of the three becomes the
authority and the other two are deleted or become its client. Then the Settings
page is worth building, because its switches will do something.

**The launcher is probably not lane C's.** `/etc/startup.conf` and
`services/init` are lane B's, and if user autostart belongs beside service
startup then the whole mechanism does. If it belongs to the desktop session
instead, it is lane C's. That is a genuine fork, and it is the reason this is
filed rather than started.

**Third family of this shape found today**, after C-Q20's four lists of
installed programs and this file's fifteen private clipboards. Several complete
implementations of one idea, each correct in isolation, none connected, nothing
red, and the system cannot do the thing. It is worth noticing that all three
were found by looking for *unused values* rather than by looking for missing
features.
