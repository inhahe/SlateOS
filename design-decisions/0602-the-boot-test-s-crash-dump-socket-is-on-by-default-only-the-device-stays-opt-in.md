## 602. The boot test's crash-dump socket is on by default; only the *device* stays opt-in

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When a boot test freezes, the single most useful fact is *which
instruction the frozen CPU is sitting on*. QEMU will tell us, over a small
text-control socket called the **HMP monitor** (a host-side command channel
into the emulator, invisible to the OS running inside it). Until now that
socket was only attached when you also passed `--hard-lockup-watchdog` — so on
every ordinary run, the code that reads the frozen address back was dead. A
freeze on 2026-08-25 burned its whole 900-second timeout and produced no
address at all. The socket is now attached on **every** run, with `--no-monitor`
to turn it off. The watchdog *device* stays opt-in, unchanged.

**Why the two were ever coupled, and why that was a category error.** §61
(Operator, via Q20) kept the hard-lockup watchdog opt-in for a specific reason:
it is `-device i6300esb`, and adding a device **changes the guest's PCI
topology** — the list of hardware the OS enumerates at boot. Every run would
then be booting a slightly different machine from the one we ship on, so the
default had to be "absent."

`-monitor tcp:127.0.0.1:<port>` is not a device. It adds nothing to the guest's
address space, its PCI bus, or its device tree; it is a socket on the *host*
side of the emulator, and no instruction the guest can execute observes it.
§61's rationale therefore never applied to it. The coupling was an accident of
how the flag was first written — the watchdog needed the monitor, so the
monitor was put behind the watchdog's flag — not a decision anyone made.

Checked rather than assumed: the 2026-08-25 pair of runs (one with the flag,
one without) produced no difference in harness stdout, in particular no
`(qemu)` prompt banner, because `-serial` already writes to a file rather than
to stdio.

**Why "default on" and not "on when it matters."** There is no such thing as
"when it matters" for this fault. `B-FORKEXEC-BOOT-HANG` appears roughly once
in a few dozen boots, and the run it appears on is not knowable in advance; the
2026-08-25 re-run *with* the flag booted green, which is the expected outcome
and exactly why arming after the fact does not work. **An opt-in detector for
an intermittent fault is a detector that is off when the fault happens.**

It also matters that this is the only detector that can see the specific wedge
we are chasing. The evidence in `known-issues.md` narrows it to a CPU spinning
with interrupts disabled (`IF=0`), which by construction cannot run any
in-guest diagnostic — not the timer tick, not the stall reporter, not even an
injected NMI handler if delivery itself is what broke. The emulator's own view
of the register file is unaffected by any of that.

| Option | *What changes* | Verdict |
|---|---|---|
| Leave it behind `--hard-lockup-watchdog` | nothing; the next hang is unreadable too | rejected — this is the status quo that just cost a diagnosis |
| A separate `--monitor` opt-in flag | the flag exists but nobody passes it on the run that hangs | rejected for the same reason, one step removed |
| **On by default, `--no-monitor` to opt out** | every run can report a wedged RIP; a listening loopback port exists for the run's duration | **chosen** |
| On by default, *and* default the watchdog on too | guest PCI topology changes on every run | rejected — that would overturn §61, which this does not |

**What it costs.** One `127.0.0.1` TCP port held for the life of the run, and
one call to the port picker. The picker shells out to `netsh`/`netstat` on
Windows (a second or two), so it now runs *inside* the enable test rather than
unconditionally — a run that will not attach the monitor should not pay for an
answer it discards. The port was already chosen defensively: Windows reserves
whole ranges, and a bind into one makes QEMU exit ~2 s in, which previously
wasted an entire wedge-soak.

**The one interaction worth a warning.** `--hard-lockup-watchdog --no-monitor`
arms a detector whose only output channel is switched off. That is almost
certainly a mistake rather than an intent, so the harness says so on stderr and
proceeds, rather than failing (it is legal — someone may be testing NMI
delivery itself via the in-guest handler's serial output).

**How to reverse.** Set `MONITOR_ENABLED=0` at its declaration in
`scripts/boot-test.sh`, next to `HARD_LOCKUP_WATCHDOG=0`. The two RIP-capture
call sites are already guarded on `${#MONITOR_ARGS[@]}`, so they go quiet on
their own.

**Where it came from.** `known-issues.md` → `B-FORKEXEC-BOOT-HANG`, the
`[A] RECURRENCE 2026-08-25` and `[A] The decisive narrowing` sections: *"The
next occurrence must produce a RIP, and today's could not."*
