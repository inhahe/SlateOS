### TD-OILS-NOCLOBBER-IGNORES-THE-AMPERSAND-FORMS. `set -C` does not protect a `&> file` target — 2026-08-03 — ✅ **FIXED 2026-08-03**

**Where:** `userspace/oils/src/interp.rs` — the `RedirectOp::WriteBoth |
RedirectOp::AppendBoth` arms of the transient redirect planner and of the
persistent (`exec`) applier, and `Shell::resolve_dup_out` /
`Shell::apply_persistent_dup_out`'s `DupWord::NotAFd if fd == 1` branches. Each
opens the file without the `self.noclobber` test its `Write` neighbour makes.

**Reproduce:**

```
$ bash --norc -c 'set -C; : > c; echo x &> c; echo "rc=$?"'
bash: line 1: c: cannot overwrite existing file
rc=1
$ osh -c 'set -C; : > c; echo x &> c; echo "rc=$?"'
rc=0
```

Same for `>& c`, `1>& c` and `exec &> c` / `exec >& c`. `&>>` is an append and is
correctly exempt in both shells.

**Fix.** `&>` is a `>` on two descriptors, so it takes the same guard. The check
is now one `Shell::noclobber_check(target, truncating)` called from all six
output-open sites — `Write`/`Clobber`/`Append` and `WriteBoth`/`AppendBoth` on
each of the transient and persistent paths, plus the two `>& file` branches —
rather than the two hand-written copies it was. `truncating` is what decides:
false for `>|`, `>>` and `&>>`, true for the rest.

The ordering matters and is bash's: the special filenames are resolved *first*,
because `redir_open` looks them up in `_redir_special_filenames` and returns
before it ever reaches `noclobber_open`. So `set -C; echo hi > /dev/stdout`
succeeds — those are dups, with no file to protect. (The reference bash on this
host disagrees, refusing `> /dev/stderr` and `>& /dev/fd/2` under `set -C`; that
build appears not to have the special-filename table compiled in, so it opens
the Cygwin device path for real. osh follows upstream bash, and the corpus case
stays away from the combination.)

Covered by `noclobber-guards-every-form-that-truncates.sh`.
