### TD-OILS-COMPGEN-C-OPENED-EVERY-FILE-ON-PATH-AND-COULD-HANG-FOREVER — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/interp.rs` — `Shell::compgen_path_commands`.

**What was wrong.** The `$PATH` scan skipped directories with
`ent.path().is_dir()`. `Path::is_dir` is `fs::metadata`, and on Windows that is
a `CreateFile` + `GetFileInformationByHandle` — **one open per directory entry**,
across every directory on `$PATH` (~10 000 entries on the dev machine) — for a
question the directory enumeration had already answered.

Cost was the smaller half. The hazard is that an entry can be a file whose
*open* does arbitrary work: `…\AppData\Local\Microsoft\WindowsApps` holds
zero-byte `IO_REPARSE_TAG_APPEXECLINK` app-execution aliases, and opening one
runs the AppX resolver, which can block indefinitely when the Store services are
wedged. It does block, in practice.

**How it was found.** A `cargo test -p oils --lib` run never finished and left
an orphan test binary holding `osh-*.exe`, so the *next* run failed to link
(`ld: cannot open output file …: Permission denied`) — which is how the hang
first showed, as a link error rather than a timeout. Attaching cdb to the orphan
gave the answer directly:

```text
interp::tests::a_flattened_alias_table_is_sorted_by_bashs_own_comparator
interp::tests::compgen_answers_in_bashs_order_not_the_options_order
  ntdll!NtQueryInformationFile
  KERNELBASE!GetFileInformationByHandle
  …
  osh::interp::Shell::compgen_path_commands+0x4a6
  osh::interp::Shell::builtin_compgen
```

Both threads had been in that call for ~15 minutes. Note the failure mode: two
*unrelated* tests hang, because what they share is only that they run `compgen`.

**Fix.** Ask the cached type instead of opening:

* Windows — `ent.file_type().is_ok_and(|t| t.is_dir())`. `DirEntry::file_type`
  comes from the attributes the enumeration already returned, so it is free and
  cannot block. It does not follow reparse points, which is the point.
* Unix — folded into the `stat` the exec-bit check already did, and tightened to
  bash's own test: `executable_file` (general.c) is one `stat` demanding both
  `S_ISREG` and an execute bit. So this is now *fewer* syscalls than before and
  closer to bash, rather than a platform workaround.

The whole `oils` unit suite went from ~190 s to **29 s** — the `$PATH`-wide opens
had been dominating a run that only touches `compgen` in a handful of tests.

**Residue, deliberate:** on Windows a *junction* to a directory named `x.exe`
sitting on `$PATH` is no longer recognised as a directory and would be offered
as the command `x`. Following it is exactly the open that hangs, and the shell's
real target is not Windows, so the trade is taken knowingly.
