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
changes nothing anyone hears.

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

### What it does not do

- **Per-program volume, the output device, the volume flyout**
  (`roadmap-detailed.md` §3.4): the kernel keeps per-program entries
  (`fs::soundmixer`) with no interface to programs; the master is what the
  control device offers.
- **A volume icon in the tray**: the pane, the keys and the overlay are the
  controls there are.
