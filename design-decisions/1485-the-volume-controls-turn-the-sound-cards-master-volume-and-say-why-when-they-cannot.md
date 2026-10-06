## 1485. The volume controls turn the sound card's master volume, through its ALSA control device -- and say why when they cannot

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the volume slider in the notification pane, the volume keys
and the mute key changed a number the desktop showed, and nothing else: no
sound got louder or quieter. They now turn the sound card's master volume
and its mute switch, asked the way Linux programs ask (`amixer`), and they
read the card's level each time they are about to show it, so a change
another program made is what they show. On SlateOS today an ordinary
program cannot reach the sound card yet -- lanes A and D are opening that
door -- so the pane says "No sound card reachable" where the slider would
be, and the volume keys say it on screen, instead of moving a number that
changes nothing anyone hears. The brightness slider beside it, which had
the same fault and has no door yet at all, now shows the screen's real
brightness and says it cannot be changed yet.

**Where:** `gui/sound/src/mixer.rs` (`Master`, `Simulated`),
`gui/sound/src/sys.rs` (the C library's `ioctl`, shared with the PCM
device), `gui/desktop/src/volume.rs` (`Output`), `gui/desktop/src/lib.rs`
(`attach_volume`, `read_volume`, `write_volume`, `show_volume_osd`),
`gui/desktop/src/notif_pane.rs` (`show_card_volume`,
`set_volume_out_of_reach`), and the `desktop` binary, which attaches the
card. Waits on `requests/e-ad-no-application-can-reach-the-sound-device.md`
(lane D's reply names the control device as part of the door).

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The card's master through its ALSA control device**: `ELEM_INFO` by name ("Master Playback Volume", "Master Playback Switch"), `ELEM_READ`, `ELEM_WRITE` | a SlateOS call or file of its own | The kernel already answers these (`alsa_control_ioctl_elem_*`), every Linux mixer asks them, and a Linux host's card answers the same: one path, tested against both. | It is reached through the same missing door as the PCM device. |
| **Read when about to be shown** -- a volume key, the pane opening | polled, or told of each change | Nothing wakes while nobody looks; the kernel's control device has no change events to subscribe to. | A level another program changed shows when next looked at -- which is when it matters. |
| **Only what changed is written** | every input written | A pointer crossing the pane writes nothing. | -- |
| **A card out of reach puts why in the slider's place**, and the keys say it on screen | a dimmed slider with the reason on hover; or the old number | The reason is there without a hover, and nothing pretends to work. | One more state of the pane's row. |
| **A level set reads back as itself** | the card's steps rounded both ways | A card of 87 steps set to 50 holds 44, which is 51 of 100; a slider moved to 50 must not jump to 51. | The level and the value written are remembered. |
| **A card with no master switch is muted by its volume** | no mute on such a card | Many cards -- HDMI's among them -- have none. | The level to go back to lives in the process that muted it. |
| **No card unless the `desktop` binary attaches one** (`Output::Own`) | every shell opening the card | A test, a harness, a picture of a theme must not turn the volume of the machine running it -- as the shell's sounds are heard only from the binary (`allow_playback`). | The binary must attach it; it does, beside `allow_playback`. |

### The brightness beside it

The pane's brightness slider had the same fault -- a number the desktop
showed, and nothing on the screen changed. The kernel reports each
display's brightness (`/proc/brightness`, a `Displays:` row each) and can
set it (`SYS_BRIGHTNESS_SET`, lane A,
`requests/c-a-brightness-has-setters-and-no-door.md`), but only for a
process holding the `SET_BRIGHTNESS` right, which nothing gives the
desktop yet (`TD-C-THE-DESKTOP-CANNOT-SET-THE-BRIGHTNESS-IT-SHOWS`). So as
the pane opens, the first display's level is read and set to itself -- a
setting no one sees, which answers whether the desktop may
(`gui/desktop/src/backlight.rs`). Where it may, the slider is the
screen's; where it may not, the row shows the level and says "Can't be
changed yet" in the slider's place -- or "No brightness control reachable"
with no report to read. A shell that asked for no screen keeps its own
slider, as for the volume. The day the desktop is given the right, the
slider comes back with no change to the desktop. *2026-10-06:* the
brightness keys -- the actions a user binds to a chord, since a laptop's
pair sends no key (`TD-C-BRIGHTNESS-KEYS-ARE-NOT-KEYS`) -- step the same
level ten points a press, the overlay showing it, or "Can't be changed yet"
where the slider would say so.

### The speaker in the tray

With a volume that is real, the tray gets the speaker every desktop has,
left of the bell (`gui/desktop/src/volume_flyout.rs`): its picture the
level's -- muted, or dimmed while there is no card to turn -- its tooltip
the level in words or why there is none, the wheel over it a volume step a
notch, and a press opening a flyout above it with the master volume's
slider, the level, and a mute switch; the arrows move the slider while it
is open, and Escape or a press elsewhere closes it. Opening it closes the
calendar, the notification pane and the start menu, and each of them closes
it. With the card out of reach the flyout says why where the slider would
be. The roadmap's popup also has an output device chooser and each
program's volume; the kernel's mixer offers programs the master alone, so
those wait on it.

### The switches above them

The same rule for the quick settings' switches. Wi-Fi and Bluetooth have
no service behind them that the desktop can ask, so they say "Not
available yet" where the switch would be and take no press
(`TD-C-THE-RADIO-SWITCHES-HAVE-NO-RADIOS-BEHIND-THEM`). And a switch that
does work joins them: **Dark Mode**, the quick toggle
`roadmap-detailed.md` asks for beside the automatic mode.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The switch shows what is drawn** (`is_light`), set wherever the appearance is | the setting alone | In the automatic mode it shows the hour's mode; a mode chosen in Settings turns it too. | -- |
| **Flipping it chooses the other mode outright**, leaving the automatic mode | flipping only until the next edge of the day | A switch that flipped and then showed the same mode until evening would look broken; this is what choosing a mode in Settings does. | A user of the automatic mode who flips it must choose "System (Auto)" in Settings to have it back. |
| **The radios' rows stay, saying why** | removing them until a service exists | The user learns where they will be; the layout does not shift when they come. | Two rows that do nothing yet. |

### What it does not do

- **Per-program volume and the output device** (`roadmap-detailed.md`
  §3.4's volume popup): the kernel keeps per-program entries
  (`fs::soundmixer`) with no interface to programs; the master is what the
  control device offers. The speaker's flyout has the master alone until
  then.
