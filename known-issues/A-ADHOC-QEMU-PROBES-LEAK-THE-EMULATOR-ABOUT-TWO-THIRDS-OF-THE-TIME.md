## A-ADHOC-QEMU-PROBES-LEAK-THE-EMULATOR-ABOUT-TWO-THIRDS-OF-THE-TIME (lane A, 2026-08-18) - **fixed by `scripts/qemu-probe.py`; read this before hand-rolling a probe**

**In short:** the one-line trick everyone uses to ask QEMU "do you support this
device?" leaves the emulator running forever about two thirds of the time. Four
such leftovers were found on this machine still alive 16-21 hours later, and one
of them had been spinning the whole time, stealing CPU from the benchmark suite
that runs on the same machine. It is now fixed: use `scripts/qemu-probe.py`
instead of writing the one-liner by hand.

### The idiom that leaks

```
printf 'info pci\nquit\n' | qemu-system-x86_64 -monitor stdio -device ...
```

### Why it leaks - it is a race, not a forgotten `quit`

This is the part that makes it worth writing down, because the idiom *looks*
correct and spot-checks fine. The pipeline hands QEMU the bytes and then
immediately closes the write end of the pipe. QEMU's stdio monitor sees the
commands and the EOF at essentially the same instant. If the EOF wins, the
character device is torn down with the queued `quit` **still unexecuted** - the
monitor is gone, nothing else will ever ask the VM to stop, and QEMU runs until
the machine reboots.

Measured on QEMU 11.0.93, same command each time:

| stdin handling | outcome |
|---|---|
| closed immediately (the naive idiom) | **4 of 6 runs wedged** |
| held open, `quit` sent after a 0.4 s settle | **0 of 4 runs wedged** |

Two corollaries that cost real time when they are guessed at instead:

- **The leak was not caused by the bad `-device` spelling being probed.**
  QEMU only *warns* on an unknown `ati-vga` model (`warning: Unknown ATI VGA
  model name, using default rage128p`) and carries on, so those runs would have
  exited cleanly. The device argument is a red herring.
- **`Popen.communicate()` inherits the bug**, because closing stdin is exactly
  what `communicate` does. Calling `proctree.run_captured` - the repo's own
  "the timeout is real" helper - therefore does **not** save you here. It
  contains the damage (the Job Object still kills the tree on the deadline) but
  every probe then costs the full timeout and reports a spurious "did not exit".

### Why they were still alive 16-21 hours later

Under MSYS, `kill "$!"` uses the Cygwin PID and does **not** reliably
`TerminateProcess` a native Windows `qemu-system-x86_64.exe`. This is already
documented at `scripts/boot-test.sh:565`, where the boot path solves it properly:
QEMU writes a `-pidfile` and `kill_qemu()` `taskkill`s the real Windows PID.
Ad-hoc probes had no such protection.

### Why it matters beyond tidiness

One orphan (`-device ati-vga,model=xyzzy -display none`) had accumulated **729
seconds of CPU**. A permanently-spinning process on the host is precisely the
background contamination that
`A-BENCH-THE-HOST-IS-A-DESKTOP-SO-A-SINGLE-RUN-IS-NEVER-A-VERDICT` describes and
that `bench-history.py` spends its effort trying to detect. It is not a plausible
explanation for the large movements documented there - the desktop applications
dwarf it - but it is real, it is ours, and unlike Chrome it is avoidable.

### The fix

`scripts/qemu-probe.py`. It never closes stdin before the guest has exited, and
wraps the run in `proctree.Tree`, so a Job Object with `KILL_ON_JOB_CLOSE` tears
down QEMU and any descendants on the deadline, on Ctrl-C, or if the script itself
dies. The settle makes the common case exit in ~0.5 s; the job is what makes it a
*guarantee*. It also replays the monitor's per-character terminal echo back into
plain text, so the output can actually be grepped for a BAR address.

```
python scripts/qemu-probe.py --device ati-vga,model=rv100 -- "info pci"
```

**If you are about to hand-roll a QEMU invocation that is not the boot test, use
this instead.** If it genuinely cannot be used, at minimum run through
`scripts/run-timeout.py` so the Job Object still bounds the damage.
