# The `shopt` builtin as a question-and-answer: with a verb (`-s`/`-u`) it
# changes an option, without one it reports where the option stands, and its
# exit status answers "are all of these on?" — which is why a query for an
# option that is off is a *failure* without anything having gone wrong.
#
# The listings here always name their options explicitly. `shopt -o` with no
# names dumps the whole `set -o` table, and this bash is built with an extra
# Cygwin-only entry (`igncr`) that osh has no reason to carry, so the full dump
# is not something the two shells can be asked to agree on.

echo "=== a verb changes the option; no verb reports it"
shopt nocaseglob
shopt -s nocaseglob; shopt nocaseglob
shopt -u nocaseglob; shopt nocaseglob

echo "=== the status says whether every named option is on"
shopt -q nocaseglob; echo "off=$?"
shopt -s nocaseglob
shopt -q nocaseglob; echo "on=$?"
shopt -q nocaseglob nocasematch; echo "one-of-two=$?"
shopt -s nocasematch
shopt -q nocaseglob nocasematch; echo "both=$?"
# `-q` withholds the listing, not the status — and not a complaint either.
shopt -q bogus; echo "unknown=$?"
shopt -u nocaseglob nocasematch

echo "=== -p words the answer as the command that would restore it"
shopt -p nocaseglob
shopt -s nocaseglob; shopt -p nocaseglob
shopt -u nocaseglob

echo "=== an unknown name is stepped over, not fatal"
# Every name gets its answer; the unknown one only costs the status.
shopt bogus nocaseglob; echo "rc=$?"
shopt nocaseglob bogus; echo "rc=$?"
shopt bogus1 bogus2; echo "rc=$?"
# Even when the known names are all on, so the failure is the name and not the
# state it would have reported.
shopt -s nocaseglob; shopt bogus nocaseglob; echo "rc=$?"; shopt -u nocaseglob
# Setting is stricter than asking: there the unknown name does fail the call.
shopt -s bogus nocaseglob; echo "rc=$?"; shopt nocaseglob; shopt -u nocaseglob

echo "=== naming both verbs at once is refused outright"
# Not a usage error — no synopsis line — and no fallback to listing either.
shopt -su nocaseglob; echo "rc=$?"; shopt nocaseglob
shopt -s -u; echo "rc=$?"
shopt -o -s -u xtrace; echo "rc=$?"
# An unknown *flag* is still the earlier complaint of the two.
shopt -zsu; echo "rc=$?"

echo "=== -o asks the same questions of the set -o options"
shopt -o xtrace; echo "rc=$?"
shopt -o -s xtrace; shopt -o xtrace; shopt -o -u xtrace
shopt -o noclobber pipefail; echo "rc=$?"
shopt -o -q pipefail; echo "q=$?"
# There the re-inputtable form is a `set` command, not a `shopt` one.
shopt -o -p pipefail
set -o pipefail; shopt -o -p pipefail; set +o pipefail
shopt -o -p xtrace nounset

echo "=== …but forgives an unknown name, where plain shopt would not"
# bash hands each name to the same code `set -o` uses and does not pass on its
# verdict, so the complaint is made and the call still succeeds. The contrast is
# with `shopt -s bogus` above, which fails.
shopt -o -s bogus; echo "rc=$?"
shopt -o -u bogus; echo "rc=$?"
shopt -o -s xtrace bogus; echo "rc=$?"; shopt -o xtrace; shopt -o -u xtrace
# Asking is strict again, and asking about several names answers for each.
shopt -o bogus xtrace; echo "rc=$?"

echo "=== options the shell keeps for itself are reported, never changed"
shopt login_shell; echo "rc=$?"
shopt -s login_shell; echo "rc=$?"; shopt login_shell
shopt restricted_shell; echo "rc=$?"

echo "=== each answer keeps its place among the complaints"
# The two streams merged, so their order is what is being read: an answer is
# written where it is reached rather than gathered up and printed after the
# names that had none.
shopt nocaseglob bogus nocasematch 2>&1; echo "rc=$?"
shopt -p nocaseglob bogus nocasematch 2>&1; echo "rc=$?"
shopt -o xtrace bogus errexit 2>&1; echo "rc=$?"
# The same listing sent to a file, where each answer is a separate write: the
# second must not start the file over.
shopt nocaseglob nocasematch >f 2>/dev/null; cat f

echo "=== BASHOPTS is the enabled options, and follows every change"
echo "$BASHOPTS"
shopt -s nocaseglob; echo "$BASHOPTS"
shopt -u nocaseglob expand_aliases; echo "$BASHOPTS"
