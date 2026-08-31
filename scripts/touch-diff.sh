#!/usr/bin/env bash
# Differential test: our `touch` against GNU coreutils'.
#
# ## What is compared
#
# Standard output, standard error, the exit status, and *the timestamps the
# case leaves behind* — every surviving path with its octal mode, its access
# time and its modification time, and for a symlink the text it points at.
#
# The timestamps are not an extra: they are the entire observable effect of
# this program. `touch` writes nothing to standard output in any case below,
# and roughly half the cases here write nothing to standard error either, so a
# text-only harness would compare two empty strings and certify nothing. Every
# question worth asking — did `-a` leave the modification time alone, did `-h`
# stamp the link or its target, did `-r` copy the reference's times or the
# clock's — is a difference in the time columns and nowhere else.
#
# `%A@`/`%T@` and not a formatted date: seconds-and-fraction since the epoch is
# what `utimensat` actually carries, and a formatted time would hide a stamp
# that got the seconds right and threw the nanoseconds away. The fixture pins
# every nanosecond field to a different nine-digit constant for that reason.
#
# ## Why "now" is folded
#
# Most of `touch`'s job is to write the *current* time, and the two sides run
# one after the other, so a raw comparison of those cases would differ by the
# milliseconds between them and say nothing about either program. Dropping
# them from the comparison is not an option either — "did this option leave the
# other timestamp alone, or quietly set it to now?" is the single most common
# way to get `touch` wrong, and it is invisible unless "now" is a value the
# snapshot can print.
#
# So [`fold_now`] rewrites any time later than [`STAMP_CUTOFF`] as the literal
# `now`. That answers *which* of the two happened without asking *when*: a
# preserved time compares as the fixture's constant, a fresh one compares as
# `now`, and the two can never be confused for each other. The fixture pins
# everything it makes to 2001–2007 and the cutoff is 2011, so nothing the
# fixture sets can be mistaken for the clock.
#
# ## Why both sides run inside WSL
#
# The reasons in `cmp-diff.sh`'s header, plus this program's own: `symlink(2)`
# is not available on a Windows host without a privilege the harness must not
# ask for, and `AT_SYMLINK_NOFOLLOW` has no Windows equivalent at all. Our
# `fsattr::set_times` has to open a handle off Unix, opening is inherently a
# follow, and so the whole `-h` section below would be measuring a stub. See
# `touch.rs`'s module docs.
#
# ## Cases that differ on purpose
#
# The family's two — `--help` omits the GNU project's `Report bugs to:` block
# (and the `-d`/`-t` lines, which this implementation does not have), and
# `--version` names SlateOS — and then one per option GNU has and this `touch`
# has not. That second group is an inventory, not a permission: `xfail_case`
# reports an XPASS the moment one starts agreeing, which is what will force it
# to be promoted to a real case when `-d` and `-t` land. `touch.rs`'s module
# docs explain why those two are *refused* rather than ignored — ignoring `-d`
# stamps the file with now instead of with the time that was asked for, which
# is precisely the state the caller was trying to leave.
#
# ## The reference is built, not found
#
# `DIFF_GNU_SOURCE=9.4` makes `diff-wsl.sh` fetch and build coreutils 9.4
# rather than compare against `/usr/bin/touch`, for the reasons in
# `diff-wsl.sh`'s "Why a built reference" and `design-decisions.md` §726. 9.4
# is the version Ubuntu ships, so that a case whose result changes can only be
# attributable to the de-patching rather than to upstream drift.
#
# Run `OURS=/usr/bin/touch ./scripts/touch-diff.sh` to confirm the harness
# still discriminates: it should report every xfail as XPASS and nothing else.
set -u

DIFF_PROG='touch'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

work=$DIFF_TMP/work
mkdir -p "$work"

case_no=0

