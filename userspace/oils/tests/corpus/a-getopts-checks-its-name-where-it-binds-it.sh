# `getopts` takes three operands, and checks the middle one last. The optstring
# and the argument list are read before the scan; the *name* is not looked at
# until the scan is over and there is something to store in it.
#
# That ordering is visible from the outside, because a scan does more than
# return a value. It advances `OPTIND`, it sets or clears `OPTARG`, and in
# verbose mode it prints its own complaint about the argument list. All of that
# still happens with an unusable name — so the `not a valid identifier` line
# comes out *after* an `illegal option` line about the same call, and the
# variable the scan could not be stored in is the only thing left unchanged.
#
# The status is 1, not the 0 a found option is worth and not the 2 a misuse is
# worth: the call simply has no answer to give. Which makes an unusable name
# indistinguishable, by status alone, from the end of the options — including
# when there was nothing to scan in the first place, where the complaint is
# still made.
#
# It is the builtin's own complaint rather than the scan's, so `OPTERR=0` — which
# mutes what the scan has to say — does not mute this. And a *missing* name is a
# different thing again: that is a usage error, worth 2, and so is an option
# `getopts` does not have. Both are settled before the scan, so neither reaches
# the name at all.

echo "=== the complaint comes after the scan has had its say"
OPTIND=1; set -- -z
getopts "a" 'bad name'; echo "rc=$? ind=$OPTIND"

echo "=== every way of not being a name"
for n in 'bad name' '1abc' 'a-b' '' 'a[0]' 'a=b' 'a$b' '-o'; do
  OPTIND=1; set -- -a
  getopts "a" "$n"; echo "  [$n] rc=$?"
done

echo "=== a name that is one is bound as usual"
OPTIND=1; set -- -a
getopts "a" '_ok9'; echo "rc=$? ind=$OPTIND ok=$_ok9"

echo "=== the scan's side effects survive the refusal"
OPTIND=1; set -- -z -a
getopts ":a" 'bad name'; echo "rc=$? ind=$OPTIND optarg=[${OPTARG-unset}]"
OPTIND=1; set -- -a
getopts "a:" 'bad name'; echo "rc=$? ind=$OPTIND optarg=[${OPTARG-unset}]"

echo "=== the name outlives the call it could not be stored in"
OPTIND=1; set -- -a; o=keep
getopts "a" 'bad name'; echo "rc=$? o=[$o]"

echo "=== the complaint is made with nothing to scan"
OPTIND=1; set --
getopts "a" 'bad name'; echo "rc=$? ind=$OPTIND"

echo "=== OPTERR mutes the scan, not the builtin"
OPTIND=1; set -- -z
OPTERR=0 getopts "a" 'bad name'; echo "rc=$? ind=$OPTIND"

echo "=== a misuse outranks a bad name"
OPTIND=1; set -- -a
getopts; echo "rc=$?"
getopts "a"; echo "rc=$?"
getopts -q "a" 'bad name'; echo "rc=$?"

echo '=== but a `--` is not a misuse, and still reaches the name'
OPTIND=1; set -- -a
getopts -- "a" 'bad name'; echo "rc=$? ind=$OPTIND"
OPTIND=1; set -- -a
getopts -- "a" ok; echo "rc=$? ind=$OPTIND ok=$ok"
