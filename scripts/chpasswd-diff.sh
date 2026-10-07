#!/usr/bin/env bash
# Differential test: our `chpasswd` against Ubuntu 24.04's (shadow-utils 4.13).
#
# ## How both sides run
#
# `chpasswd` edits the account files under `/etc`, so every case runs in a
# fake root of its own, entered with the program's own `-R`, under
# `unshare -r`: a user namespace in which the harness is root, so `chroot`
# works and nothing outside the directory can be touched. The real `/etc` is
# not writable from inside it either, which is what makes a case that
# misses its `-R` fail safe rather than edit the machine.
#
# Both sides run in the *same* directory, one after the other, so a path in a
# diagnostic is the same path on both.
#
# ## The two sides read different files, from one fixture
#
# Ubuntu's edits `/etc/passwd` and `/etc/shadow`. Ours edits
# `/etc/users.yaml` and writes those two from it (design-decisions §353). So
# the fixture is written once, as a database, and our program -- given
# nothing to change -- generates the flat files from it. Ubuntu's side starts
# from those, comment lines removed. The two therefore begin from the same
# `/etc/shadow`, byte for byte, and what is compared afterwards is the
# `/etc/shadow` each leaves: for ours the generated one, which is what a
# ported program would read.
#
# ## What is compared
#
# The status, standard output and standard error, and `/etc/shadow`. A
# password hashed during the run gets a random salt on each side, so a hash
# is compared by its shape -- the method and its parameters as written, and
# the salt's and the hash's lengths -- and each case names the passwords that
# must verify, which are checked against each side's entry with Perl's
# `crypt`, libxcrypt's.
#
# `SOURCE_DATE_EPOCH` is set for every case, so both sides stamp the same day.
#
# ## Cases that differ on purpose
#
# Recorded as `xfail`, each with its reason. They are `chpasswd`'s deliberate
# differences 1, 2, 3, 5, 6 and 9: the database's name in a message, no PAM,
# no `login.defs`, a locked account staying locked, a value that is not
# UTF-8, and a value that is not a hash showing as `*` in the generated
# `/etc/shadow`.
#
# Deliberate difference 4, a fresh salt for every password, is not a case:
# the shapes agree, and the salts are random on both sides.
set -u

DIFF_PROG='chpasswd'
DIFF_PKG='chpasswd'
DIFF_REF='/usr/sbin/chpasswd'
DIFF_NEED='unshare perl timeout'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# Upstream refuses a time later than now, so this is in the past: 2023-11-14,
# day 19675.
EPOCH=1700000000

# --- the fixture ---------------------------------------------------------------
# Entries for "old" under fixed salts, so the fixture is the same every run.
crypt_with() { perl -e 'print crypt($ARGV[0], $ARGV[1])' "$1" "$2"; }
ALICE_OLD=$(crypt_with old '$6$alicesaltalice$')
BOB_OLD=$(crypt_with old '$6$bobsaltbobsalt$')
DAVE_OLD=$(crypt_with old '$1$davesalt$')

# alice: an ordinary account with aging fields. bob: locked by a `!` in front
# of his entry, the way `/etc/shadow` spells a lock. carol: locked by the
# database's own `locked: true`, which the generated shadow also spells with a
# `!`. dave: an MD5 entry and no aging at all. root: no password.
seed_yaml() {
  cat <<EOF
users:
  - uid: 0
    username: "root"
    gid: 0
    home_dir: "/root"
    shell: "/bin/bash"
    password_hash: "*"
    password_changed: 19000
  - uid: 1000
    username: "alice"
    gid: 1000
    home_dir: "/home/alice"
    shell: "/bin/bash"
    password_hash: "$ALICE_OLD"
    password_changed: 19500
    password_min_days: 0
    password_max_days: 99999
    password_warn_days: 7
  - uid: 1001
    username: "bob"
    gid: 1001
    home_dir: "/home/bob"
    shell: "/bin/sh"
    password_hash: "!$BOB_OLD"
    password_changed: 19600
  - uid: 1002
    username: "carol"
    gid: 1002
    home_dir: "/home/carol"
    shell: "/bin/sh"
    password_hash: "$ALICE_OLD"
    locked: true
  - uid: 1003
    username: "dave"
    gid: 1003
    home_dir: "/home/dave"
    shell: "/bin/sh"
    password_hash: "$DAVE_OLD"
EOF
}

