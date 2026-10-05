### TD-OILS-MSYS-CHMOD-TYPE-A. MSYS bash's `type -a` cannot see a scratch file that `chmod +x` just marked executable — 2026-08-02 — ⛔ **WONTFIX** (a measurement artifact of the dev host, not an osh bug)

**Where:** nothing in osh. It bites when *writing* a case for
`scripts/osh-bash-diff.py`, because the reference shell is MSYS/Git-for-Windows
bash on an NTFS volume with no POSIX mode bits behind it.

**What:** MSYS `chmod +x f` does not reliably make `test -x f` true. bash's
command search does not care — `find_user_command` accepts the file and runs it,
and `type` / `type -p` / `command -v` all report it — but `type -a` goes through
the stricter `executable_file()` probe and so reports `bash: type: f: not found`
for a file the very same shell will happily execute.

```
$ printf 'echo hi\n' > w.sh; chmod +x w.sh; PATH=$PWD
$ w.sh          →  hi
$ type w.sh     →  w.sh is /c/…/w.sh
$ type -a w.sh  →  bash: type: w.sh: not found
```

**Consequence for the corpus.** A case that calls `type -a` on a file it created
itself will diverge for a reason that has nothing to do with osh. Use `type`,
`type -p` or `command -v` instead — `tests/corpus/path-prefix-assignment-search.sh`
says so in its header. A case that genuinely needs `type -a` must use a command
that was already installed by the host.

**Why not waived instead.** An `# EXPECT-DIFF` would pin the artifact into the
corpus as if it were behaviour. There is nothing to assert here: the host cannot
represent the input the test wants.

> **Gone as of 2026-08-25 — the host was the whole problem, and the host has
> changed.** `scripts/osh-diff.sh` runs both shells inside WSL on ext4, where
> mode bits are real. Re-measured:
>
> | shell | `test -x w.sh` after `chmod +x` | `type -a w.sh` |
> |---|---|---|
> | osh | `yes` | `w.sh is /tmp/…/w.sh` |
> | glibc bash | `yes` | `w.sh is /tmp/…/w.sh` |
>
> Identical. **`type -a` is now usable in corpus cases**, including on files the
> case creates itself, and the restriction this entry imposed on case authors is
> lifted. `tests/corpus/path-prefix-assignment-search.sh`'s header still warns
> against it; that warning is now stale and should be dropped when the file is
> next touched.
>
> The WONTFIX stands as a judgement about osh — there was never a bug here — but
> the *measurement* is no longer blocked.
