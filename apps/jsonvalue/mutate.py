"""Mutation test for jsonvalue, the applications' one JSON reader and writer.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails. The rows cover what the crate
promises callers who read untrusted replies (the nesting bound), what its
printers must keep (an escaped key), and the readings it hands back (a
whole number only when exactly whole; any finite number as an f64).

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

NESTING = "nesting_past_the_limit_is_an_error_not_a_stack_overflow"
KEYS = "an_object_key_with_a_quote_or_a_backslash_round_trips"
WHOLE = "only_a_whole_number_is_a_u64"
NUMBERS = "a_number_reads_as_an_f64_whole_or_not"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "nesting is not bounded",
        "    if depth > MAX_DEPTH {\n",
        "    if false {\n",
        [NESTING],
    ),
    (
        "nesting is bounded one short",
        "    if depth > MAX_DEPTH {\n",
        "    if depth >= MAX_DEPTH {\n",
        [NESTING],
    ),
    (
        "an array does not count as a level",
        "fn parse_array(input: &str, depth: usize) -> Result<(JsonValue, &str), String> {\n"
        "    let depth = deeper(depth)?;\n",
        "fn parse_array(input: &str, depth: usize) -> Result<(JsonValue, &str), String> {\n",
        [NESTING],
    ),
    (
        "an object does not count as a level",
        "fn parse_object(input: &str, depth: usize) -> Result<(JsonValue, &str), String> {\n"
        "    let depth = deeper(depth)?;\n",
        "fn parse_object(input: &str, depth: usize) -> Result<(JsonValue, &str), String> {\n",
        [NESTING],
    ),
    (
        "a key is written bare in a document on one line",
        "                    write_string(f, key)?;\n",
        '                    write!(f, "\\"{key}\\"")?;\n',
        [KEYS],
    ),
    (
        "a key is written bare in a laid-out document",
        "                let _infallible = write_string(out, key);\n",
        "                out.push('\"');\n                out.push_str(key);\n                out.push('\"');\n",
        [KEYS],
    ),
    (
        "a fraction reads as a whole number",
        "n.is_finite() && *n >= 0.0 && n.trunc() == *n && *n <= EXACT",
        "n.is_finite() && *n >= 0.0 && *n <= EXACT",
        [WHOLE],
    ),
    (
        "a number that is not finite reads as one",
        "            JsonValue::Number(n) if n.is_finite() => Some(*n),\n",
        "            JsonValue::Number(n) => Some(*n),\n",
        [NUMBERS],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "jsonvalue", timeout=300, only=only))