seed=$DIFF_TMP/seed
mkdir -p "$seed/etc"
seed_yaml >"$seed/etc/users.yaml"
chmod 600 "$seed/etc/users.yaml"
# Nothing to change, so this only saves the database -- which writes the two
# flat files. If it cannot, nothing below means anything.
if ! printf '' | unshare -r "$bindir/ours/chpasswd" -e -R "$seed" 2>"$DIFF_TMP/seed.err"; then
  echo "chpasswd-diff: our chpasswd could not generate the fixture:" >&2
  cat "$DIFF_TMP/seed.err" >&2
  exit 1
fi
for f in passwd shadow; do
  if [ ! -s "$seed/etc/$f" ]; then
    echo "chpasswd-diff: the fixture has no /etc/$f" >&2
    exit 1
  fi
done

# A fresh root for one side, at the one path both sides use.
case_root=$DIFF_TMP/root
fresh_root() {
  rm -rf "$case_root"
  mkdir -p "$case_root/etc"
  if [ "$1" = ours ]; then
    cp -p "$seed/etc/users.yaml" "$seed/etc/passwd" "$seed/etc/shadow" "$case_root/etc/"
  else
    grep -v '^#' "$seed/etc/passwd" >"$case_root/etc/passwd"
    grep -v '^#' "$seed/etc/shadow" >"$case_root/etc/shadow"
    chmod 600 "$case_root/etc/shadow"
  fi
}

