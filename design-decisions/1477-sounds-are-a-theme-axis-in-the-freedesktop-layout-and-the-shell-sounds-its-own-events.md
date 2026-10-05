## 1477. Sounds are a theme axis in the freedesktop layout, the user's own sound for an event over the theme's, and the shell sounds its own events

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the desktop now makes sounds: a chime when a notification
arrives, a tick when the volume changes, a click for a screenshot, a sound for
a device plugged in or out, a low battery, the network coming or going, and
signing in or out. Which sound plays is the user's to choose in
`appearance.yaml`, the way every other part of the look is chosen: a *sound
theme* (`theme.sounds`) -- a folder of sound files in the layout other Linux
desktops use, so theirs install here unchanged -- sounds on or off, a volume,
and the user's own file (or silence) for any one event. Where nothing is
chosen, SlateOS's own synthesized sounds play (§1476). None of it is heard on
SlateOS until the kernel lets a native program reach the sound device
(`requests/e-ad-no-application-can-reach-the-sound-device.md`).

**Where:** `gui/appearance/src/sounds.rs` (`SoundTheme`, the lookup,
`available`, `SHELL_EVENTS`), `gui/appearance/src/lib.rs` (`SoundSettings`,
`EventSound`, `AppearanceSettings::sound_for`, reading and writing `sounds.*`
and `theme.sounds`), `gui/appearance/src/themecheck.rs` (the `sounds` axis's
checks), `gui/desktop/src/event_sounds.rs` (which events sound, and playing),
`gui/desktop/src/session.rs` (signing in and out), `gui/sound/src/builtin.rs`
(the two network sounds). Asked for by `roadmap-detailed.md` §3.4 (*Tier 3 --
Sound Scheme*).

### The file

```yaml
theme:
  sounds: Yaru            # a sound theme's folder name; aero is the built-in one
sounds:
  enabled: true           # false: no event makes a sound
  volume: 0.8             # 0 to 1
  events:                 # the user's own choice for an event, over the theme's
    message-new-instant: /home/u/Sounds/ding.oga   # an absolute path, %-escaped as every path in the file
    battery-low: off
```

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The freedesktop sound theme layout**: `stereo/<event>.oga`, an `index.theme` naming the themes it inherits and its directories, `.disabled` to silence an event | a `sounds` section in `theme.yaml` mapping events to files | Every sound theme made for a Linux desktop installs as it is, and a SlateOS theme folder bundles sounds the same way (a `stereo` directory beside its `theme.yaml`). One format, one lookup. | The specification's lookup -- inheritance, directories by output profile, the extension order, the name cut at its last hyphen -- is more machinery than a map. |
| **Looked for under the SlateOS theme roots, then `$XDG_DATA_HOME/sounds` and each `$XDG_DATA_DIRS/sounds`** | SlateOS's roots alone | Where other desktops install them, so a theme installed for them is found; the user's copy first, as for icons and cursors. | -- |
| **The freedesktop theme is the fallback for every theme but the built-in one** | for every theme, as the specification says | Chosen, SlateOS's sounds are what is heard. With the freedesktop theme installed beside it, the fallback would replace nearly every one of them -- the user would have chosen SlateOS's sounds and heard freedesktop's. Whoever wants those chooses that theme. | An event the built-in sounds have none for is silent in the built-in theme even where the freedesktop theme has one. |
| **The user's own sound for an event, in `appearance.yaml`** | whole themes only | "My own sound for new messages" is the commonest change anyone makes to their sounds, and it should outlive changing the theme. Looked up with the same hyphen cut as a theme, so a sound chosen for `dialog-error` is heard for `dialog-error-serious`. | One more map in the file. |
| **An absolute path, or `off`** | a path relative to the configuration directory | Relative to what would be a guess; a sound the user picked is a file they can name. | A sound moved with its folder is lost. |
| **On or off, and a volume, are the user's** | the theme's | A theme cannot make the desktop louder, or switch its sounds back on. | -- |
| **The events by the naming specification's names** | SlateOS's own | A theme from elsewhere sounds right here, event by event. | -- |
| **A notification sounds as it arrives**, unless focus assist silenced it or its program's rule turns its sound off | only when its toast pops up | The rule's *sound* and *banner* are separate switches, and a sound with no banner (hear it now, read it later) is a choice a user can make. A notification added to an open pane is easily missed. | An open pane chimes for a card the user may be looking at. |
| **A low-priority notification makes no sound; an urgent one the warning** | one sound for all | Low priority asks to be seen when looked for; urgent asks for attention now. | -- |
| **Sign-in sounds when someone is let in** -- at start only when no login screen is up, else when a password is accepted -- **and sign-out when the login screen comes back** | at start | A sign-in sound over a login screen nobody has signed in to says something false. | -- |
| **Playback is the process's switch** (`event_sounds::allow_playback`), turned on by the `desktop` binary alone | `cfg!(test)` | A test-only `cfg` is off only in the crate's own unit tests: its integration tests, another crate's, and a harness would all sound on the machine running them. | A program that builds a shell and wants it heard must say so. |
| **The events a settings page offers are listed in the appearance crate** (`SHELL_EVENTS`, with labels), held to what the desktop sounds by the desktop's tests | in the desktop | The page is another program's (lane E's Settings), and both read the appearance crate. | -- |

### What is not done

- The page in Settings that chooses all this (lane E,
  `requests/c-e-a-sounds-page-for-the-sounds-axis.md`).
- Hearing it on SlateOS: the kernel's door to the device, and the mixer
  emptied into a sound card.
- A way for a *program* to ask the desktop to sound an event
  (`roadmap-detailed.md`'s `audio.system_sound`), and the `ui.theme.sounds`
  capability that would let a program change the sound theme: only the
  user's own file chooses it today.
