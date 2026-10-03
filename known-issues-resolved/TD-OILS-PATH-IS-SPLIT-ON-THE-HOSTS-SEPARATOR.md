### TD-OILS-PATH-IS-SPLIT-ON-THE-HOSTS-SEPARATOR, so a `:`-separated `$PATH` is one entry — 2026-08-03 — ✅ **FIXED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs`, `Shell::search_dirs` — the
`std::env::split_paths` call, which on the Windows development host splits on
`;`, not `:`.

**Reproduce:**

```
$ mkdir -p d1 d2; printf '#!/bin/sh\necho d1\n' > d1/prog; chmod +x d1/prog
$ bash --norc -c 'PATH=$PWD/d1:$PWD/d2; prog'
d1
$ osh -c 'PATH=$PWD/d1:$PWD/d2; prog'
osh: prog: command not found
```

Any `$PATH` a *script* builds is `:`-separated — it is the only spelling POSIX
has, and the only one the SlateOS target will have — so on this host every
multi-directory `$PATH` a script sets is a single nonexistent directory. Nothing
in the corpus notices because no case sets a list.

**Why it is not a one-line fix.** osh imports the host's `$PATH` verbatim, so
inside the shell it currently reads `C:\Users\x\bin;C:\Program Files\Git\usr\bin`
— `;`-separated, with `\`, and with drive-letter *colons* in it. Splitting that
on `:` would cut every entry in two. The reference bash (msys) does not have the
problem because it converts the whole variable at startup to `/c/Users/x/bin:…`,
POSIX paths joined by `:`. osh does not: `shell_path` normalises `\` to `/` but
keeps `C:` drive letters, which is deliberate (see its doc) and is what `$PWD`,
`hash` and `type` show.

So the choice is between adopting an msys-style drive mapping for *all* paths,
mapping only at the `$PATH` boundary, or leaving the host separator in place on
Windows and accepting the divergence as scaffolding. That was a fork with no
obviously-correct answer, so it was raised as Q36 in `open-questions.md`.

**The fix (2026-08-04).** Q36's option B, taken autonomously and recorded in
`design-decisions.md` §"`$PATH` is split on the shell's separator, and on the
host's too where the host wrote the value". `std::env::split_paths` is gone,
replaced by two free functions in `interp.rs`:

* `split_search_path` — splits on `:` *everywhere*, which is the whole rule on
  the SlateOS target; on the Windows development host it also splits on `;`,
  because the value the shell inherits there is the host's own and is written
  that way. It also answers one empty entry for an empty string, which
  `split_paths` does not, and does not strip quotes, which `split_paths` does.
* `drive_colon_at` — the one exception: a `:` immediately after a single ASCII
  letter *and followed by `/` or `\`* belongs to a drive letter, not to the
  list. The trailing-separator requirement is what keeps `PATH=x:y` splitting
  into two one-letter directory names, which is far likelier in a shell script
  than a drive-relative `x:y`.

So a script's own `PATH=bin:$PATH` now works on the development host, and the
inherited `C:\a;C:\b` still splits into two entries. Covered by the unit test
`the_search_path_separator_is_the_shell_s_own`, and the corpus is no longer
restricted to single-entry `$PATH` values.

**Blast radius while it stood:** `Shell::search_dirs` and everything built on it
— command lookup, `hash`, `type`, `command -v`, `compgen -c`, and (since
2026-08-03) `.`/`source`. Single-entry `$PATH` values, which is what every corpus
case used, behaved correctly throughout.
