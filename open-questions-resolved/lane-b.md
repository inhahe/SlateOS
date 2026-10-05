## Resolved — lane B

- B-Q8 through B-Q21 (not B-Q15) -- answered by the operator on 2026-09-27
  in lane F's session and relayed verbatim to lane B; each is written up in
  `design-decisions.md` with the operator's words:
  - B-Q8 Which width table? -- (§1042) the operator left the table to Claude,
    which took GNU's (gnulib, Unicode 15.1.0); SlateOS's terminal is to answer
    a width query too (requested from lane C), and the operator's worry about
    glyphs that disagree with the table is answered there.
  - B-Q9 Keep our shell or switch to genuine Oils? -- (§1043) **genuine Oils
    becomes the default**; the Rust OSH is kept as a fallback, not deleted.
  - B-Q10 grep's `-P`: manual or program? -- (§1044) **the manual**: a match can
    belong to any number of windows. Fixed the same day in the operator's
    `grep.py` and `grep.cpp` and in ours.
  - B-Q11 169 names inside other programs -- (§1045) Claude's recommendation:
    case by case, and a kept name is installed as the same file.
  - B-Q12 Should `osh` quote names? -- (§1046) no change: genuine Oils is to be
    the default, so a toggle in ours is not worth having.
  - B-Q13 Randomness shapes; the effort rule's home -- (§1047) Claude's
    recommendation: a userspace library, apart from cryptographic randomness;
    the rule stays in the parent `CLAUDE.md` only.
  - B-Q14 Which `logger` survives? -- (§1048) Claude's recommendation; the tree
    had already arrived there -- util-linux's logger, ported, is the survivor.
  - B-Q16 Record §1005 and §1006? -- option 1. Both entries existed (written
    2026-09-07; the question searched for the wrong heading format), so
    nothing was missing to write.
  - B-Q17 Delete sbctl's refusing commands? -- (§1049) **option 2**: the four
    that need RSA/X.509 are deleted; `enroll-keys` and `reset` wait for lane A.
  - B-Q18 Which large port next? -- (§1050) Claude's recommendation, the fastpy
    compiler; the operator's further ports (Mono, Xonsh, Nushell on SlateOS,
    their WinDirStat fork, a debugger, a LithicBackup reimplementation) are now
    on the roadmap.
  - B-Q19 A standing rule for search-and-replace edits? -- (§1051) **yes, both
    habits, in the `CLAUDE.md` of all three accounts** -- to be applied when
    the operator confirms in lane B's own session, since the answer came by
    relay.
  - B-Q20 `shred --random-source` -- (§1052) the premise had gone: `shred` is now
    a port of GNU's with the option working; the one detail the operator
    proposed differently (restart a source file each pass) is kept as GNU's
    and put back to them.
  - B-Q21 Most programs never reach the image -- (§1053) **stage everything that
    builds for now** (lane D's recipe), then a catalogue of every program and
    options for what the OS is for; a program is recorded where every lane
    looks.
- B-Q7 Which copy of the command-line tools is canonical, after the premise
  behind June's §8 turned out to be false? — resolved 2026-09-07 (§1005,
  `Decided by: Operator`): **B, `coreutils` is the one home.** The better half
  of each of the 41 duplicate pairs survives inside it; the duplicate crate is
  deleted; the 45 bundle-only names stay put rather than becoming 45 crates.
  The operator noted that the "dependency shape that exists nowhere in the tree
  yet" bullet reads like an effort argument and would carry no weight if it
  were one — it is an architectural argument (option A cannot be reached
  without a per-tool crate importing the bundle, the shape §8 set out to
  retire), and B wins on the other reasons regardless. §8 superseded, §359
  un-suspended.
- 2,288 of the 2,756 commands in `userspace/` report success for work they
  never did — which ones do we keep? — resolved 2026-09-07 (§1006,
  `Decided by: Operator`): **stricter than my option A — delete every
  fabricating command, not only the ones that can never work.** A name that
  could be ported one day is added back when it is implemented, not before,
  because a command's existence is a claim made to `command -v` probes as well
  as to people, and a refusing stub answers "yes" to the probe and fails later.
  The audit script is pinned as a ratchet once the deletion lands.
- The test machine cannot produce random numbers, on purpose, and eighteen
  tests in the apps depend on that — should it start? — resolved 2026-09-07
  (§1007, `Decided by: Operator`): **A, land it.** Lane C rewrites its eighteen
  `assert_eq!`-on-two-draws tests in its own tree; lane B files the request and
  the list rather than editing inside lane C's globs. `main` may be red in
  between, which was accepted as the lesser cost.
- B-Q5 70 compiled programs are stored in git and go stale without git
  noticing — keep storing them, or rebuild on demand? — resolved 2026-08-21
  (§355, `Decided by: Claude (autonomous)`): **B, build on demand**, against my
  own earlier "A for now" and against lane A's revised case for C. Measuring the
  arrangement rather than arguing about it settled it: the stamp gate covers
  **9 of the 70**, and **60 of the unguarded 61 were stale at that moment** — so
  drift is the steady state, not an occasional accident. C cannot reach those 61
  at all, because their compiler (fastpy) is a *different repository* whose
  revision this tree cannot record. Rebuilding every fixture costs ~65 s, and the
  kernel already `include_bytes!`s an untracked build output, so B demands no
  toolchain the tree did not already demand. B ships with the guard inverted —
  the rootfs build must refuse to stage a short fixture set, because
  `load_test_elf` self-skips and naive B would otherwise turn stale tests into
  *no* tests, silently green.
- B-Q6 Should the console login prompt obey the system-wide failed-guess
  delay? — resolved 2026-08-21 (§354): **A, and `su` joins with it.** Both obey
  the shared tally for every account including root; the delay-your-neighbour
  effect is accepted as bounded. `passwd` contributes but is never delayed,
  because it gates the remedy rather than access.
- B-Q4 Two user databases that drift apart — which one is real? — resolved
  2026-08-21 (§353): **C, one store with two faces.** `/etc/users.yaml` is the
  truth; `/etc/passwd` and `/etc/shadow` are generated from it on every change.
- B-Q3 Password hashes that can no longer be checked: fail closed, or admit
  those users once more? — resolved 2026-08-21 (§352): **A, fail closed.** Root
  runs `passwd <user>`; no authentication code is kept alive to accept a known
  non-hash.
- B-Q2 GNU's curly quotes in diagnostics, or keep straight ones? — resolved
  2026-08-21 (§351): **B, follow GNU.** Curly marks in the `invalid argument`
  family only; file names stay straight, as they are in GNU.
- Q48 Real kernel objects for "set the clock" / "bind port 80" / "raise your
  own rlimit", or leave them denied? — resolved 2026-08-21 (§350): **B, objects
  for all three.** The operator took B for the port too, where the
  recommendation had been to drop the rule; an object can express "everyone may"
  and dropping the check cannot express anything else.
- B-Q1 Which tzdata do we ship, from where, and how is it updated? — resolved
  2026-08-15 (§311): ship **full tzdata**, vendored as prebuilt TZif binaries
  and updated as a `pkg/` package.

