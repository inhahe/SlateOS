## TD-C-THE-MUSIC-PLAYER-PLAYS-NOTHING-AND-DRAWS-A-VISUALISER-OF-IT -- FIXED 2026-09-15

**In short:** press Play in the music player and the button changes to Pause,
the elapsed time starts counting, the progress bar fills, and the Now Playing
panel animates a bar visualiser. No sound is produced, because nothing in this
operating system can produce sound. The bars are random numbers eased over
time.

**Date:** 2026-09-15. **Lane:** C. Not yet fixed; scoped below.

**There is no audio anywhere in this tree.** The string `audio` appears in
exactly one `Cargo.toml` — `apps/musicplayer`'s own, in its description:

    description = "SlateOS Music Player — audio playback with playlist management"

There is no audio driver in `kernel/src/drivers/`, no audio service in
`services/`, no codec, no mixer output path, no PCM anything. `apps/mixer`
exists and is a set of sliders. So this is not a program waiting on a
subsystem that is nearly ready; it is a program named for a capability the
project does not have.

**What makes this its own entry rather than another fabrication.** The other
apps repaired today claimed a *completed act* — "Renamed 6 files", "Connected
successfully", "Killed process 4821". This one claims a **continuous present**:
it is playing, right now, and here is how far through. That is harder to
disbelieve, because a static claim can be checked and a running clock looks
like evidence of itself. A user who sees 1:04 of 3:47 and a moving waveform
concludes their speakers are muted.

**The visualiser is the sharpest part, and someone has already been here.**
`advance_visualizer` carries a long and genuinely good comment explaining that
the bar heights used to be `((position_secs * 10) as u32 * 31 + i * 7) % 100`
— an arithmetic progression that drew a sliding diagonal staircase rather than
a waveform, identical on every machine and for every track. That was fixed.
The fix made the bars **random, eased with a time-constant**, so they now look
convincingly like audio levels. The repair improved the fabrication.

That is worth stating plainly because it is the third time today the same
shape has appeared: `apps/vpnmanager` had its invented byte counters removed
and its "Connected successfully" left in place; `apps/sysinfo` had its
fabricated hardware readings removed and its lying controls left; here a
fabricated waveform was replaced with a better-looking fabricated waveform.
**A fabrication has two halves — the numbers and the claim — and improving the
numbers without touching the claim makes the program more convincing and no
more honest.**

**The fix, and it is the vpnmanager shape.** The app is a playlist manager
that cannot play. Keep what is real and stop asserting what is not:

1. `toggle_play` reports that there is no audio output on this system, and
   does not enter a playing state. `position_secs` does not advance and the
   progress bar does not fill, because both are measurements of a thing that
   is not happening.
2. The visualiser is not drawn. A panel of moving bars is a claim about sound
   in the room; there is none. An empty panel saying why is the honest
   version.
3. `load_m3u` gets a door — it takes a `&str` and has no caller outside the
   tests, so a playlist cannot be loaded either. It is one of the 40 stranded
   serialisers. A playlist manager that can import an `.m3u` and export one is
   a real and useful program, and that is what this becomes.
4. The crate description stops promising audio playback.

**Done in `12e3c7848`**, in the four steps below. The migration was smaller
than expected: most tests set `playing` directly to exercise the advance
machinery, and only the `toggle_play` ones pinned the user-facing claim -- the
same split `apps/vpnmanager` had, where twenty-one tests reached a connected
state through the claim itself and the machinery underneath was fine.

**What was filed as the reason to defer:** the playing state is threaded through
the window — row highlighting, the transport buttons, the tick, the
visualiser — so it is its own commit with its own test migration, of the same
size as `vpnmanager`'s twenty-one. Nothing about it is blocked, and the
ordering above is the order to do it in.
