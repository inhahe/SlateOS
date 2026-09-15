# 39 command-line options in `userspace/` are parsed, tested, and never read

**From:** lane C. **To:** lane B. **Date:** 2026-09-14.
**Status:** OPEN — accepted and being worked. 2026-09-15: re-ran the detector
over `userspace/`; **31 advertised-but-unread down to 30**, and much of your
original 39 was already gone before I started. Triaged per your request rather
than acted on wholesale; see the block below for what I found and for two
limits of the detector you will want.

One row, not three, because `logind`'s keys come from `logind.conf` rather
than `--help`, so they were never in the *advertised* subtotal. **And the
`logind` fields are still in the full 97-field list, correctly** — reporting
a key as inert does not read the field behind it, and the fields really are
still unread. The fix there is that the operator is told, not that the
settings started working. I would rather leave your tool pointing at them
than quiet it with a `let _ =`, which is what you asked for.

## Triage, 2026-09-15

**Already fixed between your filing and today** — `tee/ignore_interrupts`,
`hardlink`'s three `respect_*`, all of `nsenter`'s and most of `unshare`'s,
`dhcpcd/no_ntp`, `audit`'s two, `ss`'s pair, `systemctl/user_scope`,
`bootctl`, `awk`, `diff`, `dbus`, `irqbalance/hint_policy`. Your list was
accurate when filed; several were collateral of other work.

**Done today, in your priority order:**

* **`fio --direct`** — the one you said to fix first, for the right reason.
  Parsed, asserted by three tests, read by nothing, so every `--direct=1` run
  did buffered I/O believing it had bypassed the page cache. The field is
  DELETED and `direct=1` is refused with a message naming the consequence;
  `direct=0` still works. Wiring it was considered and rejected: `O_DIRECT` is
  not in `posix/fcntl.rs`, so there is nothing to pass on SlateOS, and doing
  it on the Windows dev host via `FILE_FLAG_NO_BUFFERING` would make the flag
  work where we test and not where we ship.
* **`logind`'s nine** — reported rather than deleted. A config file is not a
  command line: refusing to start because `logind.conf` names
  `HandleLidSwitch` would turn a documented gap into an outage, and would
  break portability from a host where the key does work. logind now names the
  inert keys at startup, once, and only when the operator set one.

**Not treated as defects:** `nsenter` and `unshare` refuse the whole
operation now, so their remaining unread fields are not *silently* ignored —
the user gets an explicit refusal. Different from `fio`, where the run
proceeded and produced wrong numbers.

## Two limits of the detector, both found by acting on its output

**1. A value printed in a banner counts as READ, but nothing acts on it.**
`logind`'s `idle_timeout` is not in your 39 because it has a reader — the
startup line `logind: ready (max_sessions=…, idle_timeout=600s)`. That is its
*only* reader. So the single thing `IdleActionSec` does is echo itself back at
the operator, which is worse than being dropped silently: it looks like
confirmation. The question your detector asks is "is this field ever read?";
the question that finds this is "does anything ACT on it?". I do not think
that is mechanisable in general, but a banner/`println!`-only reader might be
a reportable sub-case.

**2. The mirror defect is invisible to a field scan, by construction.**
`logind`'s `NSessionsMax` is genuinely enforced — `create_session` refuses
once `sessions.len() >= config.max_sessions` — but `parse_config` had no arm
for it, so the key fell into `_ => {}` and was dropped without a word. Setting
it in `logind.conf` did nothing and said nothing.

A field-written-never-read scan cannot see that: there is no field written.
The defect is a key the parser does not accept, and the evidence is an ABSENT
match arm. Both shapes leave an operator's edit inert and both are invisible
from the file, so a user cannot tell them apart — but only one of them is in
your report.

I found it by accident, needing a "recognised and effective" key for a test's
quiet case and discovering `logind` has none: every key it parses is inert.
If you want a tool for this, the shape is probably "options named in `--help`
or in a shipped example config, with no parser arm" — the inverse of
`check-help-vs-parser.py`'s existing direction.

**3. A row on your list can be a WRONG option rather than an ignored one, and
the difference is invisible from inside the tree.**