# --- the fixture --------------------------------------------------------------
# One of everything `touch` treats differently, with every time pinned to a
# distinct constant so that a stamp landing on the wrong file is a visible
# difference rather than two identical numbers.
#
# The times are built with the *system* `touch`, deliberately not with either
# side: the fixture's job is to be identical on both sides before either
# program runs, so building it with the program under test would fold a
# regression into the baseline and hide it.
#
# `-a` and `-m` are set to *different* constants on every file. A fixture whose
# two times agree cannot distinguish "left the access time alone" from "set
# both", which is the whole of what `-a`, `-m` and `--time` do.
#
# The nanosecond fields are all different and all nine digits long, because a
# `timespec` whose `tv_nsec` is written into the wrong field width, or dropped,
# is otherwise a difference of under a second that no seconds-resolution
# comparison would show.
mktree() {
  mkdir -p dir
  printf 'a\n' > file
  ln -s file link
  ln -s nowhere dangling
  # `./` because `readonly` is also a shell builtin, and a redirection whose
  # target is spelled like one is how `foo > kill` gets written when a pipe was
  # meant (SC2238). The prefix names the same file and cannot be misread.
  printf 'ro\n' > ./readonly

  /usr/bin/touch -a -d '2001-02-03 04:05:06.111111111' file
  /usr/bin/touch -m -d '2002-03-04 05:06:07.222222222' file
  /usr/bin/touch -h -a -d '2003-04-05 06:07:08.333333333' link
  /usr/bin/touch -h -m -d '2004-05-06 07:08:09.444444444' link
  /usr/bin/touch -h -a -d '2005-06-07 08:09:10.555555555' dangling
  /usr/bin/touch -h -m -d '2006-07-08 09:10:11.666666666' dangling
  /usr/bin/touch -a -d '2007-08-09 10:11:12.777777777' readonly
  /usr/bin/touch -m -d '2001-09-10 11:12:13.888888888' readonly
  # The directory last: stamping a file inside it would move its own time.
  /usr/bin/touch -a -d '2002-10-11 12:13:14.999999999' dir
  /usr/bin/touch -m -d '2003-11-12 13:14:15.123456789' dir
  chmod 444 readonly
}

# Anything later than this is the harness's own clock rather than the fixture's.
# The fixture pins everything to 2001-2007; the cutoff is 2011.
STAMP_CUTOFF=1300000000

# Rewrite a time the fixture did not set as the literal `now`. See the header.
#
# The two times are matched as one anchored group at the *end* of the record
# and the name in front of them is copied through untouched, which is the only
# shape that is safe here. Two alternatives were tried and are wrong:
#
#   * `$NF = "now"` — assigning to any field makes awk rebuild the whole record
#     with single-space separators, silently rewriting a name that holds a tab.
#     Section 8 creates one.
#   * a per-field `sub(/[AM]=[0-9.]+/, …)` — `sub` without a target operates on
#     `$0` and replaces its *first* match, so folding the M field overwrote the
#     A field instead, and every `-m` case reported `M=now M=1788…`. It looked
#     like a difference between the two programs and was a bug in this
#     function; both sides were in fact identical.
fold_now() {
  # `LC_ALL=C` because section 8 names a file `\377`, which is not valid UTF-8
  # and which a locale-aware awk warns about once per side on stderr. The data
  # here is bytes — that is the point of the case — so bytes is the right
  # locale to read it in, and the warning was noise rather than a finding.
  LC_ALL=C awk -v cut="$STAMP_CUTOFF" '
    {
      if (match($0, / A=[0-9]+\.[0-9]+ M=[0-9]+\.[0-9]+$/)) {
        head = substr($0, 1, RSTART - 1)
        split(substr($0, RSTART + 1), t, " ")
        a = substr(t[1], 3); m = substr(t[2], 3)
        print head " A=" (a + 0 > cut ? "now" : a) \
                   " M=" (m + 0 > cut ? "now" : m)
        next
      }
      print
    }'
}

# What a case leaves behind: every path, its octal mode, its kind, and both of
# its times.
#
# A directory's size is that of the block holding its entries and says nothing
# about what is in it, so `d` stands in. A symlink's mode is 0777 on Linux
# always and its size is the length of its text; the text is what carries
# information, so that is what is printed. Both times are printed for every
# kind, since `-a` and `-m` are exactly the options that move one and not the
# other.
#
# `find` does not follow symlinks, so `dangling` is listed rather than skipped
# and `link`'s own times are reported rather than its target's — which is the
# distinction the whole `-h` section turns on.
snapshot() {
  ( cd "$1" 2>/dev/null && find . -mindepth 1 \
        \( -type d -printf "%P %m d A=%A@ M=%T@\n" \
        -o -type l -printf "%P l -> %l A=%A@ M=%T@\n" \
        -o -printf "%P %m %s A=%A@ M=%T@\n" \) 2>/dev/null \
      | fold_now | LC_ALL=C sort )
}

