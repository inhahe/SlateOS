#!/usr/bin/env bash
# Differential test: our `env` against GNU env.
#
# ## The trap this subject sets, and why the harness is shaped around it
#
# Every other harness here reaches its subject through `$bindir/ours/NAME` or
# `$bindir/gnu/NAME`, two directories that are the whole of `PATH` for one
# invocation, so that `argv[0]` is the bare word on both sides.
#
# `env` cannot be run that way, because **`env` prints its environment, and its
# environment contains `PATH`**. With the usual arrangement the two sides are
# handed different `PATH` values by construction, so every case that dumps the
# environment differs -- on the harness's own scaffolding, not on anything the
# programs did. A harness that then "normalised PATH away" would be hiding a
# difference in exactly the variable most worth comparing.
#
# So the subject is reached through **one** directory whose single entry is
# re-pointed between the two runs. Both sides see byte-identical `PATH`, both
# get `argv[0] == "env"`, and nothing has to be filtered out of the comparison.
#
# The general form, worth having written down: a harness may not put its own
# identity into the subject's input. It is only visible here because `env` is
# the program whose entire job is to show you that input.
#
# ## Why the environment is built with `-i` and pinned
#
# The inherited environment of a WSL shell is not reproducible -- it carries
# `WSL_DISTRO_NAME`, a `WSLENV`, a `SHLVL` that depends on nesting depth, and
# whatever the invoking Windows session exported. Each side is therefore started
# from an empty environment with a fixed set built on top, so that "what `env`
# prints" is a property of the program and not of who ran it.
#
# ## Why the reference is built rather than installed
#
# `env` is GNU coreutils, so §726 applies: WSL's installed copy is Ubuntu's
# patched `9.4-3ubuntu6.3`, and a green run against it would certify agreement
# with Debian. `DIFF_GNU_SOURCE` builds 9.4 from source.
#
# ## Why `od -An -c`
#
# `env -0` separates with NUL rather than newline, which is the whole point of
# that option and is invisible to any comparison that reads lines.
set -u

DIFF_PROG='env'
DIFF_GNU_SOURCE=9.4
# Every invocation below is bounded, both sides. `env` executes an arbitrary
# command, so an unbounded harness inherits whatever that command does -- and
# one of the cases below deliberately runs a program that does not exist, which
# is a class of thing worth being unable to hang on.
DIFF_NEED=timeout
# The bindir is still built (the two symlinked copies are what `subject` points
# at), but the invocation below does not use it.
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# The one directory both sides are reached through. `PATH` is this, identically,
# for ours and for GNU; only what the symlink points at changes.
subject=$DIFF_TMP/subject
mkdir -p "$subject"

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# A directory to chdir into, for -C.
mkdir -p target/inner
printf 'marker\n' > target/marker.txt

# The fixed environment. Deliberately small and deliberately awkward: an empty
# value, a value with a space, a value containing `=`, and a name that sorts
# before and after the others, because `env` does not sort and the order it
# prints in is part of what is being compared.
run_side() {
  local side=$1; shift
  # Re-point the single entry rather than switching directories. `ln -sf` on an
  # existing symlink to a directory would create the link *inside* it, so the
  # old one is removed first.
  rm -f "$subject/env"
  ln -s "$bindir/$side/env" "$subject/env"
  # Written out here rather than built by a helper and expanded.
  #
  # The first version used `$(env_args)`, and the unquoted expansion split
  # `WITH_SPACE=a b` into two words -- so the outer `env` read `b` as the
  # COMMAND TO RUN and `env "$@"` as its arguments. Every case then failed
  # identically on both sides and the harness reported "59 passed, 0 differed"
  # having compared nothing at all.
  #
  # What caught it was the two `xfail` cases. `--help` and `--version` are
  # required to DIFFER, because that text is ours; they came back XPASS, which
  # is the only signal in the output that anything was wrong. A harness needs at
  # least one case that must fail for the same reason a gate needs a refusal
  # probe -- without it, "everything agreed" and "nothing ran" print the same
  # line.
  diff_run timeout -k 2 15 /usr/bin/env -i \
    "PATH=$subject:/usr/bin:/bin" \
    "LC_ALL=C.UTF-8" \
    "ZETA=last" \
    "ALPHA=first" \
    "EMPTYVAR=" \
    "WITH_SPACE=a b" \
    "WITH_EQ=x=y" \
    env "$@"
}

compare() {
  local o_out g_out o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  run_side ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
  run_side gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

report() {
  local label="$1"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report "env $*"; }

xfail_case() {
  local why=$1; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS env %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail env %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- printing the environment --------------------------------------------------
run_case
run_case -0
run_case --null
run_case -i
run_case -i -0
run_case --ignore-environment
run_case -

# --- unsetting ------------------------------------------------------------------
run_case -u ZETA
run_case --unset=ZETA
run_case -uZETA
run_case -u ZETA -u ALPHA
run_case -u NOSUCHVAR
run_case -u EMPTYVAR
run_case -u ''
run_case -u 'WITH=EQ'
run_case -u
run_case -u PATH

# --- assignments ------------------------------------------------------------------
run_case NEW=value
run_case NEW=value OTHER=second
run_case ZETA=overwritten
run_case EMPTY2=
run_case 'SPACED=a b c'
run_case 'EQ=x=y=z'
run_case '=novalue'
run_case 'novalue'
run_case -i NEW=only
run_case -i -u ZETA NEW=only

# --- running a command --------------------------------------------------------------
run_case true
run_case false
run_case /bin/true
run_case NEW=value /bin/echo hi
run_case -i /bin/echo hi
run_case /bin/echo -n
run_case nosuchcommand
run_case NEW=value nosuchcommand
run_case /nosuch/path
run_case target
run_case ./target/marker.txt

# --- the end-of-options marker ---------------------------------------------------------
run_case -- /bin/echo hi
run_case -- -u
run_case -- NEW=value
run_case -i -- /bin/echo hi

# --- chdir ------------------------------------------------------------------------------
run_case -C target /bin/pwd
run_case --chdir=target /bin/pwd
run_case -C target/inner /bin/pwd
run_case -C /nosuch /bin/pwd
run_case -C target
run_case -C
run_case --chdir

# --- split-string, which is the option most likely to be absent ---------------------------
run_case -S'/bin/echo hi'
run_case --split-string=/bin/echo hi
run_case -S '/bin/echo one two'
run_case -S''

# --- refusals ------------------------------------------------------------------------------
run_case -Q
run_case --nosuchoption
run_case --unse=ZETA
run_case --ign
run_case --nu
run_case --chd=target /bin/pwd

# --- the two whose text is ours ---------------------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
