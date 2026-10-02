## Resolved — lane A

- A-Q21 Seven security modules are built but nothing uses them. Staged for
  later, or believed to be working? — resolved 2026-09-27 (978): **B, wire them
  in, each fixed first.** `secureboot` gets a real fingerprint check before its
  door, and `diskencrypt` a real key derivation. `sealing`, `capsettings` and
  `secpolicy` are re-keyed by file identity before they are connected. The
  per-file metadata tables get syscall doors in the same work.
- A-Q18 Every lane appends to the end of `known-issues.md`: give it a per-lane
  seam? — resolved 2026-09-27 (977): **A, a section per lane for new
  entries.** Six `## Lane X: new entries` sections at the end; the entries
  already in the file stay where they are.
- A-Q17 Moving or scaling a video or cursor layer silently does nothing: refuse
  the request, or honour it? — resolved 2026-09-27 (976): **B, honour it.** The
  software backends compose the planes at flip time, and virtio-gpu's cursor
  uses the device's cursor queue. A rectangle a backend cannot do is refused at
  commit.
- A-Q16 One kind of kernel lock skips the deadlock checker on a premise that
  turned out false: which way? — resolved 2026-09-27 (975): **A, convert the
  locks that have another lock taken under them, then revert by measurement**
  any conversion whose cost on a hot path outweighs the check.
- A-Q15 A program can only have one network connection open at a time: which
  fix? — resolved 2026-09-27 (972): **measure A against B.** Both designs are
  built behind one switch, and a load harness decides between them. Anything
  QEMU cannot judge is deferred to bare metal.
- A-Q14 A kept copy of a file: its content from before the save, or after it?
  — resolved 2026-09-27 (971): **A, after it.** A version-history entry holds
  the file as it stands after the save that made it.
- A-Q13 One agent's push has cost another a full test run eight times in two
  days: should pushing be gated? — resolved 2026-09-27 (974): **move the fast
  checks to push time, and fix the checks that raise false alarms** before
  they are moved.
- A-Q11 Who owns `scripts/hooks/pre-push`? — resolved 2026-09-27 (973):
  **every file gets one owner, and a gate refuses a file with none.** The hook
  is lane A's. The rest of `scripts/`, `requests/`, the root crates and the
  root files get owners by the rules §973 lists, and a lane wanting a change
  asks the owner.
- A-Q20 A lane may only publish work after a green test run, and lane A's has
  been red for days on another lane's faults. What should a blocked lane do?
  — resolved 2026-09-26 (968), the operator leaving it to Claude: **publish,
  under three conditions, and only then** — every red rung is another lane's
  tracked fault, `main` already fails it for the same image, and the lane's
  own failures are zero; each publish that relies on this says so.
- A-Q10 Saving a file costs twice what it needs to: keep the automatic undo
  history? — resolved 2026-09-13 (936): **opt-in per directory AND off the
  save path.** History is off by default and enabled per directory; where it is
  on, the read-back and checksum happen after the write returns. Accepts a
  bounded cost: a crash in that window loses one version of one file, in a
  directory that opted in.
- A-Q12 Old FAT media show filenames as `????????`: which alphabet do we assume?
  — resolved 2026-09-13 (935): **3 and 4 — neither, then optionally both.**
  Undecodable 8.3 names render as visible escapes, which are reversible and so
  cannot collide; a mount may additionally be told its code page for correct
  names when the user knows the disk's origin. The escape is the right answer
  without information; the code page is an optimisation for when it exists.
- A-Q9 Networking exists twice, in the kernel and as a daemon: should the daemon
  become the default? — resolved 2026-09-12 (934): **C, then D.** Fix the
  head-of-line block first (`D-NETSOCK-SYNC`: a listener and all its accepted
  connections share one session behind one lock), then flip `net.userspace` on by
  default, then delete the in-kernel stack. The order is the decision: flipping
  first would ship the rough edge to everyone, and deleting is the only step with
  no way back but a revert.
- A-Q8 Desktop icon layout exists in two places, kernel and shell: which is the
  authority? — resolved 2026-09-12 (933): **C, neither — it leaves the kernel.**
  `fs::deskicons` and `/proc/deskicons` are deleted; `gui/desktop/src/icons.rs`
  becomes the layout authority and persists positions in userspace, which also
  makes the `icon_size` setting live. Lane C wires first, lane A deletes after,
  so no reboot loses icon positions in between.