# And the bytes, so that a `touch` which created a file that was supposed to
# stay missing — or truncated one that was supposed to keep its contents, which
# is what the `set_len` trick this program replaced would have done — is caught
# rather than passing on a matching size.
#
# NUL-separated for the sake of section 8's names, which hold a newline.
contents() {
  ( cd "$1" 2>/dev/null || return 0
    find . -type f -printf '%P\0' 2>/dev/null | LC_ALL=C sort -z \
      | while IFS= read -r -d '' f; do
      printf '== %s\n' "$f"
      cat -- "$f" 2>/dev/null
      printf '\n'
    done )
}

# --- knobs, reset after every case --------------------------------------------

# Shell run inside the case directory to build the fixture.
TREE=
reset_knobs() { TREE='mktree'; }
reset_knobs

scrub() { sed -e "s|$1|<DIR>|g"; }

# --- running one side ---------------------------------------------------------

run_one() {
  local side=$1 dir=$2 out=$3 err=$4 rcf=$5; shift 5
  mkdir -p "$dir"
  ( cd "$dir" && eval "$TREE" ) >/dev/null 2>&1
  (
    cd "$dir" || exit 1
    # Reached as the bare word `touch`, through the one-entry directory
    # `diff-wsl.sh` built, and not by the path of the symlink: gnulib's
    # `set_program_name` takes `argv[0]` whole, so GNU invoked by a long path
    # prefixes every diagnostic with that path while ours prints `touch:`.
    PATH="$bindir/$side:$PATH"
    diff_run timeout -k 2 30 touch "$@" >"$out" 2>"$err"
  ) </dev/null
  echo $? >"$rcf"
  return 0
}

# --- comparing the two sides --------------------------------------------------

