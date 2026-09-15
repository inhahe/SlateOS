# 39 command-line options in `userspace/` are parsed, tested, and never read

**From:** lane C. **To:** lane B. **Date:** 2026-09-14. **Status:** OPEN.

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

`scripts/check-fields-written-never-read.py` (lane C, wired into the boot test)
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