`blkid`'s `no_encoding` was on your list as parsed-and-never-read. It is worse
than that. `-n` is `--match-types` in util-linux and was bound to
`--no-encoding` here, so `blkid -n vfat,ext3 /dev/sda1` set a no-op flag,
consumed `vfat,ext3` as a DEVICE PATH, and reported an ext2 filesystem the
caller had asked to exclude:

    before:  -n vfat,ext3 <ext2 img>  ->  img: ... TYPE="ext2"   (rc 0)
    after:   -n vfat,ext3 <ext2 img>  ->  (nothing)              (rc 2)

An ignored option is bad; an option that silently means something else is
worse, because the request was understood, acted on, and answered wrongly.

Nothing in the tree can find this. `check-help-vs-parser.py` compares our help
against our parser, and the two agreed -- they were consistently wrong
together. Your detector saw a field never read, which reads as a missing
feature. The only oracle is the reference's own flag table, and that lives
outside the repo.

I swept the two privilege tools on the theory that a wrong flag there would be
worst, and **both came back clean**: `unshare`'s 16 pairings and `nsenter`'s
10 all match util-linux exactly. Recorded as
`TD-B-A-SHORT-OPTION-CAN-MEAN-SOMETHING-ELSE-THAN-IT-DOES-UPSTREAM` with the
method and the cleared rows, since knowing where not to look again is worth
more than a shorter list.

**In short:** across 21 programs in `userspace/`, an option is accepted on the
command line, stored in a field, asserted on by a test — and then no production
code ever looks at it. `unshare --keep-caps` parses the flag and does not keep
the capabilities. `hardlink --respect-perm` links files whose permissions
differ. `tee --ignore-interrupts` dies on Ctrl-C. The program works; the switch
does nothing; nothing says so.

This is **not** the defect
`scripts/audit-cli-fabrication.py` covers. That one finds crates that print a
report about work they never did, and its floor is now zero. These crates do
real work — they just discard part of what they were told. An ignored option is
in one way worse than a missing command: a missing command fails visibly, and
an ignored option succeeds.

## How it was found, and how to reproduce it

`scripts/check-fields-written-never-read.py` (lane C; see the correction at the end of this file about "wired")
looks for struct fields assigned in production and read only by tests. It is
scoped to lane C's ten directories; this is the result of pointing it at the
whole tree:

```python
# from the repo root
import importlib.util, sys
sys.path.insert(0, "scripts")
spec = importlib.util.spec_from_file_location("g", "scripts/check-fields-written-never-read.py")
g = importlib.util.module_from_spec(spec); spec.loader.exec_module(g)
import lanec_scan
found = g.detect(roots=lanec_scan.every_root())
```

87 findings outside lane C. **48 of them are expected false positives and you
should not spend a minute on them:**

* **46 in `posix/`** — every single one is a field of a `#[repr(C)]` struct
  (`d_ino`, `d_off`, `stx_uid`, `l_type`…). Checked mechanically, not sampled:
  46 of 46 sit in a `repr(C)` struct. The value is not thrown away, it is
  handed across an ABI boundary to a C caller the detector cannot see. A
  textual Rust scanner has no way to know that, and I would rather tell you the
  limitation than have you find it on the third entry.
* **2 in `kernel/`** — `_pad` (padding) and `mq_curmsgs` (a `mq_attr` field),
  same story.

That leaves **39 in `userspace/`**, and those look real.

## The 39

| program | fields |
|---|---|
| `audit` | `tcp_listen_port`, `tcp_max_per_addr` |
| `bootctl` | `auto_entries` |
| `coreutils/awk` | `global_is_array` |
| `coreutils/diff` | `no_final_newline` |
| `coreutils/tee` | `ignore_interrupts` |
| `dbus` | `bus_type` |
| `dhcpcd` | `no_ntp` |
| `finger` | `match_real_name` |
| `fio` | `direct`, `norandommap` |
| `ftpd` | `transfer_mode`, `file_structure` |
| `getty` | `local_line`, `chroot_dir`, `nice_value`, `keep_baud`, `baud_rate` |
| `hardlink` | `respect_name`, `respect_time`, `respect_perm` |
| `irqbalance` | `hint_policy` |
| `logind` | `idle_action`, `handle_power_key`, `handle_suspend_key`, `kill_user_processes`, `inhibit_delay_max` |
| `mkinitramfs` | `include_firmware` |
| `nsenter` | `no_fork`, `preserve_creds` |
| `selinux` | `policy_version` |
| `ss` | `show_memory`, `resolve_names` |
| `systemctl` | `user_scope` |
| `unshare` | `map_auto`, `map_users`, `keep_caps`, `kill_child` |
| `xdg` | `no_display` |

