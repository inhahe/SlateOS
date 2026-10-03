## `B-RENICE-DIVERGES-FROM-UTIL-LINUX-IN-FIVE-MEASURED-PLACES` (lane B, 2026-08-27) -- **open**, deliberate divergence

**In short:** our `renice` is a transcription of util-linux 2.39.3's, and
almost everything about it -- including several behaviours nobody would guess
-- was measured against the real binary and matched. Five things are on
purpose different. One is an upstream bug we declined to copy; the rest are
house rules. Recorded here so a future reader who diffs the two does not
"fix" them back.

| # | Upstream (measured) | Ours | Why |
|---|---|---|---|
| 1 | A *successful* user lookup can be rejected because of a leftover from an earlier operand: `renice 0 -u root` works, but `renice 0 -p abc -u root` says `unknown user root`. | A lookup that succeeds is accepted, whatever came before it. | Upstream bug -- one `char *endptr` is shared by every `strtol` in `main`, and the `getpwnam` path never assigns to it. It reports a user that exists as missing, which is a wrong answer rather than a cosmetic one. See design decision 623 for the rule this sets. |
| 2 | Diagnostics are prefixed with `argv[0]` as typed, so a symlinked or path-qualified invocation says something else. | Always `renice:`. | House rule across this coreutils. |
| 3 | The password database is glibc's NSS, so LDAP/systemd-userdb accounts resolve. | `pwdb` -- `/etc/passwd` and `/etc/group`, the same files `id` and `chown` read. | This OS has no NSS. A name absent from those files is `unknown user`. |
| 4 | An operand echoed back in a diagnostic is printed as raw bytes, so a username containing an escape sequence reaches the terminal. | Escaped through `quote::escape_unprintable`. | A diagnostic must not be a way to drive the terminal. Same call as every other bin in this sweep. |
| 5 | `--version` prints `renice from util-linux 2.39.3`. | `renice from SlateOS coreutils 0.1.0`. | Keeps the shape scripts grep for without claiming to be util-linux. Same call as `cal` (`B-CAL-IS-NARROWER-THAN-UPSTREAM-IN-FIVE-PLACES`). |

**Everything else was measured and matched**, including the parts that look
like bugs and are not:

- `-n` is **absolute**, not relative, unless `POSIXLY_CORRECT` is set in the
  environment -- upstream's own comment calls this incorrect and says it has
  been that way since 2009. `--relative` and `--priority` are the unambiguous
  spellings.
- `-h`, `--help`, `-v`, `-V` and `--version` are honoured **only as the whole
  command line**. `renice -h -p 1` is not help; it is `invalid priority '-h'`.
- The priority word goes through C's `strtol` in base 10, so `renice ' 5' 1`
  and `renice +5 1` are accepted, `renice 0x10 1` is not, and -- the one that
  surprises -- `renice '' 1` **is accepted with priority 0** while
  `renice ' ' 1` is refused, because `strtol` leaves its end pointer at the
  start when it converts nothing.
- An out-of-range priority is not an error: `strtol` saturates to `LONG_MAX`
  and the value is then truncated to `int`. `renice 2147483647 $$` prints
  `new priority 19`, because the *kernel* clamps and the program reads the
  priority back after setting it.
- A target is an `int`, so `renice 0 4294967296` renices process **0** and
  `renice 0 2147483648` is `bad process ID value: 2147483648` -- and the
  message prints the word as typed, not the truncated number.
- `-p`, `-g` and `-u` are sticky mode words that may repeat, and the last one
  before a target wins: `renice 0 -g -p 410` renices process 410.
- A failing target does not stop the ones after it; the status is 1 if any
  failed.
- Nothing takes an option after the first non-option word, and `--` is not
  special: `renice -- 0 $$` is `invalid priority '--'`.

**Where it lives:** `userspace/coreutils/src/bin/renice.rs` -- the "Measured
against util-linux 2.39.3" table and the "Where this deliberately diverges"
list in the module doc, and a test named for each behaviour. Divergence 1 is
pinned by `a_successful_lookup_is_not_poisoned_by_an_earlier_bad_operand`.

**What would change this:** #3 stops being a limitation if this OS grows a
name-service layer; the call site is the single `pwdb::Db::load()` in
`run_main`. #1, #2, #4 and #5 are settled policy and should not be revisited
without changing the corresponding rule for every other bin in this sweep.
