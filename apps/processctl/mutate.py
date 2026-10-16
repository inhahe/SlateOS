"""Mutation test for what a task manager does to a process it picked.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

ROUTE = "each_ending_goes_the_way_the_program_understands"
WIDE = "a_pid_that_does_not_fit_reaches_nothing"
GONE = "a_process_already_gone_is_said_to_be"
SAID = "the_line_says_what_was_done"
NAMED = "the_ways_are_named_gentlest_first"

MUTATIONS = [
    (
        "close is sent as an end",
        "            Self::Close => Request::Close,\n",
        "            Self::Close => Request::Terminate,\n",
        [ROUTE, NAMED],
    ),
    (
        "end is sent as a force",
        "            Self::End => Request::Terminate,\n",
        "            Self::End => Request::Force,\n",
        [ROUTE, NAMED],
    ),
    (
        "force is sent as an end",
        "            Self::Force => Request::Force,\n",
        "            Self::Force => Request::Terminate,\n",
        [ROUTE, NAMED],
    ),
    (
        "an ending goes by signal alone",
        "    procstop::deliver(sys, pid, ending.request(), Via::Either)\n",
        "    procstop::deliver(sys, pid, ending.request(), Via::Signal)\n",
        [ROUTE],
    ),
    (
        "a pid too big wraps to a negative",
        "    i32::try_from(pid).ok()\n",
        "    Some(pid as i32)\n",
        [WIDE],
    ),
    (
        "a signal is sent nowhere",
        "    sys.kill(widen(pid).ok_or(EINVAL)?, sig)\n",
        "    widen(pid).ok_or(EINVAL).map(drop)\n",
        [WIDE],
    ),
    (
        "a program asked to close is said to have been asked to end",
        "        Ok(Route::Message) if ending == Ending::Close => {\n",
        "        Ok(Route::Message) if false => {\n",
        [SAID],
    ),
    (
        "a close that cannot be asked names the wrong way out",
        "            Ending::End.label()\n",
        "            Ending::Force.label()\n",
        [SAID],
    ),
    (
        "not permitted reads as already gone",
        '        EPERM => format!("Not permitted to {verb} {name} (PID {pid})"),\n',
        '        EPERM => format!("{name} (PID {pid}) is already gone -- nothing to {verb}"),\n',
        [SAID],
    ),
    (
        "already gone reads as an error number",
        '        ESRCH => format!("{name} (PID {pid}) is already gone -- nothing to {verb}"),\n',
        "",
        [GONE, SAID],
    ),
    (
        "a signal accepted says nothing of what was sent",
        '        Ok(()) => format!("Sent {verb} to {name} (PID {pid})"),\n',
        '        Ok(()) => format!("{name} (PID {pid})"),\n',
        ["a_signals_line_says_what_was_done"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "processctl", timeout=600, only=only))