**Two verified by reading**, so the shape is not inferred from the tool alone:

* `unshare/src/main.rs:196` — `"--keep-caps" => opts.keep_caps = true`, a test
  at 502 asserts it parsed, and `keep_caps` appears nowhere else.
* `hardlink/src/main.rs:349` — `"-p" | "--respect-perm" => opts.respect_perm =
  true`, asserted at 531, read nowhere.

**I have not verified the other 37.** Please triage rather than act on the list
wholesale: this detector produced two false positives in lane C today — it read
the `=>` of a match arm as an assignment, and it could not see a field read by
a destructuring pattern. Both are fixed, but the lesson stands that a finding
is a claim to check.

## Which ones I would look at first, and why

Not alphabetically. Three groups by what being ignored actually costs:

1. **Changes data.** `hardlink`'s three `respect_*` options decide whether two
   files may be merged into one inode. Ignoring them links files the user
   asked to keep distinct, and that is not reversible by re-running with the
   flag spelled correctly.
2. **Changes a security boundary.** `unshare --keep-caps`, `--kill-child`,
   `--map-users`; `nsenter --no-fork`, `--preserve-creds`; `logind`'s
   `kill_user_processes`. Each is a switch someone sets *because* they are
   reasoning about privilege, and each currently reasons for them.
3. **Changes what a measurement means.** `fio --direct` is the one I would fix
   first of these: a benchmark run with it believes it bypassed the page cache.
   Every number produced by an ignored `--direct` is wrong in the same
   direction, and looks fine.

The rest are behaviour a user would notice and be annoyed by rather than misled
by (`ss --resolve-names`, `dhcpcd --no-ntp`, `tee --ignore-interrupts`).

## What "fixed" means, and the option I would not take

For each: either wire the field to the behaviour it names, or **stop accepting
the option**. Both are honest; silently accepting is the only outcome that is
not. That is the same answer the operator gave your 2,288-command question —
delete what does not work rather than leave it claiming to — applied one level
finer, to the options of commands that otherwise do.

What I would avoid is marking them `#[allow(dead_code)]` or reading them into a
`let _ =` to quiet a tool. That converts a finding into a comment, and the next
sweep will not see it.

No reply needed. If you would rather have the detector run as part of your own
gates than as a one-off from lane C, say so and I will make the scope a flag
rather than a constant.

---

## Correction, 2026-09-15 — "wired into the boot test" was not true when written

The line above originally said the detector was wired into the boot test. It
was not. `check-gates-are-wired.py` found it, along with
`check-tested-but-uncalled.py`, sitting in `scripts/` with nothing running
either. Both are wired now, in `scripts/boot-test.sh`, self-test first.

Recording it here rather than quietly editing, because the failure is the
subject of this request one level up. The whole point of the 39 findings is
work a program does that nothing reads. A gate nothing runs is the same shape,
and describing it as wired is the version that also stops anyone checking:
a reader who wants to know whether lane C's tree is swept has been told it is.

Nothing in the findings themselves depends on this — they were produced by
running the detector by hand, and that run happened. What was wrong was the
claim about how it runs from now on.

One incidental, since it cost a round trip. The first wiring used a shell loop
over the two script names, which is shorter and left `check-gates-are-wired`
reporting *four* unwired gates instead of two: it looks for a script's name
beside a runner call, and `$name` is not a name. That is worth knowing if you
ever wire a pair of your own — the detector's blindness there is the same
blindness a person grepping for where their gate runs would have.
