"""Mutation test for `offloop`, the worker that runs an application's slow
work off its event-loop thread.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

What must hold: a request supersedes the requests waiting behind the worker;
a result is handed back only for the newest request; the loop is woken when
one is ready; `busy` says whether the newest is still outstanding.

One mutation is deliberately absent: making `wait` loop on a timeout instead
of returning.  Its test would not fail but hang, until the harness's own
timeout ended it -- which proves nothing about which test covers it.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

HANDED = "a_result_is_handed_back_and_the_loop_woken"
NEWEST = "requests_waiting_are_superseded_by_the_newest"
STALE = "a_superseded_result_is_dropped"

MUTATIONS = [
    (
        "every waiting request is run",
        "                    while let Ok(newer) = requests.try_recv() {\n                        next = newer;\n                    }\n",
        "",
        [NEWEST],
    ),
    (
        "the loop is not woken",
        "                    waker.wake_by_ref();\n",
        "",
        [HANDED],
    ),
    (
        "requests are not numbered",
        "        let ticket = self.asked.saturating_add(1);",
        "        let ticket = self.asked;",
        [HANDED],
    ),
    (
        "nothing is ever busy",
        "        self.answered < self.asked",
        "        false",
        [HANDED, STALE],
    ),
    (
        "a superseded result is handed back",
        "        while let Ok(done) = self.results.try_recv() {\n            if done.ticket == self.asked {",
        "        while let Ok(done) = self.results.try_recv() {\n            if true {",
        [STALE],
    ),
    (
        "a result taken is not recorded as answered",
        "            if done.ticket == self.asked {\n                self.answered = done.ticket;\n",
        "            if done.ticket == self.asked {\n",
        [HANDED],
    ),
    (
        "wait hands back a superseded result",
        "                Ok(done) if done.ticket == self.asked => {",
        "                Ok(done) => {",
        [NEWEST],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "offloop", timeout=300, only=only))
