## TD-C-A-SPEED-TEST-THAT-NEVER-SENT-A-PACKET -- FIXED 2026-09-15

**In short:** `apps/speedtest` told the user how fast their internet connection
was. It has no network access of any kind, so it had never contacted anything;
the figures were generated on the spot from a random number generator tuned to
look like a good broadband line.

**Date:** 2026-09-15. **Lane:** C.

Pressing Start ran a full latency, then download, then upload sequence, a frame
at a time, with a sweeping dial and a filling graph, and produced roughly
450 Mbps down, 120 Mbps up and 12.5 ms round-trip. All four constants were in
the source, named `SIM_DOWNLOAD_MBPS` and so on, and the word "simulated"
appears around twenty times in the file.

**It appears zero times on screen, and the screen is what the user reads.**
That is the whole finding, and it is the same axis as everything else in this
sweep: rank by what the program tells the user to believe, and a `--help` line
and a window count equally. An internal comment is not a disclosure.

**Why this ranks where it does.** It is not irreversible like the recovery and
partition tools, so it sits below them. It is above the remaining stub apps
because a speed test is *evidence in an argument with a third party*. It is
what someone checks before deciding whether the connection they are paying for
is the connection they are getting, and 450/120 is a good result -- so the
fabrication counsels **doing nothing** about a line that may genuinely be bad.
A fabricated number that flatters the status quo is acted on by inaction,
which is the hardest kind of harm to notice afterwards.

**What was kept, and why that is most of the file.** Everything that *computes*
is real and correct: minimum, maximum and average round-trip time, jitter,
packet loss, average, peak and instantaneous throughput, the rolling
twenty-entry history with its aggregates, and the text export. All of it
operates on samples handed to `LatencyTester::record_sample` and
`ThroughputTester::record_bytes`. **Those two functions are the doors a real
network stack comes through**, and they already exist and are already right.
Only the producer that was feeding them from an RNG became `#[cfg(test)]`.

This is the same shape as the `sysinfo` fix: the consumer was fine, the
producer was invented, and the honest state is a tested consumer with no
producer rather than a producer that makes things up.

**The refusal needed its own phase.** `SpeedTestPhase::Error` already existed,
and using it would have been wrong: an error is something a run *hit*, and this
is the absence of a run. Labelling it "Error" tells the user their
**connection** failed -- a claim about their network that a program which has
never sent a packet is in no position to make. The new `Unavailable` variant
reads "Not run".

**The line worth arguing for is the third one on screen:** *"No download,
upload or latency figure was produced -- they are unknown, not zero."* An empty
result strip is read as a *reading*, and for a speed test the reading it is
read as is zero, which says the line is dead. Removing a flattering untruth and
leaving an alarming one in its place is not a fix. This is the sharpest
instance so far of lane B's "no results" versus "cannot look": for a
**measurement**, the empty value is not merely ambiguous, it is a specific and
alarming number.

**What the deletion exposed, via compiler warnings.** Two things nothing had
noticed because the fabrication was holding them up:

* The app's random number generator. Production draws no random numbers now,
  because it produces no numbers.
* `ThroughputTester::num_connections`, which held the same fact as
  `connection_bytes.len()` while only the simulator ever read it. `record_bytes`
  already indexed the vector, so the count was a second copy that could
  disagree with the thing it described. Deleted.

**Fifteen tests rested on the run.** Eighth application in a row. Most moved to
a `begin_simulated_run` fixture and still cover the phase machine, which is
correct and was hard-won. **Two could not be moved, and that is a real loss
recorded in place rather than papered over:** they asserted the wiring from the
event loop to the run -- that `Event::Tick` reached the phase machine at all.
That wiring is gone with the run it carried. It was missing once before, in
August, and nothing noticed for months: the app ran its entire ten-second test
inside a single call, so no frame was ever drawn in a testing phase and
Escape's cancel could never fire. The comment left in the test file says to
restore the test the day Start does something again.
