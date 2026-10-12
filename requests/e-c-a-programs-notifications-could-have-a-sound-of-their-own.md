# E -> C: a program's notifications could have a sound of their own

**From:** Lane E (`apps/settings`). **To:** Lane C (`gui/notifsettings`,
`gui/desktop`, `gui/appearance`). **Filed:** 2026-10-10. **Status:** OPEN --
nothing is broken while it waits: each program's notifications chime with the
theme's notification sound, or not at all.

**In short:** design.txt asks for each program's notifications to have a
sound the user picks -- "set sound (dropdown of sounds from OS sounds
directory, previewable, includes (no sound))" (design.txt line 1295;
roadmap-detailed "Notification settings per app: sound (dropdown,
previewable)"). Today a program's rule has only `sound: bool`: the theme's
notification sound, or silence. Settings' Notifications page can offer the
dropdown -- and play each sound as it is chosen, as the Sound page's rows do
now -- once the rule can hold a sound and the desktop plays it.

## What there is

- `notifsettings::AppRule::sound: bool`, read and written as
  `apps.<name>.sound` in `notifications.yaml`; Settings' Notifications page
  draws it as a switch (`ToggleId::NotifSound`).
- `desktop::event_sounds::for_notification(priority)`: `message-new-instant`
  for a normal or high notification, `dialog-warning` for an urgent one,
  none for a low one -- the same for every program.
- The user's choices for the shell's own events, `sounds.events` in
  `appearance.yaml` (`EventSound::File` or `Off`), which Settings now sets
  (`c-e-a-sounds-page-for-the-sounds-axis.md`, done).

## What it asks (lane C's)

1. **A program's rule holds a sound**, in place of the switch: the theme's
   (today's `true`), none (today's `false`), one of the theme's sounds by
   its sound-naming name (`bell`, `complete`, `message`,
   `dialog-information`, ...), or a file of the user's (an absolute path).
   `true` and `false` in an existing file keep meaning what they mean now.
2. **The desktop plays it** for that program's notifications. Whether an
   urgent one keeps `dialog-warning` or takes the program's own is yours;
   lane E would keep the warning, since urgency is the one thing the sound
   tells that the program's name does not.
3. **The theme's sounds to offer, each with a label**: the list a dropdown
   shows. `sound::BuiltIn::ALL` is that list already -- nineteen sounds,
   each with its sound-naming `name()`, and each one every theme can play
   (its own file, or the built-in) -- and lacks only a label for a person
   ("Message", "Bell", "Complete", ...), as `SHELL_EVENTS` has for the
   shell's eleven.

## What lane E then does

The Notifications page's per-program Sound switch becomes a list: "The
theme's", "Off", each of the theme's sounds by its label, and "A file of my
own..." (the picker the Sound page uses, filtered to `*.oga`, `*.ogg`,
`*.wav`) -- each played as it is chosen, at the system sounds' volume.
