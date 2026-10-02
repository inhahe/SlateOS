## TD-B-EIGHTY-THREE-DISCARDED-FAILURES-ARE-PINNED-UNREAD (lane B, 2026-09-10)

**In short:** `check-read-defaults` was widened to see two more spellings of the
defect it already catches, and found 83 more sites. They are pinned in the
baseline **as a set**, not inspected one at a time, so the ratchet holds the
line while the triage happens. Sampling them found real defects, listed below.
The 16 original `read_to_string` entries were inspected individually and are
not part of this.

**What the widening added.** `env::var(..).unwrap_or_default()`, and
`local_fn(..).unwrap_or_default()` where the function is defined in the same
file and its signature says it returns `Option` or `Result`. The return type is
read from the definition rather than guessed from the name, and method calls
are excluded — a file defining any local `fn get(..) -> Option<T>` otherwise
implicates every slice in it, which is the difference between 39 findings and
171.

**The worst of them**, from a sample of twelve — and one of the three I first
listed here was a false finding, which is recorded rather than deleted because
a sample read without its context is exactly how this list could fill with
them:

| Site | What the default means |
|---|---|
| `ftp/src/main.rs:1792,1828,2001` | `read_password("Password: ").unwrap_or_default()` — **a failed password read becomes an empty password, which is then sent.** |
| `stty/src/main.rs:1272-1304` (5×) | `tiocgwinsz(fd).unwrap_or_default()` — an ioctl failure becomes a 0×0 terminal, and the caller then computes a layout for it. |
| ~~`crontab/src/main.rs:669`~~ | **THIS ENTRY WAS WRONG.** I wrote it from a sample without reading the surrounding code. The empty username only ever accompanied `Action::Help`, which prints usage and touches no spool file — the line above it said so, and the real path already used `ok_or_else`. Corrected 2026-09-11; the field is `Option<String>` now so the next action added cannot inherit the trap. |

Also `stat` (empty symlink target), and `udevd` and `thermald`, both fixed
2026-09-11.

**Three more of the names above were re-read in context on 2026-09-11 and two
are defensible**, which is the same correction as `crontab`'s and is why the
whole list should be read before it is worked:

* `hostname` — `read_hostname()` returns `Err` only when BOTH
  /proc/sys/kernel/hostname and /etc/hostname are absent, empty or unreadable,
  so `Err` already means "no hostname is configured". The caller is
  `--boot-set`, whose job is to ensure one exists. Defensible.
* `mktemp` (the `id` personality) — the three `uid_to_name`/`gid_to_name`
  defaults are tested with `is_empty()` on the very next line and print
  `uid=1000` without a name, which is what real `id` does for an unresolvable
  uid. Defensible.
* `efibootmgr` — NOT defensible, and worse than the entry said. The discarded
  read was the small half; the crate substituted two INVENTED boot entries
  ("Slate OS" and "UEFI Shell", with plausible device paths) whenever no real
  ones were found, and printed them as the machine's boot configuration. A test
  asserted the labels. Fixed 2026-09-11: it refuses, and says whether efivarfs
  is absent or merely empty. The `unwrap_or_default()` on BootOrder stays
  baselined — with the fabrication gone it leads to that refusal rather than to
  an invented answer.

**A separate finding from the same sample, not part of this entry's debt:**
`userspace/last` carries a FOURTH copy of the utmp record parser
(`extract_string(data, offset + UT_USER_OFFSET, ..)`). who, uptime and w were
converted to the `utmpfile` crate earlier today and `last` was missed because
it reads `/var/log/wtmp` rather than `/var/run/utmp` — the same format under a
different path, so a grep for the path could not find it. Its fields are
`String` via the same lossy decode that was removed from `who`.

**The proper fix** is per-site and mostly small: keep the `Option` and let the
caller print `?`, skip the row, or refuse. `userspace/iostat` prints six
question marks where it printed six zeroes, which is the whole shape of it.
**Trigger: fix them in batches by crate, dropping each from the baseline as it
goes.** The baseline may only shrink, so the count is the progress bar.

**Progress: 95 -> 74 -> 76.** It went UP, and the two extra are not
regressions -- read the next paragraph before reading the count as a defeat.

**The count was measured through a scanner that could not see the whole
corpus.** `rustlex.live_code` cut each file at its first `#[cfg(test)] mod`,
which in `oils/src/interp.rs` is a `mod stderr_tee` helper at line 3,348 of
109,742. Fixing it (2026-09-11) added 65,069 lines to the visible corpus,
+15.6%, and the gate immediately found one real site in the newly-visible
code: `fc -e` read back the file the user had just edited with
`std::fs::read(&path).unwrap_or_default()` and RAN the result, so a failed
read ran nothing and reported success. Fixed, not pinned.

The other two came from teaching the gate `fs::read` -- absent until now
because a bare `read` alternative matches every `buf.read(..)` method call.
Both are `pwdb::from_files`, and both are CORRECT: "A file that cannot be read
is an empty database, not an error. That is glibc's behaviour ... `ls -l` on a
system with no /etc/passwd must still list the directory, printing numeric ids,
rather than fail." They are pinned as known-and-justified. So the honest
reading of 76 is 74 minus one fixed, plus two that were always there and are
meant to be.

**A ratchet whose count only shrinks cannot tell you that the instrument
shrank instead of the problem.** Both times a scanner here was found blind, it
was by accident -- a boot test going red in August, a count dropping by two in
September. Neither floor noticed, because a floor on the total cannot see a
hole in the distribution.

`ftp`'s six are fixed (2026-09-10) -- the three
`read_line("Name: ")` and three `read_password("Password: ")` sites now
distinguish end-of-input from an empty answer, so a closed stdin aborts the
login instead of sending a blank password.

`stty`'s five followed, and three of them were a READ-MODIFY-WRITE rather than
a display: `stty rows 40` read the Winsize, set one field, and wrote the whole
struct back, so a failed TIOCGWINSZ set the terminal to 40 rows and zero
columns and discarded both pixel dimensions. That is the same shape as
sudo/visudo rewriting /etc/sudoers from an empty read -- the fifth instance of
the family, in a terminal instead of a file.

`last`'s six followed (four wtmp fields, two lastlog), and then `udevd`'s two
and `thermald`'s one, which were **not** display defects:

* `udevd` matched udev rules with `read_sysfs_attr(..).unwrap_or_default()`.
  An unreadable attribute became `""`, so `ATTR{x}=="v"` did not match --
  harmless -- but `ATTR{x}!="v"` became `!glob_match(v, "")`, **true**. A rule
  saying "apply to devices whose attribute is not v" fired for a device whose
  attribute could not be read, and a matching rule there sets OWNER and MODE on
  the device node. The same missing value failed closed one way and open the
  other.
* `thermald` read `trip_point_N_type` after checking the file exists, so a
  failure was a read error rather than an absent trip point -- and it fell to
  `_ => continue`, dropping the trip point silently. On a thermal daemon that
  can be the critical one. **Worth grepping the remaining 84
for the same pattern before working through them in order: a discarded read
that is then written back is a different severity from one that is printed.**
