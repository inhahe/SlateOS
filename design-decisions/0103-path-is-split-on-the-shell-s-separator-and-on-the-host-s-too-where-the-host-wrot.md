## §103 — `$PATH` is split on the shell's separator, and on the host's too where the host wrote the value

**Date:** 2026-08-04

**Decided by:** Claude (autonomous) — this was Q36 in `open-questions.md`,
raised for the operator on 2026-08-03. It is being taken autonomously because
the recommended option turned out to be small, local and easy to reverse, and
because leaving it open was blocking every corpus case that needs a
multi-entry `$PATH`. The operator may overrule.

**Context.** osh split `$PATH` with `std::env::split_paths`, which on the
Windows development host splits on `;`. Every shell script in the world writes
`PATH=/a:/b`, so osh read that as one directory literally named `/a:/b` —
which never exists. A multi-entry `$PATH` built by a script silently found
nothing, and that reached command lookup, `hash`, `type`, `command -v`,
`compgen -c` and `.`/`source` alike. No corpus case could use a `$PATH` list.

The separator could not be changed on its own, because the separator and the
path *syntax* are one decision. `:` is unambiguous only if a path can never
contain one — but osh keeps native Windows paths with drive letters
(`C:/Users/...`), deliberately, since that is what `$PWD`, `hash` and `type`
show. Splitting those on `:` alone would cut every entry after its drive
letter.

**Decision.** Q36's **option B**: split at the `$PATH` boundary only, with a
drive-letter escape. `std::env::split_paths` is replaced by two free functions
in `interp.rs`:

* `split_search_path` splits on `:` — the shell's separator, everywhere, and
  the whole rule on the SlateOS target. On Windows it *additionally* splits on
  `;`, because the value the shell inherits there is the host's own and is
  written that way.
* `drive_colon_at` is the single exception: a `:` one byte after the start of
  an entry, preceded by a single ASCII letter **and followed by `/` or `\`**,
  belongs to a drive letter and is not a split point.

**Rationale.** It buys the behaviour that matters — a script's own
`PATH=bin:$PATH` works, and multi-entry lookup order becomes corpus-testable —
for one localised function, without committing the project to an msys
convention that SlateOS will never use. Requiring a *separator after* the
colon is what keeps the heuristic honest: `PATH=x:y` still splits into two
one-letter directory names, which is far likelier in a shell script than a
drive-relative `x:y`, and it matches how Windows itself reads the two.

Writing the splitter by hand also fixed two smaller `split_paths` mismatches
that had nothing to do with the separator: `split_paths` yields *nothing* for
an empty string where the shell wants one empty entry (which means the current
directory), and it strips double quotes around an entry, which bash does not.

**Alternatives considered.**
- *A — msys-style drive mapping everywhere* (`/c/Users/...` as the shell's
  canonical spelling). The only option that also aligns `$PWD`, `pwd`,
  `BASH_SOURCE`, glob results and `cd` with the reference bash on this host.
  Rejected as the largest change by far — it touches every place a host path
  enters or leaves the shell — and because it bakes an msys convention into a
  shell whose real target is SlateOS, where paths are already `/`-rooted and
  the problem does not exist. It would be the right answer only if osh were
  meant to be a first-class Windows shell, which it is not.
- *C — accept the divergence as dev-host scaffolding*, documenting it and
  letting it come right for free on SlateOS. Zero risk and zero work, but it
  leaves a real bash-parity gap in `$PATH` lookup order permanently untested
  for as long as the dev host is Windows — which is the whole foreseeable
  future of this work.

**Where it lives.** `userspace/oils/src/interp.rs` — `split_search_path`,
`drive_colon_at`, `Shell::search_dirs`; unit test
`the_search_path_separator_is_the_shell_s_own`.

**How to reverse.** `split_search_path` is the only caller-visible seam: make
its `b';' => cfg!(windows)` arm `false` (or restore `std::env::split_paths` in
`search_dirs`). Nothing else in the shell depends on the two-separator rule.
