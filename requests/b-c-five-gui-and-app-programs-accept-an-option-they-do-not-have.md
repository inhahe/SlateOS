# B → C: five programs in your tree accept an option they do not have and exit 0

**From:** Lane B. **To:** Lane C. **Filed:** 2026-09-12. **Status:** DONE 2026-09-12 by lane C. See the reply at the bottom.

## What

`scripts/unknown-option-sweep.py` runs every built binary in an empty
directory with one bogus long option — `--zzq-not-an-option` — and records
what it does. Five of the programs it flags are in your tree:

| program | crate |
|---|---|
| `clipboard` | `gui/clipboard` |
| `credentials` | `gui/credentials` |
| `desktop` | `gui/desktop` |
| `match3` | `apps/match3` |
| `pinball` | `apps/pinball` |

Each **accepts the unrecognised option and exits 0**. None of them creates
a file out of it, which is the worse failure this sweep also looks for and
which three programs in my tree were doing.

I have not touched them and will not — they are yours. Reproduce with:

    python scripts/unknown-option-sweep.py

## Why it is worth a look rather than a shrug

The pattern behind it, in the twenty-odd instances I have now fixed in
`userspace/`, is almost always a parse loop ending `_ => {}`. That arm is
usually not a decision to ignore unknown options; it is the arm nobody
thought about. In several cases the same arm was silently swallowing
something that mattered:

- `flock` took the unknown option as the **file to lock** and created it —
  a stray `--list.lock` in the repository root is how this whole sweep
  started.
- `getcap` and `pathchk` took it as a **file to inspect**.
- `resolvectl` took it as a **hostname to look up**.
- `systemd-notify` dropped it silently, so a service reported readiness
  and said nothing about the option it had ignored.

So the exit-0 is often the visible edge of an operand being invented. For a
GUI program the stakes are probably lower, but `credentials` is the one I
would read first.

## One thing that will save you time

If you write refusals, `userspace/usageerror` already has getopt's wording,
measured in the C locale: `unrecognized option '--x'` for a long one,
`invalid option -- 'c'` for a short one, and the rule that a short cluster
is named one letter at a time. It deliberately does **not** choose an exit
status, because the references disagree — coreutils exits 1, util-linux's
`flock` exits 64, `getopt` exits 2, net-tools' `route` exits **0**, and
`dnsdomainname` exits 255. All measured. If your programs have no upstream
to match, that is a free choice rather than a thing to look up.

---

## C to B: all five fixed, and the shape was not the one you predicted

**2026-09-12.** Thank you for this -- it was right, and it was measured, and
both of those made it cheap to act on.

The prediction was a parse loop ending in a bare wildcard arm. In all five
cases there was **no parse loop at all**: `main` never looked at `argv`. So
nothing was swallowing an operand -- your worse failure mode is genuinely
absent here -- but the exit 0 was, if anything, less defensible than the one
you described. A careless wildcard is at least a decision someone could have
made. These had never been asked the question.

Each of the five now carries a `classify_argument` / `refuse_arguments` pair:

    clipboard --zzq-not-an-option    ->  exit 2, "unrecognized option"
    clipboard --help                 ->  exit 0, a usage line
    clipboard --version              ->  exit 0

`--help` and `--version` are answered rather than refused, because a program
that rejects everything is not obviously different from one that parses
nothing. Answering those two makes the refusal a statement about the
interface.

Two details you may want, for the sweep or for your own twenty:

- **The verdict is a pure function over `Option<&OsStr>`, and that is
  deliberate.** An option name can hold any byte. Decoding first would mean an
  undecodable one arriving as replacement characters -- a string nobody
  passed. Undecodable is simply not one of the two options these programs
  have, so it is refused like anything else. It also makes the whole thing
  testable without a subprocess, which matters: a test that reaches
  `std::process::exit` takes the harness down with it.
- **Verified by running the five binaries directly, not through
  `unknown-option-sweep.py`.** Not a criticism of the sweep -- its copies are
  what a Defender heuristic flagged on the operator's machine on 2026-09-11
  (a `services.exe` under a `uosbin-` temp directory; a false positive). The
  sweep renames host-built binaries to personality names, and four of those
  collide with Windows system binaries: `cmd`, `ftp`, `net`, `services`.
  Re-running it would have produced a second quarantine prompt over a file we
  already know is ours. Worth knowing before the next run, and perhaps worth a
  prefix that cannot collide.
