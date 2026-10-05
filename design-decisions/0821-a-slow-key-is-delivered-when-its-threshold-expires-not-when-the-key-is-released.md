## 821. A slow key is delivered when its threshold expires, not when the key is released

**Date:** 2026-09-07. **Lane:** C.
**Decided by:** Claude (autonomous). Mechanism rather than policy, but it
changes how typing feels for the people who turn the feature on, so it is
recorded rather than just written.

**In short:** "Slow keys" is an accessibility setting that ignores keys tapped
by accident: a key only counts if you hold it for a moment first. The question
is *when the letter appears*. It can appear the instant you have held the key
long enough — while you are still holding it — or it can wait until you let
go. The first is what every other system does and what this will do. The
second is much easier to build here and feels wrong: nothing happens while you
hold the key, and the letter arrives on release.

**The problem.** `Compositor::handle_key` is called on key *down*, and at that
moment nobody knows how long the key will be held. The state machine
(`a11ykeys::FilterKeys::on_press`) answers "should this count, given a hold
duration", and deliberately does not say where that duration comes from.

**Option A — deliver on release.** Measure the hold when the key comes up; if
it met the threshold, deliver the press then.
*What changes:* letters appear when you lift your finger rather than when you
press. Cost: it is not really slow keys. Key repeat becomes impossible (a
repeat is by definition something that happens while the key is still down),
and every keystroke is delayed by however long the user happens to hold it,
which for this user is a long time. Free to implement — no timer, no
scheduling.

**Option B — deliver when the threshold expires, key still held (chosen).**
On press, hold the keystroke back and note the deadline. When the deadline
passes with the key still down, deliver it and let normal repeat follow. If
the key comes up first, drop it — which is exactly the accidental tap the
feature exists to discard.
*What changes:* the letter appears a fixed moment after you press, and then
behaves like any other key. Cost: the compositor has to wake at the deadline.

**Why B, given A is free.** A is not a cheaper version of the feature, it is a
different and worse one. X11's AccessX, Windows and macOS all do B, so a user
who has used slow keys anywhere else would find A broken. And A silently
removes key repeat from anyone who enables it, which is a second accessibility
regression handed to the group least able to work around it.

**The part that made this worth writing down: the idle backoff.** §-numbered
work earlier added `IdleBackoff` to `compositor::server`, which stops the
compositor spinning when nothing is happening: after `SETTLE_TICKS` of quiet
it polls every `IDLE_INTERVAL` = **100 ms**. A key waiting out a slow-keys
threshold is, to that mechanism, perfect quiet — no input, no damage, nothing
to draw. So the naive version of B delivers the key up to 100 ms late, and by
a different amount each time, on top of a threshold that defaults to 300 ms.

The fix is to count a pending keystroke as activity, so the backoff cannot
engage while one is outstanding. Jitter then falls to one frame. It is a small
change and the alternative — teaching the backoff to wake at a specific
deadline — is more machinery for the same result, since a keystroke is
pending for at most a few hundred milliseconds and the compositor was about to
be busy anyway the moment it lands.

**What this does not decide.** Bounce keys needs none of this: it looks only
at the past, so it can answer on the key-down that arrives. It is implemented
in the same state machine and is not waiting on any of the above.