- Q45 Convert the whole shell to bytes, or only the expanded word? — resolved
  2026-08-21 (§261): **B, the expanded word.** One data path — keystroke to
  syscall — goes byte-clean end to end; the source line stays text, as in bash.
- Q49 Modern AMD graphics: write it blind, buy hardware, or say we don't
  support it? — resolved 2026-08-21 (§262): **A for now**, C someday. The
  operator's "write it blind but label it untested" variant is recorded in the
  entry along with why it was not adopted.
- Q50 The Intel iGPU driver we also cannot run — which way? — resolved
  2026-08-21 (§263): **C.** Switch the iGPU on in firmware, boot SlateOS on
  this PC's bare metal from a USB stick, then write i915 against the real chip.
  Operator does the physical half; lane A readies the bootable-USB path first.
- Q51 Start the Mesa port now, or leave 3D parked? — resolved 2026-08-21
  (§264): **B, do the port** — sequenced after wifi, before Chromium. Chromium
  uses Mesa heavily but bundles SwiftShader, so Mesa is a performance
  prerequisite for it, not a functional one.
- Q52 Should the contamination-canary check keep failing on noise? — resolved
  2026-08-21 (§265): **D then C.** 20+ idle rounds first, then a shifted-band
  rule instead of zero tolerance.
- Q53 71% of benchmarks move >10% from a no-op rebuild — change the rule? —
  resolved 2026-08-21 (§266): **E.** Restate the threshold against each
  benchmark's measured band now; real hardware (unblocked by §263) is the fix
  that makes it mean something again.
- Q54 Switch to the 3.5× faster accelerator, split, or stay? — resolved
  2026-08-21 (§267): **E then C.** Measure whether the fast accelerator removes
  the noise; if so split — benchmarks fast, correctness gate stays on TCG where
  SMEP/SMAP/UMIP are actually exercised.
- Q46 [opt-level=0 benchmarks: release default or bench-only?] — resolved
  2026-09-07 (§922): **C + commit-count gate trigger;** implemented as
  pre-push gate 15.
- Q47 [D: drive full — shared vs separate target directory?] — operator input
  received 2026-09-07: tree now on E: with ~300 GB free; serialisation cost
  may be near zero; needs re-evaluation on E: before deciding.
- Q56 [Linux ABI exempt from native file-permission checks] — resolved
  2026-09-07, recorded 2026-09-09 (§924): **A, enforce parity**, paid for by
  suspend-and-prompt or an ahead-of-time grant (the same facility as §918).
  Operator's two follow-ups answered in §924: no per-account default-grant
  mechanism exists anywhere in the tree (it is a new feature), and yes it
  should cover native programs too — a per-account default grant is not
  ambient authority, because a real, revocable token is still issued.
- Q57 [capability-request prompt for keyboard/mic/camera?] — resolved
  2026-09-07 (§918): **A, yes,** and fix the error message.
- A-Q1 [`find -size` bare number: bytes here, blocks elsewhere] — resolved
  2026-09-07 (§916): **C, match POSIX** — bare number means 512-byte blocks.
- A-Q2 [C-test programs link unknown library; fix in fastpy] — resolved
  2026-09-07 (§915): **A, fix fastpy directly.**
- A-Q3 [kernel self-tests halt machine on production boot] — resolved
  2026-09-07 (§914): **D, halt on integrity failures, log-and-continue for
  the rest.**
- A-Q4 [`oci run` continues when option unapplied] — resolved 2026-09-07
  (§917): **A, refuse to start.**
- A-Q5 [shell `grep`: case-insensitive + line numbers by default] — resolved
  2026-09-07 (§919): **A, match standard defaults;** also integrate
  operator’s custom grep features.
- A-Q6 [deletion commits + fake-name commits in published history] — resolved
  2026-09-07 (§920): **A, leave history as-is.**
- A-Q7 [70 ms/file-open on D: — antivirus or disk?] — resolved 2026-09-07
  (§921), **closed by measurement 2026-09-09 (§923): it was the disk.**
  Cold reads cost 19.3 s on D: vs 0.27 s on E: for the same 807 files (71x);
  warm, both drives are identical. The "warm pass still costs 61.8 s"
  observation that ruled out the disk does not reproduce (0.19 s). No
  antivirus exclusion should be requested. Re-runnable:
  `python bench/file-read-latency.py`.

