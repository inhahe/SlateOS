## `A-KSHELL-FOURTEEN-MORE-COMMANDS-DISCARD-AN-OPTION-THEY-DO-NOT-RECOGNISE` (lane A, 2026-08-25) — ✅ **FIXED** 2026-08-25

**In short:** The sweep above fixed nine commands that threw away a word they
did not understand. A checker written straight afterwards — asking not "does
this look like the nine?" but "does this loop dispatch on option spellings and
have no way to say no?" — found fourteen more, including two that erase disks.
`mkfs.fat --labl NAME /dev/sda` formatted the device with no label and said
nothing. All fourteen now refuse.

**Where.** `kernel/src/kshell.rs`. Found by `scripts/check-option-refusal.py`,
detector D3.

| Command | The discard | What it silently became |
|---|---|---|
| `dmesg` | catch-all arm dropped the word; `-l` fell to `_ => 0` | every option *narrows* the log, so a typo printed the whole ring buffer |
| `du` | `-d N` (separated) was never implemented | depth 0, then `N` became the path, then the real path overwrote it — three misreadings of one line |
| `touch` | unknown option became the path | `touch -x f` created `f`; `touch a b` created `b` and never mentioned `a` |
| `column` | unknown option, and a second operand, became the file | `column a b` formatted `b` |
| `lsplus` | `--sort`/`--type` fell back to their defaults | `--type fil` listed directories too; the default *is* "everything" |
| `recent` | `--type` fell back to `None` | `None` is not "no type given", it is "every type" |
| `fswalk` | the trailing max-depth operand took 64 | `fswalk count / two` walked the whole filesystem |
| `fileops` | operands were "everything that is not a flag" | `fileops copy a b --overwite` copied to a directory named `--overwite` |
| `toolbar` | catch-all dropped the flag | every flag turns a button *on*, so the toolbar lacked exactly the button asked for |
| `traceroute`/`traceroute6` | `-m`/`-q` `.ok()` on failure | `-m 5x` traced 30 hops and printed "max 30 hops" as if that were the request |
| `mount` | `-t`/`-o` at end-of-line fell into the positional arm | `mount -t` tried to mount a device named `-t` |
| `container ls` | printed "Ignoring filter …" and continued | `ls -q --filter status=stoped` inside `rm $(…)` would have listed every *running* container's id |
| `container info` | second id dropped | `info 3 4` answered about 3 alone |
| `oci build` | warned and continued for `-t`/`--target`/`--build-arg` | no `--target` builds the *last* stage instead of the named one — a different image, under the right name |
| `mkfs.fat` | unknown option became the device, then the real device overwrote it | formatted the right device with the wrong options; the only evidence was a missing label |
| `fsck.fat` | unknown option became the device | `--repar` looked like a repair and was a check |
| `flock` | catch-all dropped the mode | the default is `query`, which takes no lock — a dropped `-x` declined to lock and printed a status line |
| `od` | `-N` unreadable → `usize::MAX`; `-A`/`-t` unknown letter → octal | `usize::MAX` is the *absence* of a limit, the opposite of what `-N` is for |

**Why this is the interesting half.** The nine in the entry above were found by
a human reading code adjacent to a bug. These fourteen were found by asking the
*category* question, which is what §299 says a gate's trigger must do. Had the
checker been keyed on the nine's spellings — `.filter(|w| !w.starts_with('-'))`,
`parse().unwrap_or(…)` — it would have caught two of these fourteen. Keyed on
"a loop that dispatches on option spellings and contains no refusal", it caught
all of them, plus `container ls` and `oci build`, which do not look like the
original nine at all: they *print a message* and then carry on anyway, which is
§600's explicitly-rejected middle option and reads as diligence in review.

**Two false positives, and what they taught the checker.** `parse_tr_args` and
`classify_sed_args` refuse properly — via `return Err(…)`, because their
contract is a `Result`, not `set_exit`. The detector learned the shape rather
than being given an allowlist entry: an allowlist entry would have exempted
those two functions forever, including any *future* mute loop inside them.

**Tested by.** Self-test rung 64, deliberately separate from rung 63 so that
the checker-found set and the bug-found set can diverge visibly. It asserts a
non-zero exit for each refusal, and for the two commands with an observable
effect it asserts the effect did not happen: `touch -x path` leaves no file,
and `fileops copy … --overwite` creates nothing named `--overwite`. It also
asserts the *positive* direction — `du -d 1`, `od -A x -N 4`, `lsplus --type
file` all still work — so the rung cannot pass by having broken the options.

**Not a regression.** True of each command since it was written.
