## B-TWO-IMPLEMENTATIONS-OF-`w`-AND-THE-BETTER-ONE-CAN-NEVER-RUN (lane B, 2026-08-22)

**In short:** `w` is the command that lists who is logged in and what each of
them is running. We have written it **twice**, in two different places, and the
two disagree about what columns it prints. Only one of them can ever actually
run — and it is the **less complete** of the two. The better one is unreachable
by construction: it is a second personality of the `who` program, activated by
running `who` under the name `w`, and nothing ever does that, because a
*different* program already owns the name `w`. Two more commands, `finger` and
`pinky`, are written and have no way to be run at all.

**How it happens.** Two crates, each with its own executable, and one of them
changes behaviour based on the name it is invoked under:

| Crate | Executable it builds | Extra personalities it answers to |
|---|---|---|
| `userspace/who` | `who` | **`w`** — `userspace/who/src/main.rs:1091`, `if basename == "w"` |
| `userspace/w` | `w` | `finger`, `pinky` — `userspace/w/src/main.rs:42-43` |

An extra personality only runs if something invokes the executable under that
name — a symlink, a hard link, or a copy. So:

- `who`'s `w` personality needs a `w` that is a link to `who.exe`. There is
  none, and there cannot be a natural one, because `userspace/w` builds a real
  `w.exe`. **Dead code.**
- `finger` and `pinky` need links to `w.exe`. Nothing creates them: there is no
  `userspace/finger` or `userspace/pinky` crate, no `coreutils` bin of either
  name, and `scripts/create-ext4-rootfs.sh` creates no such alias (it does copy
  binaries to second names — `dash` → `/bin/sh`, line 669 — so the machinery
  exists; it is simply never pointed at these). **Unreachable features.**

**The one that loses is the better one.** GNU `w` prints a summary line
(time, uptime, user count, load averages) and then eight columns:
`USER TTY FROM LOGIN@ IDLE JCPU PCPU WHAT`.

| | Summary line | Columns printed |
|---|---|---|
| `who` in `w` mode (**unreachable**) | yes | all eight — `who/src/main.rs:940` |
| `userspace/w` (**this is what runs**) | yes | six: `USER TTY FROM LOGIN@ IDLE WHAT` — no `JCPU`, no `PCPU` (`w/src/main.rs:415`) |

`JCPU` and `PCPU` are the CPU time used by all processes on the terminal and by
the current process respectively — the two columns that make `w` more than a
prettier `who`. The implementation that computes them (`who/src/main.rs:600`,
`get_user_process_info`, which scans `/proc`) is the one that never runs.

**The proper fix.** One name, one program — the same rule as the entry above,
and this is the same disease in a form that filename collisions do not catch,
because the two executables have *different* filenames and only the behaviour
is duplicated. Concretely:

1. Decide which `w` survives. On the evidence it should be `who`'s
   implementation, which is the complete one — but that means either deleting
   `userspace/w` or gutting it to a link.
2. Port `finger` and `pinky` to whatever survives, and **stage the alias names
   in the image build**, so a personality that exists is a command that runs.
   A personality with no link is not a feature; it is unreachable code that
   reads like one, which is worse than not having it.
3. Prefer removing the `argv[0]` dispatch entirely over adding links to it.
   Per `coreutils-canonical-answer.md`, one executable serving several tool
   names is the shape this OS's capability model is meant to exclude: the
   kernel grants permissions per file, so one file answering to `w`, `finger`
   and `pinky` must hold the union of what all three need.

**UNBLOCKED since 2026-09-07 and this paragraph did not say so until
2026-09-12.** It read "sequence it after `open-questions.md` → B-Q7, which
decides whether `userspace/<tool>` crates or `coreutils` is the home for this
family." B-Q7 was answered five days before that was noticed: §1005,
`Decided by: Operator`, **coreutils is the one home** and the duplicate crate
is deleted. So the sequencing advice was sound and its precondition had been
met; the entry just went on giving it. Nothing here is waiting on anything.

**How to see it** (once built):

```
$ ./w --help          # six columns, no JCPU/PCPU -- userspace/w
$ cp who.exe w.exe && ./w    # eight columns -- userspace/who's dead branch
$ ./finger            # no such file: the personality has no executable
```
