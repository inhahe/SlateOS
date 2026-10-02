### TD-OILS-MAPFILE-UFD. `mapfile`/`readarray` does not implement `-u fd` (read from a numbered descriptor) — ✅ RESOLVED 2026-07-20

**Resolution (2026-07-20):** `builtin_mapfile` now accepts `-u N` and, for
`N >= 3`, routes the read through `open_fds` (byte cursor) / `coproc_read_fds`
(live pipe) via a masking `RedirPlan`/`StdinSrc` — exactly `read -u`'s routing —
before `read_all_bytes`. The invoking name (`mapfile` vs `readarray`) is threaded
through as a `tag` so the invalid-option and fd diagnostics carry the right name,
including bash's two distinct fd errors: `<tag>: <spec>: invalid file descriptor
specification` (non-numeric, status 1) and `<tag>: <n>: invalid file descriptor:
Bad file descriptor` (closed/out-of-range, status 1). Both `help` synopses now
advertise `[-u fd]`. Verified byte-for-byte against bash 5.x; covered by the
`mapfile_reads_from_numbered_fd` regression test. Original report follows.

**Where:** `userspace/oils/src/interp.rs` — `builtin_mapfile`. Its option loop
handles `-t -d -n -c -C -s -O` and the array operand, but had no `-u` case, so
`mapfile -u 3 arr` hit the invalid-option path and failed with status 2. bash
reads the array from descriptor `fd` (e.g. one opened by `exec 3< file` or a
coproc read end).

**Repro:** `exec 3< <(printf 'a\nb\n'); mapfile -t -u 3 arr; echo "${arr[@]}"` →
bash prints `a b`; osh prints `mapfile: -u: invalid option` + usage, exit 2.

**Proper fix:** mirror `builtin_read`'s `-u` handling — accept `-u N`, and when
`N >= 3` route the read through `open_fds` (byte cursor) / `coproc_read_fds`
(live pipe) instead of the ambient `stdin`/`redir`. `mapfile` already takes
`stdin`/`redir`; add a `ufd: Option<i32>` and build a masking `RedirPlan` /
`StdinSrc` exactly as `read` does before calling `read_all_bytes`. The usage
synopsis already advertises `-u fd`, so no wording change is needed. Note
`readarray` shares this code path (same gap).
