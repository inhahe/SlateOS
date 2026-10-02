## `B-CAL-IS-NARROWER-THAN-UPSTREAM-IN-FIVE-PLACES` (lane B, 2026-08-27) — **open**, deliberate limitation

**In short:** five things util-linux's `cal` does that ours does not, each
because the thing it depends on does not exist in this build yet. None of them
change a printed calendar in the default case; the first is the one most likely
to be noticed, because it means `cal -3` will not widen itself to fill a very
wide terminal window unless the shell has told it how wide the window is.

| # | Upstream | Ours | What it depends on |
|---|---|---|---|
| 1 | Asks the terminal its width with the `TIOCGWINSZ` ioctl (a request meaning "how big is this window?"), falling back to the `COLUMNS` environment variable. | `COLUMNS` only, defaulting to 80. | `TIOCGWINSZ` is not wired up in this build's terminal layer. Until it is, `cal --columns=auto` in a 200-column window still lays out for 80 unless `COLUMNS` is exported. |
| 2 | Reads `_NL_TIME_WEEK_1STDAY` from the locale to decide whether the week starts on Sunday or Monday. | Sunday — which is what the `C` locale answers — unless `-m`/`--monday`, `-s`/`--sunday` or `--iso` says otherwise. | A locale database with the `LC_TIME` week fields. We have no locale support beyond `C`. |
| 3 | Has configurable `workday` and `weekend` colour sequences alongside the highlight. | Omitted entirely. | Nothing — upstream's built-in values for both are the *empty string*, so shipping them would add two settings that produce no bytes. Add them if and when a colour scheme file exists to set them. |
| 4 | `--version` prints `cal from util-linux 2.39.3`. | `cal from SlateOS coreutils 0.1.0`. | Deliberate: the sentence shape is upstream's (so a script that greps `from` still works), but claiming to be util-linux would be a lie, and a version-sniffing script would then expect the two vertical bugs above to be fixable by upgrading. |
| 5 | Diagnostics name `argv[0]` as typed, so `/usr/local/bin/cal -Z` says `/usr/local/bin/cal: invalid option`. | Always `cal:`. | House rule across this coreutils — the program name is a constant, not an attacker-controlled string. |

**Two more differences that are implementation, not behaviour**, noted so a
reader diffing against `cal.c` does not go looking for a bug: we render the
whole calendar into one `String` and write it once (upstream writes as it goes,
so a full disk truncates it mid-month; ours either writes all of it or reports
the failure), and operands echoed back in an error message go through
`quote::escape_unprintable`, so a month name made of arbitrary bytes cannot
smuggle escape sequences into the terminal.

**The proper fix for #1**, which is the only one with a user-visible cost:
`TIOCGWINSZ` needs to reach `coreutils::stdfd` as a `winsize(fd) -> Option<(u16,
u16)>`. Several other commands want it (`ls` column layout, `less`, `ps`
truncation), so it belongs there and not in `cal`. Once it exists, `cal.rs`'s
`terminal_width()` is the single call site to change.

**Where it lives:** `userspace/coreutils/src/bin/cal.rs` — "Where this
deliberately diverges" in the module doc (7 items, of which these are 5), and
the tests `a_wide_terminal_never_widens_the_row_but_columns_auto_does` and
`a_pipe_removes_both_of_the_things_a_colour_would_have_marked`.
