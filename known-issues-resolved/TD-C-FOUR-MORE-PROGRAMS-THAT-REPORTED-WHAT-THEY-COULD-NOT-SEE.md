## TD-C-FOUR-MORE-PROGRAMS-THAT-REPORTED-WHAT-THEY-COULD-NOT-SEE -- FIXED 2026-09-15

**In short:** a screen recorder that filed recordings of nothing, an email
client that opened on somebody else's inbox, a weather app that issued a
severe-weather warning it invented, and a startup manager that listed programs
it had never looked for. None of the four could reach the thing it described.

**Date:** 2026-09-15. **Lane:** C. Applications eleven through fourteen of the
fabrication sweep.

### `apps/screenrecorder` -- the sound recorder's bug, plus a durable record

`handle_tick` called `record_frame(self.frame_bytes())` every tick while
recording. `frame_bytes` is computed from the **resolution**, not from captured
pixels, because nothing in the crate captures pixels and nothing writes a file.
So the frame counter and byte total climbed, the floating indicator showed
elapsed time and a growing file size, and the recording light blinked once a
second.

**Then Stop filed a history entry** with a name, duration, frame count,
resolution, size and a path -- `~/Videos/Recordings/recording_20260518_120000` --
and `App::new` pre-loaded two more, so the window already opened on a 1.0 GB
"Desktop Recording".

That last step is the difference from `soundrecorder`, where the loss at least
announced itself the moment you went looking for the audio. **Here the user is
left with a list, with sizes, that survives the session.** They may act on it
days later by freeing space elsewhere, or by sending someone the path.

Refused at the *start* of the take rather than reported during it, for the same
reason as the sound recorder: by the time a running take reports trouble, the
event it was pointed at has already happened.

### `apps/email` -- and the one structural difference that mattered

Opened on an account for `user@gmail.com` in the name of "John Doe", an inbox
of messages nobody had received, and a filter rule acting on them. **The
account is the part worth singling out**: an empty inbox is at worst ambiguous,
but a *configured account* is read as credentials being stored and a server
having been reached -- a claim both about the user's setup and about what this
program has been handed.

The crate's own source already admitted the problem. A comment in its event
section notes that about twenty IMAP and SMTP command builders "have nowhere to
send a string because this tree has no network". **The protocol builders were
honest about it and the window was not.**

**This is the only one of fourteen applications where removing the fabrication
broke no tests at all** -- 73 passed before and after. The reason is structural
and worth keeping: **its seeding lived in `main()`, not in `new()`.** Every
other app wired the fixture into the constructor, so every test received
invented data without asking. Here the tests had to ask, and still can. The
same fabrication, one call frame higher, costs nothing to remove.

### `apps/weather` -- the alert is worse than the forecast, and not for the obvious reason

`WeatherApp::new` filled an observation, an hourly forecast, a daily forecast,
a saved location of "New York, NY" marked as the user's default -- a claim about
where they are -- and this:

> **Thunderstorm Watch.** Thunderstorms expected this afternoon. Stay alert.

A wrong forecast is wrong for a day. But **a weather app is the only program in
this sweep carrying a channel whose entire purpose is to make somebody change
their plans for safety**, and a fabricated warning teaches the user that this
app *has* such a channel. So its silence tomorrow reads as "no warnings in
force" rather than "not connected".

**The false alert is a one-day problem; the false confidence in the channel
outlives it, and it fails in exactly the situation it was trusted for.** This is
the sharpest case of absent-versus-empty in the whole sweep, because here the
dangerous state is the *empty* one, and it arrives later -- when nobody is
looking at the change that caused it. The banner says so outright: *"It cannot
deliver severe-weather alerts either. Silence here is not an all-clear."*

`current` became `Option<CurrentWeather>`. As a bare struct the only answer it
could give was invented, and a default renders as **0 degrees and Clear** --
not a blank, a plausible winter reading. Same defect and same fix as
`soundrecorder`'s `available_bytes: u64` a few hours earlier. Only three
functions read it, so threading it through as a parameter was cheaper than
twenty-one `unwrap_or_default()`s and makes the absent case *unrepresentable*
in the drawing code rather than merely handled.

### `apps/startupmanager` -- and the class these two belong to

Opened on System Tray at `/usr/bin/systray`, Network Manager at
`/usr/sbin/networkd --daemon` and the rest, each with a path, arguments, a
vendor, a description and an impact rating, none of it read from anything. The
crate's only `std::` import beyond the toolkit is `process::ExitCode`.

**Every entry was benign, and that is the harm rather than a mitigation of it.**
A startup manager is an audit tool: people open it to find out what runs at
login that they did not put there. An invented list cannot contain that thing.

**Together with `devicemanager`, this names a class worth carrying forward: for
a diagnostic tool, invented data is worse than it is for a display tool,
because what the user is looking for is an *absence* or an *anomaly*, and
fabricated data is neither. A wrong weather forecast is a wrong fact. A wrong
startup list is a wrong conclusion.**

Enable and Disable flipped a flag in this process. **Disable is the
consequential half** -- somebody disables a startup entry because they do not
want it running, often because they do not trust it -- and greying the row out
while the program still launches at every login is `devicemanager`'s Uninstall
again: the user marks the thing as dealt with and stops watching it.

### Two process notes from these four

**A banner drawn before the background is not drawn.** In `screenrecorder` I
pushed the explanation at the top of `render_commands`, ahead of the background
`FillRect` that would have painted over it. The only reason it was caught is
that `test_render_produces_commands` asserts the first command is the
background. Third render-ordering mistake of the sweep, second one a test found
rather than me.

**Fixture placement predicts the blast radius exactly.** Thirteen of fourteen
apps wired their fixture into the constructor and broke 6 to 66 tests each;
the one that wired it into `main` broke none. The fix is identical either way,
so the cost is entirely in where the call sat.
