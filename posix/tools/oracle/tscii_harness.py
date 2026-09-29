"""glibc 2.39's iconv for TSCII, as the oracle -- cp125x_harness.py's
generator and runner over tscii_cases.py's cases.

    python posix/tools/oracle/tscii_harness.py           # print the table
    python posix/tools/oracle/tscii_harness.py --check   # compare with iconv.rs's
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import cp125x_harness as h  # noqa: E402
from tscii_cases import CASES  # noqa: E402

h.CASES = CASES


def main() -> None:
    lines = h.glibc_lines("tscii_oracle")
    h.emit_table(h.table("GLIBC_TSCII", "tscii_harness.py", lines), "iconv.rs")


if __name__ == "__main__":
    main()
