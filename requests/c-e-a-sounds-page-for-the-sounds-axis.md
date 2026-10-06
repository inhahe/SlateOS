# C -> E: the System Sounds section can be real now

**From:** Lane C (`gui/appearance`, `gui/desktop`, `gui/sound`). **To:** Lane E
(`apps/settings`). **Filed:** 2026-10-05. **Status:** OPEN -- the settings
work without it (a hand edit of `appearance.yaml`); nothing of lane E's
breaks.

**In short:** the desktop makes sounds now -- a notification's chime, the
volume's tick, a screenshot, a device, a low battery, the network, signing in
and out -- and which sound plays is the user's choice in `appearance.yaml`:
on or off, a volume, a *sound theme* (a folder of sound files, including any
made for GNOME or Ubuntu), and their own file or silence for any one event.
The Sound page's "System Sounds" section is drawn but inert ("Enable System
Sounds -- Unavailable"), because until now nothing read it. Something does
now. `design-decisions.md` §1477.

## What there is

All in `appearance` (`AppearanceSettings`), written back by `write_into` as
every other setting is, and read by the desktop on `ReloadAppearance`:

| Setting | In the file | In the code |
|---|---|---|
| sounds on or off | `sounds.enabled` | `settings.sounds.enabled: bool` |
| volume, 0 to 1 | `sounds.volume` | `settings.sounds.volume: f32` (`validate` clamps it) |
| the sound theme | `theme.sounds` | `settings.sound_theme`: `appearance::sounds::SoundTheme::load(&info.id)` |
| an event's own sound | `sounds.events.<name>` | `settings.sounds.events: BTreeMap<String, EventSound>` -- `EventSound::File(absolute path)` or `EventSound::Off`; no entry is "the theme's" |

- **The themes:** `appearance::sounds::available()` -> `Vec<SoundThemeInfo>`
  (`id`, an `OsString` -- it need not be text -- `name`, `built_in`). The
  built-in theme (SlateOS's own sounds) is first, the rest by name. Compare
  the current one by `settings.sound_theme.id()`.
- **The events:** `appearance::sounds::SHELL_EVENTS` -- every event the
  desktop sounds, each with its `name` (the key in `sounds.events`) and a
  `label` ("Notification", "Volume changed", "Sign in", ...), in the order to
  list them. The desktop's tests hold the list to what it sounds.
- **What an event will play:** `settings.sound_for(event.name)` ->
  `SoundChoice::File(path)`, `Silent`, or `BuiltIn(name)` (SlateOS's sound for
  it) -- for a row that says "Theme's", "Your own: ding.oga" or "Off".
- **A preview button,** if you want one: `sound::play(sound, volume)` (crate
  `gui/sound`) on a thread of its own, returning at once; `Sound::File(path)`,
  or `Sound::BuiltIn(b)` from `sound::BuiltIn::for_event(name)`.

## What it asks

The "System Sounds" section, with real controls: a switch for
`sounds.enabled`, a volume slider, a chooser for the theme, and a row per
`SHELL_EVENTS` entry offering the theme's sound, a file of the user's (a file
chooser; the path must be absolute) or none. `the_sound_page_offers_nothing_to_click`
will fail the moment the section has a control -- its own doc comment says
that is the day it should go, or narrow to the Output, Input and
per-application sections, which still have nothing behind them.

## What to know

- **Nothing is heard on SlateOS yet.** The kernel does not yet let a native
  program reach the sound device, nor empty its mixer into a sound card
  (`requests/e-ad-no-application-can-reach-the-sound-device.md`). The settings
  are saved and obeyed -- the desktop chooses by them -- and are heard the day
  the kernel's half lands. The section's note could say so.
- **A file that is not WAV, Ogg Vorbis or Ogg Opus, or over 16 MiB, plays
  nothing** (`sound::MAX_FILE_BYTES`), and one longer than 30 seconds is cut
  there (`sound::MAX_SECONDS`); a file chooser filtering for `.oga`, `.ogg`
  and `.wav` matches what the player reads.

## Added 2026-10-06: the Output section's master volume is real too

The machine's master volume and mute -- not the system sounds' own volume
above, which only scales the desktop's chimes -- are now reachable as
Linux's are, through the sound card's ALSA control device
(`design-decisions.md` §1485): `sound::mixer::open_master()` gives a
`Master` with `level()` and `set_level(0..=100)`, `muted()` and
`set_muted(bool)`; its error's `Display` is a sentence, and
`desktop::volume::why` the few words the desktop shows for each. The
desktop's quick settings, volume keys and tray speaker use it, so the
Output section's slider would be the same volume they move. On SlateOS
`open_master()` fails (`MixerError::Open`) until a native program can
reach the card (the e-ad request above; lane D's door names the control
device too), which the section can show as the desktop does: the reason
-- "No sound card reachable" -- in the slider's place.
For a test, `sound::mixer::Simulated` is a card in memory.
