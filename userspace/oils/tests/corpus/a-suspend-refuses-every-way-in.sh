# `suspend` stops the shell — and every way into it that a script can reach is
# a refusal, because neither shell here has job control. What the case pins is
# that the refusals are *not alike*, even though all but one share a status.
#
#   * No job control is a plain failure: the message, status 1, and the script
#     carries on to the next command.
#   * An unknown option is an ordinary usage error instead — the offending
#     letter is named, the synopsis follows on its own line, and the status is
#     2, not 1. Letters are read as a bundle, so `-nf` objects to `n` and never
#     reaches the `f` that would have been accepted.
#   * An *operand* is neither. bash refuses it with `no_args`, which reports
#     `too many arguments` and then unwinds to the outermost read-eval loop
#     rather than returning — so the rest of that line is discarded while the
#     next line still runs. This is the same abort a second operand to `exit`
#     or `return` raises. A lone `-` takes this path too: it is an operand, not
#     an empty option bundle.
#
# `--` ends the options without being one, so it reaches the job-control
# refusal like a bare `suspend`.
#
# Deliberately absent:
#
#   * **`-f`**. It forces past *both* the job-control and the login-shell check
#     and stops the shell for good, so probing it against real bash hangs the
#     probe until the pid is killed — the corpus has no way to recover from
#     that. See known-issues TD-OILS-NO-BIND-BUILTIN, which carries the same
#     warning. osh refuses `-f` like the rest, which is the documented
#     divergence: it has no way to stop and no way to be started again.

echo "=== without job control the shell cannot stop"
suspend; echo "  plain     rc=$?"
suspend --; echo "  --        rc=$?"

echo "=== an unknown option is a usage error at status 2"
suspend -z; echo "  -z        rc=$?"
suspend -nf; echo "  -nf       rc=$?"

echo "=== an operand unwinds the line instead of returning"
suspend extra; echo "  UNREACHED"
echo "  after     rc=$?"
suspend -; echo "  UNREACHED"
echo "  dash      rc=$?"

echo "=== it is a builtin like any other"
type -t suspend
command -v suspend
help -s suspend
