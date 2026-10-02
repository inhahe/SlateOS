## 1335. Any program's settings file is announced by name, a name checked at the wire, and the watch for changes runs outside the compositor

**Date:** 2026-09-27
**Lane:** F
**Decided by:** Claude (autonomous), taking lane C's proposal
(`requests/c-f-announce-a-change-to-any-programs-settings-file.md`) inside the
operator's answer to C-Q26 (§1418: each program keeps its settings in a file
of its own, and a change reaches its open windows at once). Lane C offered the
watch in the compositor as an equal alternative; lane F chose against it.

**In short:** when any program's settings file changes, every open window is
told, and the program the file belongs to re-reads it -- so a change shows at
once instead of at the next start. The display protocol names the file, but
only by a name of 1 to 32 lowercase letters, digits, `_` or `-`, which cannot
point anywhere but the settings folder. Noticing the change is a small
service of lane C's (`gui/settingswatch`) that watches the folder and sends
the announcement; the compositor only passes it on.

**What was decided.**

- **A checked name, not a closed list.** The four files the desktop reads stay
  variants of `SettingsGroup`; every other program's is
  `SettingsGroup::Program(SettingsName)`. One function,
  `SettingsName::new`, decides what a name may be, and both decoders (the
  request and the event) use it.
- **One way to say each file.** A desktop file announced by name
  (`AnnounceSettings { name: "input" }`) is exactly its own verb: re-read if
  the compositor reads it, then announced as `Input`. The event decoder
  refuses `Program("input")`, which no compositor sends, so a receiver has one
  thing to match per file (`guiremote::settings_group`).
- **Announced, not adopted.** The compositor never reads a program's file. The
  reply is `Ok` whatever the file says, as for the four verbs.
- **The watch is its own process.**

**Alternatives.**

| | For | Against |
|---|---|---|
| The watch in `gui/settingswatch` (chosen) | a filesystem watch and its failures -- inotify limits, a folder not yet created, a user's config folder the compositor may not own -- fail alone; the compositor stays a relay | one more process for the session to start |
| The watch in the compositor | one process fewer; it already relays and lives as long as the session | every window depends on the compositor, which would take on a per-user folder and its errors |
| A free string on the wire | no length or alphabet limit | a sender could name any path, and every receiver would parse |

**How to reverse.** Moving the watch into the compositor is lane C's loop
moved into `Compositor`; the protocol stays as it is. Widening the alphabet
or the length is `SettingsName::new` alone (and the input and control
versions, since a longer name would be refused by an older peer).
