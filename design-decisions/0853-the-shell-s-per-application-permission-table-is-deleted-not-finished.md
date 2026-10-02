## 853. The shell's per-application permission table is deleted, not finished

**Date:** 2026-09-15
**Lane:** C
**Decided by:** Claude (autonomous) — on `design.txt`'s authority rather than on
my own preference; see below.

**In short:** the desktop carried a 2,014-line model of which programs may use
your camera, microphone and location, with nothing using it. It is deleted
rather than wired up, because this system does not decide those questions with
a table of program names. It decides them by whether a program was handed a
handle to the device, and a table that could override that would be a second
answer to a question the kernel already answers.

**What was there.** `gui/desktop/src/privacy_settings.rs`: `PermissionKind`,
`PermissionState`, `AppPermission`, `ActivityEntry`, `TelemetryLevel`,
`PrivacySettings`, with `is_allowed`, `revoke_all` and 37 tests. Nothing
outside the file named any of it.

**Why deleting rather than wiring.** `design.txt` specifies capability-based
security from day one, with no ambient authority: a program reaches a device
because it holds an unforgeable handle, not because a central list has its name
ticked. A per-application permission table is therefore not this system's model
half-built — it is Windows' and Android's model, borrowed. Wiring it up would
mean building the store the kernel does not consult, and then either ignoring
it (a control that changes nothing) or letting it override the capability
system (two answers to one question, with the weaker one on top).

That is the whole argument, and it is `design.txt`'s rather than mine. The
decision I actually made is only *when* — the file has been dead for as long as
it has existed, and `apps/settings`' copy of the same model shipped a tick-and-
cross table telling users a browser had their camera. That page went on
2026-09-15; this is the other half.

**The case for keeping it, which I do not think survives.** It is 2,014 lines
and 37 tests of real work, and a future permission *UI* would want something.
But what it would want is a view of capabilities a process actually holds —
readable from the kernel, not stored here — and that is a different data model
with a different source of truth. Keeping this one would give whoever writes
that a head start in the wrong direction, which is worse than a blank file.

**What is preserved.** `gui/toolkit/src/surface.rs` cited this module as its
worked example of a test that passes vacuously — a `FillRect` matcher that sees
nothing once the box becomes an outline, and keeps passing because every
assertion is over an empty set. The example is why anyone believes the lesson,
so the doc now records that the module was deleted and what it demonstrated,
rather than pointing at a file that is not there. Deleting an example silently
is how a rule becomes folklore.

**Recorded in** `known-issues.md` under
`TD-C-THE-PRIVACY-PAGE-TOLD-YOU-A-BROWSER-HAD-YOUR-CAMERA`, which carries the
`apps/settings` half.
