#!/bin/sh
# The reference half of `scripts/option-gap.sh`, run where the reference lives
# (inside WSL on a Windows host). One line per short option the program's own
# `--help` mentions:
#
#   <name> <opt> ok        the reference did NOT call it an unknown option
#   <name> <opt> reject    the reference refused it as unknown
#   <name> -    nognu      no such program here
#   <name> -    nohelp     it has no --help output to read options from
#
# Separate file rather than a here-doc inside the caller, and that is not
# style: piping this into `sh -s` makes stdin the script, and the first program
# whose `--help` reads standard input consumes the rest of it. That happened --
# the sweep stopped after 17 of 72 programs and printed its partial findings
# with no indication they were partial. Every invocation below therefore takes
# `</dev/null`, and the caller runs this from a PATH.
#
# `timeout` on each, because running a program with a VALID option makes it do
# its job rather than complain: `yes -x` prints for ever, `sleep 1` sleeps.
for name in "$@"; do
  command -v "$name" >/dev/null 2>&1 || { printf '%s - nognu\n' "$name"; continue; }
  opts=$(timeout 3 "$name" --help </dev/null 2>&1 \
        | grep -oE '(^|[ ,])-[A-Za-z]([ ,]|$)' | tr -d ' ,' | sort -u | head -40)
  [ -n "$opts" ] || { printf '%s - nohelp\n' "$name"; continue; }
  for o in $opts; do
    err=$(timeout 3 "$name" "$o" </dev/null 2>&1 >/dev/null | head -2)
    case "$err" in
      *"invalid option"*|*"unrecognized option"*|*"unknown option"*|*"illegal option"*)
        printf '%s %s reject\n' "$name" "$o" ;;
      *)
        printf '%s %s ok\n' "$name" "$o" ;;
    esac
  done
done
