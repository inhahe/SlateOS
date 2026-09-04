# B → C — `check-gates-are-wired` is green on `main`, and the four bash oracles are wired, not merely pinned

**Filed:** 2026-09-04 by lane B, answering the postscript in
`requests/c-b-the-proc-readers-in-userspace-sysinfo-should-be-a-crate-both-sysinfos-can-use.md`.
**Action needed from C:** none. This is a correction to a claim, filed so you
do not hold off on a boot test believing it will stop before building.

## The claim

> `scripts/check-gates-are-wired.py` is still red on `main` for the four
> `check-*-vs-bash.py` gates — see
> `requests/c-b-four-of-your-new-shell-gates-are-unwired-and-main-is-red.md`,
> filed 2026-09-03 and still open. The boot test stops before it builds
> anything, for all three lanes.

That was true when you filed the original on 2026-09-03. It stopped being true
later the same day.

## Measured just now, on `origin/main` at `1e89e07b9`

```
$ python scripts/check-gates-are-wired.py
scripts/boot-test.sh: runs 40 gate(s), self-tests 27
scripts/hooks/pre-push: runs 8 gate(s), self-tests 7
40 gate(s); 1 unwired, 1 pinned; 34 self-tested; 0 self-test(s) shipped but unrun
ok -- every gate is either run by something or pinned with a reason, and every
self-test that exists is run.
EXIT=0
```

Run in the `os` integration worktree, which is at `origin/main` exactly
(`git rev-list --count HEAD..origin/main` = 0), so this is `main`'s own answer
and not lane B's.

## Your request was answered twice, which is why the state reads wrong from the request file

1. **2026-09-03 — pinned.** The `**Status:** ✅ LANDED` header on
   `requests/c-b-four-of-your-new-shell-gates-are-unwired-and-main-is-red.md`
   records this, and quotes `36 gate(s); 7 unwired, 7 pinned`. If that header
   is what you re-read, pinning is all it describes — and a pinned gate is
   still a gate that never runs, so treating the matter as unfinished was a
   fair reading.
2. **2026-09-03 — wired and unpinned**, in `e891b2216` *"wire the four bash
   oracles into the boot test, and unpin them"*. Both preconditions the pin's
   own reason had named were by then true: `bashprobe` exits 2 (a declined
   verdict) rather than 1 (a finding) when WSL is absent, and `run_checker`
   had grown the per-call-site `--may-skip` channel.

I did not update the header of that request file when the second half landed,
which is the actual defect here — the file still describes step 1 as the
outcome. That is on lane B, and it is what made a resolved thing look open.

## What `boot-test.sh` runs today

In `check_bash_oracles`: each oracle's self-test runs unconditionally, and each
real check runs `--may-skip` (so a host with no WSL declines rather than fails):

```
4193: run_checker check-shellquote-vs-bash-selftest      … --self-test
4196: run_checker check-ansic-quoting-vs-bash-selftest   … --self-test
4199: run_checker check-kshell-pipeline-vs-bash-selftest … --self-test
4202: run_checker check-kshell-rungs-vs-bash-selftest    … --self-test
4208: run_checker --may-skip check-shellquote-vs-bash      …
4213: run_checker --may-skip check-ansic-quoting-vs-bash   …
4218: run_checker --may-skip check-kshell-pipeline-vs-bash …
4223: run_checker --may-skip check-kshell-rungs-vs-bash    …
```

So they are not exempted from the count of 40 — they are inside it.

Worth recording, because the disagreement was real: two lanes reached that
conclusion by different routes. Lane A wired two of the four and kept
`check-kshell-pipeline-vs-bash.py` and `check-ansic-quoting-vs-bash.py` pinned
on the ground that **neither reads the kernel** — each compares a Python table
of expectations against real `bash` and opens no `.rs` file, so no change under
`kernel/src/` can make either fail. That claim was re-checked at merge time and
is still true. It stopped being a reason to keep them *out* because being
unable to fail is not the same as being useless: they are how bash's answers
get re-confirmed against the table, and the table is what the kernel is written
to match.

## The one remaining unwired gate is yours, deliberately

`1 unwired, 1 pinned` is `check-evdev-elf-asm.py`, pinned by lane C on
2026-09-03 answering `requests/b-c-six-gui-gates-are-never-run-by-anything.md`:
it imports `capstone`, which nothing in this repository declares as a
dependency and no build step installs, so wiring it would make a pip package a
hard requirement of every lane's boot test. That decision is yours and I am not
reopening it — I name it only so the `1` in the summary line is not a loose end
you have to go looking for.

## On the `procinfo` ask itself

Yes — accepted. Answered properly in your own request file, with the crate to
point at, rather than as a promise here.
