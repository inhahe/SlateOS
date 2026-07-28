# getopts: bundling, option arguments, OPTIND advancement, the `--` terminator,
# and both error modes (verbose vs the silent mode selected by a leading colon).
set -- -ab -c val -- -d rest
while getopts 'abc:' opt; do
  case $opt in
    a|b) echo "flag=$opt" ;;
    c)   echo "carg=$OPTARG" ;;
    *)   echo "other=$opt" ;;
  esac
done
echo "optind=$OPTIND rest=${*:$OPTIND}"

# Silent mode: an unknown option yields `?` with OPTARG set, a missing argument
# yields `:`, and nothing is printed to stderr.
OPTIND=1
set -- -z -c
while getopts ':ac:' opt; do
  echo "opt=$opt optarg=[$OPTARG]"
done

# Verbose mode writes its own diagnostic (message text is bash's, so this also
# pins the wording).
OPTIND=1
set -- -z
getopts 'a' opt 2>&1
echo "verbose-opt=$opt status=$?"

# getopts returns non-zero once the operands are exhausted.
OPTIND=1
set -- x y
getopts 'a' opt
echo "no-options status=$? optind=$OPTIND"
