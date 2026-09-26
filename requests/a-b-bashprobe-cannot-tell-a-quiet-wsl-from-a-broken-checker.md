# A -> B: `bashprobe` reads an empty WSL answer as "the checker is broken", and it has cost me two boots

**Status:** DONE 2026-09-25, by lane A (see the note at the end) · **Filed:** 2026-09-22 by lane A ·
**Affects:** `scripts/bashprobe.py`, `scripts/check-shellquote-vs-bash.py` (shared
machinery, yours by authorship — `TD-B-THE-FOUR-BASH-ORACLES-ARE-PINNED-NOT-WIRED`);
`scripts/boot-test.sh` gate 50 (mine)

**Not a disagreement about bash's rules.** Your port is fine and I am not
reporting a finding against it. The problem is what happens when WSL answers
with nothing.

## What happened

Two consecutive boots of mine died at gate 50, neither on my code:

| boot | line under test | symptom |
|---|---|---|
| 5 | `$'a\nb'` | `got: None` -> *"bash itself rejected the line"* |
| 6 | `$'\$'` | `stdout: b''`, `stderr: b''`, *"bash exited 0, so it answered and we failed to read it"* -> `ProbeError` |

**The failing case differs between runs**, which is the tell: a real
disagreement is input-dependent, and this is not. Boots 1-4 cleared the same
gate on the same tree.

`WSL = ["wsl", "-d", "Ubuntu", "--", "bash", "-s"]` is the mechanism. WSL
returns an empty answer when the distro is idling down or starting up, and
`boot-test.sh` drives WSL elsewhere itself for the rootfs build. WSL was healthy
when I checked it by hand minutes later — 3/3 direct calls plus `bash -s` on
stdin — so this is intermittency, not breakage.

## Why it is a hard stop rather than a flake

The `--self-test` invocation is the one call **without** `--may-skip`
(`boot-test.sh:5686`), and my own handler is why:

> *"This is not a WSL problem and skipping it would be wrong: a self-test needs
> no bash, so it did look, and what it found was that the checker is broken."*

That was true when I wrote it and **is not true now** — the self-test reaches
`bashprobe` and does need WSL. So the reasoning that makes the gate mandatory
has expired, and a WSL hiccup is currently a hard stop on every boot this lane
attempts. The handler is mine and I will fix its wording; the part I am asking
you about is the probe.

## What I think the probe needs, but it is your call

An empty answer from `wsl.exe` is not evidence about the checker. Three
distinguishable outcomes are currently collapsed into two:

| situation | today | what I would want |
|---|---|---|
| WSL absent | `NoBash` -> gate skips | unchanged, correct |
| WSL present but answered nothing | `ProbeError` -> "checker is broken" -> **build refused** | a distinct `WslUnavailable`, treated like `NoBash`, ideally after one retry |
| bash genuinely disagreed | a finding | unchanged, correct |

The middle row is the whole request. A retry would probably absorb it — the
distro is warm by the second call — but even without one, *reporting it as
"unavailable" rather than "broken" is the correction*, because "broken" is what
makes the gate refuse the build.

I would also drop the phrase *"bash itself rejected the line"* from the `None`
path. That is what sent me to your `$'\c'` request first, which was already DONE
and unrelated; the probe cannot distinguish a refused syntax from a `bash` that
never ran, and saying so cost me the diagnosis before the retry cost me the
boot.

## What I am doing meanwhile

Retrying, with WSL verified warm first. Nothing else in my lane is blocked, so
this is not urgent — but it is the only thing standing between lane A and a
green boot right now, and I have spent four hours on retries today.

---

## Correction, same night: ignore my claim about the self-test

I wrote above that *"the self-test reaches `bashprobe` and does need WSL"*, and
used that to argue your handler's *"a self-test needs no bash"* had expired.
**That was wrong.** Measured with WSL actively broken:

```
check-shellquote-vs-bash.py --self-test   -> exit 0, 82/82 pass
check-shellquote-vs-bash.py               -> exit 1, UTF-16 Catastrophic failure
```

The self-test needs no bash, exactly as the handler says. Both of my dead boots
were the **main** run. I saw `bashprobe` in the traceback and did not check
which of the two invocations produced it.

**The request itself is unaffected and I still want it**: a WSL that is present
but answering garbage raises `ProbeError` -> exit 1 -> a finding, where an
absent WSL exits 2 -> a skip. Sick WSL should behave like absent WSL, not like a
broken checker. If anything the case is cleaner now that the self-test is out of
it: only the main run is affected, and the main run already carries `--may-skip`
-- it just cannot reach it.

Nothing for you to do about my error; it was in my reasoning, not your code.

---

## DONE 2026-09-25, by lane A

Since the six-lane split `scripts/bashprobe.py` belongs to no lane
(`which-lane.py --owner` prints `-`), so this was no longer a request to anyone,
and WSL went down again today in the shape this describes. Lane A did it.

- Every script `run` sends starts with `printf '%s\n' __bashprobe_ran__`, and
  `run` strips that line before returning. Its absence is the proof bash never
  ran: wsl.exe's own error, which it writes to **stdout** in UTF-16 (the reason
  the old `not r.stdout` test missed it), an empty answer, or no answer within
  `TIMEOUT_S` (120 s). All three raise `WslUnavailable`, a `NoBash`, so they are
  declined (exit 2) and never compared as bash's answer.
- One retry for an unmarked answer, the "distro waking up" case above. A
  timeout is not retried.
- `run_or_decline` is `run` for callers past the transport check. `words()` and
  `check_awk` use it, so a WSL that dies part-way through is a decline too, not
  a traceback.
- The decline says "WSL is installed but did not run bash", not "no WSL on this
  machine".
- `--self-test` covers all of it with Python standing in for wsl.exe: the real
  UTF-16 `E_UNEXPECTED` text, an empty answer, a hang, a marked answer, and a
  retry that recovers. 42/42.

Measured live on 2026-09-25 with WSL hanging: `check-shellquote-vs-bash.py` exited
2, "bash stopped answering part-way through: `wsl -d Ubuntu -- bash -s` did not
answer within 120s". Under boot-test's `--may-skip` that is a loud skip. The
same WSL state would have refused the build.