judge() {
  local o_dir=$1 g_dir=$2 o_out=$3 g_out=$4 o_extra=$5 g_extra=$6 label=$7
  local o_snap g_snap o_body g_body o_show g_show
  # Before `contents`, which reads every file and so moves the access times the
  # snapshot is there to report.
  o_snap=$(snapshot "$o_dir"); g_snap=$(snapshot "$g_dir")
  o_body=$(contents "$o_dir" | scrub "$o_dir"); g_body=$(contents "$g_dir" | scrub "$g_dir")
  o_show=$(scrub "$o_dir" <"$o_out"); g_show=$(scrub "$g_dir" <"$g_out")
  o_extra=$(printf '%s' "$o_extra" | scrub "$o_dir")
  g_extra=$(printf '%s' "$g_extra" | scrub "$g_dir")

  if [ "$o_show" = "$g_show" ] && [ "$o_extra" = "$g_extra" ] \
     && [ "$o_snap" = "$g_snap" ] && [ "$o_body" = "$g_body" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours: %s\n        out{%s}\n        tree{%s} files{%s}\n  gnu : %s\n        out{%s}\n        tree{%s} files{%s}' \
    "$(printf '%s' "$o_extra" | tr '\n' '|')" "$(printf '%s' "$o_show" | tr '\n' '|')" \
    "$(printf '%s' "$o_snap" | tr '\n' '|')" "$(printf '%s' "$o_body" | tr '\n' '|')" \
    "$(printf '%s' "$g_extra" | tr '\n' '|')" "$(printf '%s' "$g_show" | tr '\n' '|')" \
    "$(printf '%s' "$g_snap" | tr '\n' '|')" "$(printf '%s' "$g_body" | tr '\n' '|')")
  LABEL=$label
}

compare() {
  case_no=$((case_no+1))
  local o_dir=$work/o$case_no g_dir=$work/g$case_no
  local o_out=$work/oo$case_no g_out=$work/go$case_no
  local o_err=$work/oe$case_no g_err=$work/ge$case_no
  local o_rc=$work/or$case_no g_rc=$work/gr$case_no
  local label="touch $*"
  [ "$TREE" = mktree ] || label="$label   [tree: $TREE]"
  run_one ours "$o_dir" "$o_out" "$o_err" "$o_rc" "$@"
  run_one gnu  "$g_dir" "$g_out" "$g_err" "$g_rc" "$@"
  judge "$o_dir" "$g_dir" "$o_out" "$g_out" \
    "rc=$(cat "$o_rc") err{$(cat "$o_err")}" \
    "rc=$(cat "$g_rc") err{$(cat "$g_err")}" \
    "$label"
  reset_knobs
}

report() {
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$LABEL" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

missing() { xfail_case "not implemented by this touch" "$@"; }

echo "touch-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# =============================================================================
# 1. Operands, and the absence of them
# =============================================================================
# The old implementation parsed no options at all and returned every word as a
# file name, so these are the cases that would have caught it: each one below
# is a word that must not become a file.

run_case
run_case ''
run_case file
run_case newfile
run_case file newfile dir
# An operand naming a directory. GNU exits 0 even though the open fails with
# EISDIR, because the stamp is attempted on the path regardless.
run_case dir
run_case nosuchdir/f
# One failure must not abandon the rest, and must still count against the
# status.
run_case nosuchdir/f newfile
run_case newfile nosuchdir/f

# =============================================================================
# 2. Option errors, and where the option table is observable
# =============================================================================
# `--n` and `--no` are ambiguous only because `LONG_OPTIONS` carries
# `--no-dereference` beside `--no-create`. They are the cases that fail if
# someone prunes the table down to the options this implementation acts on,
# which would silently turn `touch --no` from an error into `--no-create`.

run_case -q file
run_case --nosuchoption file
run_case --no-create=yes file
run_case --n file
run_case --no file
run_case --no-c file
run_case --no-d file
run_case --no-dereference file
run_case --t file
run_case --=x file
run_case --time file
run_case --time= file
run_case --time=nosuchword file
run_case --time=a file
run_case --time=m file
run_case -r
run_case --reference
# The value-taking options this `touch` refuses are nonetheless *declared* as
# taking a value, so a missing one is `option requires an argument` and not the
# refusal. Both sides agree, which is the point: these are real cases rather
# than members of the inventory in section 11. They were written as `missing`
# first and the harness reported them as XPASS, which is what that machinery is
# for.
run_case -d
run_case -t
run_case --date

# =============================================================================
# 3. Which of the two times is written
# =============================================================================
# Every case here is a claim that the *other* time was left alone, which is
# what `UTIME_OMIT` is for and what a read-then-write implementation would get
# subtly wrong. The fixture gives `file` two different constants so that
# "left alone" and "set to the same thing" cannot be confused.

run_case -a file
run_case -m file
run_case -am file
run_case -a -m file
run_case --time=access file
run_case --time=atime file
run_case --time=use file
run_case --time=modify file
run_case --time=mtime file
# `--time` and the short spellings accumulate rather than replacing.
run_case -a --time=modify file
run_case --time=access -m file

# =============================================================================
# 4. -c, --no-create
# =============================================================================
# `-c` on a missing file is a *silent success*, not a suppressed error — the
# case `-c` exists for. On a file that is there it changes nothing at all.

run_case -c nosuch
run_case --no-create nosuch
run_case -c file
run_case -c -a file
run_case -c nosuchdir/f
run_case -c nosuch file

# =============================================================================
# 5. -r, --reference
# =============================================================================
# Measured: `touch -r /nope` reports the reference and exits 1 **even with no
# operands at all**, which pins the order of the two checks — the reference is
# read before the missing-operand check, not after.

run_case -r file newfile
run_case -r file dir
run_case --reference=file newfile
run_case --reference file newfile
run_case -r nosuch newfile
run_case -r nosuch
run_case -r file
run_case -r dir newfile
# Only the named half of the reference's times is copied.
run_case -r file -a newfile
run_case -r file -m newfile
# `-r` on a symlink follows it without `-h`; the `-h` half is section 7.
run_case -r link newfile
run_case -r dangling newfile

# =============================================================================
# 6. -f, and the option that is ignored on purpose
# =============================================================================
# GNU documents `-f` as accepted and ignored — it is compatibility ballast for
# a BSD `touch` that once had it — so ignoring it *is* the implementation, and
# it must not join the refused inventory by accident.

run_case -f file
run_case -f -a file
run_case -acf nosuch

# =============================================================================
# 7. -h, --no-dereference
# =============================================================================
# `-h` does three separable things, and each of the three is a different column
# here. It selects the *link* rather than its target (the `link` and `dangling`
# rows swap with `file`'s and `nowhere`'s); it suppresses the create-open, so
# `-h` on a missing name leaves nothing behind and `-h` on a dangling link does
# not create the far end; and it makes `-r` read the reference with `lstat`, so
# a dangling reference is fatal without it and a copied pair of times with it.
#
# The without-`-h` twin of each case is here on purpose. A `-h` wired to the
# wrong sense still passes one of every pair, and only the pair pins it.

run_case -h link
run_case link
run_case -h -a link
run_case -h -m link
run_case -h --time=modify link
run_case --no-dereference link
run_case --no-der link

# The dangling link: without `-h` the far end is *created*, which is the
# clearest statement that the stamp followed.
run_case -h dangling
run_case dangling
run_case -hc dangling

# `-h` skips the open exactly as `-c` does, but forgives nothing: measured,
# `touch -h nosuch` fails with `setting times of` while `touch -hc nosuch` is
# silent at 0. Collapsing the two into one condition is the obvious
# simplification and it is wrong.
run_case -h nosuch
run_case -hc nosuch
run_case -h -c nosuch
run_case -c nosuch
# And the diagnostic is `setting times of`, never `cannot touch`: with no open
# there is no open error to report.
run_case -h nosuchdir/f

# `-h` on things that are not symlinks at all, where it must change nothing.
run_case -h file
run_case -h dir
run_case -h readonly

# `-r` read with `lstat`. The dangling reference is the pair that turns an
# error into a success rather than one time into another.
run_case -h -r link newfile
run_case -r link newfile
run_case -h -r dangling newfile
run_case -r dangling newfile
run_case -h -r link -a newfile
# The reference is a link and so is the operand, which is the case where
# getting either half backwards still moves *a* timestamp.
run_case -h -r link dangling
run_case -r link dangling

# =============================================================================
# 8. Names, and the bytes in them
# =============================================================================
# A file name may hold every byte but `/` and NUL (`design.txt`), so argv is
# read as `OsString` and stays bytes all the way to the syscall. Reading it as
# `String` panics on the third case here, which is defect 2 in the module docs.
#
# The newline case is also the one that says a name cannot forge a second
# diagnostic line: quoted, it is one line with a `\n` in it.

TREE="mktree; printf x > 'a b'"
run_case 'a b'
TREE="mktree; printf x > \$'a\\tb'"
run_case $'a\tb'
TREE='mktree; printf x > "$(printf %b "\\xff")"'
run_case "$(printf '\377')"
run_case "$(printf 'a\nb')"
run_case nosuchdir/"$(printf 'a\ntouch: /etc: Permission denied')"
run_case -- -a
run_case ./-a
# `--` ends options; it does not stop `-` meaning standard output.
run_case -- -
run_case -

# =============================================================================
# 9. The read-only file, and the directory
# =============================================================================
# These are one rule: GNU always calls `utimensat` on the path whether or not
# the open worked, and reports the open's error only when the stamp failed too.
# That is why `touch` on a file you may not write still succeeds, and it is
# what the implementation this replaced could not do, because it stamped
# through a handle it had to open first.

run_case readonly
run_case -a readonly
run_case -c readonly
run_case -r file readonly
run_case dir readonly file

# =============================================================================
# 10. Options after operands, and bundles
# =============================================================================
# `getopt_long` permutes by default, so `touch a -c b` is `touch -c a b`.

run_case file -a
run_case newfile -c
run_case -a file -m newfile
run_case -ah link
run_case -ha link
run_case -amc nosuch
run_case -r file -- newfile
run_case -a -- -m

# =============================================================================
# 11. Options this touch does not have
# =============================================================================
# An inventory rather than a permission: each entry names the option, and a
# case that starts agreeing is reported as an XPASS, which is what will force
# it to be promoted when `-d` and `-t` land. Both are blocked on a date parser
# this crate genuinely lacks — `-d` on `parse_datetime`, `-t` on
# civil-time-to-epoch conversion in the local zone including its history.
#
# The value is passed even though the option is refused, because `-d` is
# declared as taking one: that is what makes `touch -d` answer `option requires
# an argument` rather than jumping to the refusal, and what stops the
# `2001-01-01` in `touch -d 2001-01-01 f` being left behind to be created as a
# file.

missing -d now file
missing --date=now file
missing --date now file
missing -d '@1000000000' newfile
missing -t 202001010000 file
missing -t 202001010000.30 newfile

# =============================================================================
# 12. --help and --version
# =============================================================================

xfail_case 'help omits the -d/-t lines and GNU bug-report block' --help
xfail_case 'version names SlateOS' --version
# Measured: an option *after* `--help` is never looked at, while one before it
# is an error. Both sides agree on the second, so it is a real case.
run_case --bogus --help

# The wording is the family's, not this harness's own: `scripts/all-diff.sh`
# decides green by matching " 0 differed" in the tail line, so a summary that
# said "0 failed" would be reported as a failing harness forever.
printf '\ntouch: %d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
exit 0