# `/etc/shadow` with every hash reduced to its shape: `$id$params$` as
# written, then the salt's and the hash's lengths. A thirteen-character DES
# hash is `<des 13>`. Anything else -- `*`, `!`, `*0`, a plaintext -- is kept.
shadow_shape() {
  grep -v '^#' "$1" 2>/dev/null | perl -ne '
    chomp;
    my @f = split /:/, $_, -1;
    my $h = $f[1] // "";
    my $lock = "";
    if ($h =~ s/^(!+)//) { $lock = $1; }
    if ($h =~ /^\$/) {
      my @p = split /\$/, $h, -1;
      if (@p >= 4) {
        $p[-1] = "<" . length($p[-1]) . ">";
        $p[-2] = "<" . length($p[-2]) . ">";
        $h = join("\$", @p);
      }
    } elsif (length($h) == 13 && $h =~ m{^[./0-9A-Za-z]+$}) {
      $h = "<des 13>";
    }
    $f[1] = $lock . $h;
    print join(":", @f), "\n";
  '
}

# Whether `user`'s entry in shadow file $1 verifies password $3.
verifies() {
  local hash
  hash=$(grep -v '^#' "$1" | awk -F: -v u="$2" '$1 == u { print $2 }')
  perl -e 'exit(crypt($ARGV[0], $ARGV[1]) eq $ARGV[1] ? 0 : 1)' "$3" "$hash"
}

# --- one case ------------------------------------------------------------------
# `INPUT` is a `printf %b` string, or `INPUT_FILE` a file to read instead.
# `VERIFY` is `user=password ...`: passwords that must verify on both sides
# afterwards. `EPOCH_VAR` overrides the `SOURCE_DATE_EPOCH` given (`unset` for
# none). `NOSHADOW` leaves `/etc/shadow` out of the comparison, for the few
# cases whose stored value can only be plain text -- an 8,184-byte password,
# which no hash method takes -- and so would be compared through `userdb`'s `*`
# (deliberate difference 9) rather than tested. In the arguments, `@ROOT@` is
# the case's root directory.
INPUT=; INPUT_FILE=; VERIFY=; EPOCH_VAR=$EPOCH; NOSHADOW=
reset_knobs() { INPUT=; INPUT_FILE=; VERIFY=; EPOCH_VAR=$EPOCH; NOSHADOW=; }

run_one() {
  local side=$1; shift
  local args=() a
  for a in "$@"; do args+=("${a//@ROOT@/$case_root}"); done
  fresh_root "$side"
  local envs=(LC_ALL=C.UTF-8 "PATH=$bindir/$side:$PATH")
  [ "$EPOCH_VAR" != unset ] && envs+=("SOURCE_DATE_EPOCH=$EPOCH_VAR")
  if [ -n "$INPUT_FILE" ]; then
    env -i "${envs[@]}" timeout -k 2 60 unshare -r chpasswd "${args[@]}" \
      <"$INPUT_FILE" >"$DIFF_TMP/$side.out" 2>"$DIFF_TMP/$side.err"
  else
    printf '%b' "$INPUT" | env -i "${envs[@]}" timeout -k 2 60 unshare -r chpasswd "${args[@]}" \
      >"$DIFF_TMP/$side.out" 2>"$DIFF_TMP/$side.err"
  fi
  printf '%s' "$?" >"$DIFF_TMP/$side.rc"
  shadow_shape "$case_root/etc/shadow" >"$DIFF_TMP/$side.shadow"
  local pair bad=
  for pair in $VERIFY; do
    verifies "$case_root/etc/shadow" "${pair%%=*}" "${pair#*=}" || bad="$bad ${pair%%=*}"
  done
  printf '%s' "$bad" >"$DIFF_TMP/$side.unverified"
}

compare() {
  run_one ours "$@"
  run_one gnu "$@"
  local s same=yes compared='rc out err shadow unverified'
  [ -n "$NOSHADOW" ] && compared='rc out err unverified'
  for s in $compared; do
    cmp -s "$DIFF_TMP/ours.$s" "$DIFF_TMP/gnu.$s" || same=no
  done
  # A password that fails to verify on *both* sides is not agreement.
  [ -s "$DIFF_TMP/ours.unverified" ] && same=no
  AGREED=$same
  REPORT=$(
    for side in ours gnu; do
      printf '  %-4s rc=%s out{%s} err{%s} unverified{%s}\n' "$side" \
        "$(cat "$DIFF_TMP/$side.rc")" "$(tr '\n' '|' <"$DIFF_TMP/$side.out")" \
        "$(tr '\n' '|' <"$DIFF_TMP/$side.err")" "$(cat "$DIFF_TMP/$side.unverified")"
    done
    diff "$DIFF_TMP/gnu.shadow" "$DIFF_TMP/ours.shadow" | sed 's/^/    shadow /'
  )
  reset_knobs
}

label() { printf 'chpasswd %s' "$*"; }

check() {
  local name; name=$(label "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK    %s\n' "$name"
  else
    fail=$((fail + 1))
    printf 'DIFF  %s\n%s\n' "$name" "$REPORT"
  fi
  return 0
}

xcheck() {
  local why=$1; shift
  local name; name=$(label "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$name" "$why"
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n%s\n' "$name" "$why" "$REPORT"
  fi
  return 0
}

# =============================================================================
# Hashes to store with `-e`. A value that is not one would be compared through
# `userdb`'s `*` (deliberate difference 9), so every `-e` case below that is
# not about that stores a real one.
H1=$(crypt_with one '$1$salt1234$')
H2=$(crypt_with two '$1$salt5678$')
H6=$(crypt_with new '$6$freshsaltfresh$')

# --- passwords stored as given ------------------------------------------------
INPUT="alice:$H6\n"; check -e -R @ROOT@
INPUT="alice:$H1\ndave:$H2\n"; check -e -R @ROOT@
# Each password once more, last one winning.
INPUT="alice:$H1\nalice:$H2\n"; check -e -R @ROOT@
# `--encrypted` and `--root`, spelled long.
INPUT="alice:$H1\n"; check --encrypted --root @ROOT@
INPUT="alice:$H1\n"; check --encrypted --root=@ROOT@
# A locked entry stored as such.
INPUT="alice:!$ALICE_OLD\n"; check -e -R @ROOT@
# bob's `!` goes with his old entry, on both sides.
INPUT="bob:$H1\n"; check -e -R @ROOT@
# The last line needs no newline, and an operand is not read.
INPUT="alice:$H1"; check -e -R @ROOT@
INPUT="alice:$H1\n"; check -e -R @ROOT@ ignored-operand
# Nothing to read: nothing changes, and the save still happens.
INPUT=''; check -e -R @ROOT@

# --- passwords hashed: each method's shape, and the password verifies ---------
INPUT='alice:new\n'; VERIFY='alice=new'; check -c SHA512 -R @ROOT@
INPUT='alice:new\n'; VERIFY='alice=new'; check -c SHA256 -R @ROOT@
INPUT='alice:new\n'; VERIFY='alice=new'; check -c MD5 -R @ROOT@
INPUT='alice:new\n'; VERIFY='alice=new'; check -m -R @ROOT@
INPUT='alice:new\n'; VERIFY='alice=new'; check -c DES -R @ROOT@
INPUT='alice:new\n'; VERIFY='alice=new'; check -c YESCRYPT -R @ROOT@
INPUT='alice:a\ndave:d\n'; VERIFY='alice=a dave=d'; check -c SHA512 -R @ROOT@
# Exactly the bytes after the first colon: a second colon, blanks, a CR.
INPUT='alice:p:q\n'; VERIFY='alice=p:q'; check -c SHA512 -R @ROOT@
# (`VERIFY` is split on blanks, so this one is compared by its shape alone.)
INPUT='alice: spaced \n'; check -c MD5 -R @ROOT@
INPUT='alice:\n'; VERIFY='alice='; check -c SHA512 -R @ROOT@
INPUT='alice:crlf\r\n'; check -c MD5 -R @ROOT@
# DES reads eight characters of it.
INPUT='alice:12345678ninth\n'; VERIFY='alice=12345678'; check -c DES -R @ROOT@

# --- rounds and cost, and where `-s` is read --------------------------------
INPUT='alice:r\n'; VERIFY='alice=r'; check -c SHA512 -s 9000 -R @ROOT@
INPUT='alice:r\n'; VERIFY='alice=r'; check -c SHA256 -s 1000 -R @ROOT@
INPUT='alice:r\n'; check -c SHA512 -s 5000 -R @ROOT@
INPUT='alice:r\n'; check -c SHA512 -s 0 -R @ROOT@
INPUT='alice:r\n'; check -c SHA512 -s 10 -R @ROOT@
INPUT='alice:r\n'; check -c SHA512 -s 0x1000 -R @ROOT@
INPUT='alice:r\n'; check -c SHA512 -s 010000 -R @ROOT@
# A negative count is a huge `unsigned long` upstream, so the most rounds:
# 999,999,999 for SHA-crypt, which takes minutes to hash, so only yescrypt's
# cost is run here (the unit tests pin SHA-crypt's setting).
INPUT='alice:r\n'; VERIFY='alice=r'; check -c YESCRYPT -s -1 -R @ROOT@
INPUT='alice:r\n'; check -c SHA512 -s 4294967297 -R @ROOT@
INPUT='alice:r\n'; check -c SHA512 --sha-rounds=7000 -R @ROOT@
# Before its method, `-s` is taken and never read.
INPUT='alice:r\n'; check -s 9000 -c SHA512 -R @ROOT@
INPUT='alice:r\n'; check -s banana -c SHA512 -R @ROOT@
INPUT='alice:r\n'; check -c MD5 -s banana -R @ROOT@
INPUT='alice:r\n'; check -c SHA512 -s 9000 -c SHA256 -R @ROOT@
INPUT='alice:r\n'; VERIFY='alice=r'; check -c YESCRYPT -s 1 -R @ROOT@
INPUT='alice:r\n'; check -c YESCRYPT -s 3 -R @ROOT@
INPUT='alice:r\n'; check -c YESCRYPT -s 11 -R @ROOT@
INPUT='alice:r\n'; check -c YESCRYPT -s 99 -R @ROOT@

# --- lines that are refused, and then nothing changes -------------------------
INPUT='nobody:x\n'; check -e -R @ROOT@
INPUT='alice:x\nnobody:x\n'; check -c MD5 -R @ROOT@
INPUT='no colon\n'; check -e -R @ROOT@
INPUT=':x\n'; check -e -R @ROOT@
INPUT='\n'; check -e -R @ROOT@
INPUT='alice:x\n\nbob:y\nnobody:z\n'; check -c MD5 -R @ROOT@
# A NUL ends the line as C reads it: "too long", and the drain takes the next.
INPUT='alice:a\0b\nnobody:x\nbob:y\n'; check -e -R @ROOT@
# fgets into BUFSIZ: a line of 8191 bytes before its newline is too long, one
# of 8190 is not.
long_line() {
  printf 'alice:'
  head -c "$(( $1 - 6 ))" /dev/zero | tr '\0' 'a'
  printf '\n'
  printf '%b' "${2:-}"
}
long_line 8190 >"$DIFF_TMP/fits"; INPUT_FILE=$DIFF_TMP/fits; NOSHADOW=1; check -e -R @ROOT@
long_line 8191 "dave:$H1\n" >"$DIFF_TMP/long"; INPUT_FILE=$DIFF_TMP/long; check -e -R @ROOT@
long_line 20000 'nobody:x\n' >"$DIFF_TMP/longer"; INPUT_FILE=$DIFF_TMP/longer; check -e -R @ROOT@
# libxcrypt hashes fewer than 512 bytes of passphrase, and its failure token
# is a missing method under `$` and the stored entry under DES.
long_line 517 >"$DIFF_TMP/p511"; INPUT_FILE=$DIFF_TMP/p511; check -c MD5 -R @ROOT@
long_line 518 >"$DIFF_TMP/p512"; INPUT_FILE=$DIFF_TMP/p512; check -c MD5 -R @ROOT@
long_line 518 >"$DIFF_TMP/p512"; INPUT_FILE=$DIFF_TMP/p512; check -c SHA512 -R @ROOT@

# --- the day the password changed ------------------------------------------------
INPUT="alice:$H1\n"; EPOCH_VAR=864000; check -e -R @ROOT@
INPUT="alice:$H1\n"; EPOCH_VAR=5; check -e -R @ROOT@
INPUT="alice:$H1\ndave:$H2\n"; EPOCH_VAR=soon; check -e -R @ROOT@
INPUT="alice:$H1\n"; EPOCH_VAR=12x; check -e -R @ROOT@
INPUT="alice:$H1\n"; EPOCH_VAR=''; check -e -R @ROOT@
INPUT="alice:$H1\n"; EPOCH_VAR=99999999999999999999; check -e -R @ROOT@
INPUT='nobody:x\n'; EPOCH_VAR=soon; check -e -R @ROOT@

# --- the command line -----------------------------------------------------------
check --help
check -h -x
check -x
check --bogus
check --he
check -c
check -c sha512
check -c BCRYPT
check -s 9000
check -e -m
check -e -c MD5
check -m -c MD5
check -c SHA512 -s banana
check -c SHA512 -s ''
check -c SHA512 -s 08
INPUT='alice:r\n'; check -c SHA512 -s ' 7' -R @ROOT@
check -c YESCRYPT -s 9x
check -R relative
check -R /no/such/lane-b-root
check -R @ROOT@ --root=@ROOT@
check -R
check -e --root

# =============================================================================
# --- differences on purpose ---------------------------------------------------
# 5: carol's lock is the database's `locked: true`, which the new password
# leaves standing, with a note; upstream's lock was the `!` it wrote over.
INPUT="carol:$H1\n"; xcheck 'a locked account stays locked (difference 5)' -e -R @ROOT@
# 1: a colon in a stored value fails the save on both sides, nothing
# changed; the message names the file each was writing.
INPUT='alice:pa:ss\n'; xcheck 'the database is /etc/users.yaml (difference 1)' -e -R @ROOT@
# 6: a value that is not UTF-8 cannot be held by the database, and fails on
# its line; upstream writes it.
INPUT='alice:\377\n'; xcheck 'a value that is not UTF-8 fails its line (difference 6)' -e -R @ROOT@
# 2 and 3: with none of -c, -e and -m, Ubuntu's asks PAM, which has no
# configuration in the fake root; ours hashes with the method new passwords get.
INPUT='alice:new\n'; xcheck 'no PAM and no login.defs (differences 2 and 3)' -R @ROOT@
# 9: a value that is not a hash is kept in the database, and the generated
# /etc/shadow shows `*` for it; upstream writes it into /etc/shadow. These are
# the same runs with the same status and messages otherwise.
why9='a non-hash entry is * in the generated /etc/shadow (difference 9)'
INPUT='alice:plain text\n'; xcheck "$why9" -c NONE -R @ROOT@
INPUT='alice:plain\n'; xcheck "$why9" --crypt-method NONE --root @ROOT@
INPUT='alice:plain\n'; xcheck "$why9" --crypt-method=NONE --root=@ROOT@
INPUT='alice:x\n'; xcheck "$why9" -e -R @ROOT@
INPUT='alice:tab\there\n'; xcheck "$why9" -e -R @ROOT@
# The failure token a passphrase of 512 bytes leaves under DES, `*0`.
long_line 518 >"$DIFF_TMP/p512"; INPUT_FILE=$DIFF_TMP/p512; xcheck "$why9" -c DES -R @ROOT@

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ]
