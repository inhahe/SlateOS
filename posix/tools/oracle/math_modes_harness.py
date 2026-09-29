"""glibc 2.39's libm in the three directed rounding modes, as the oracle for
posix/src/math.rs's behaviour under fesetround.

    python posix/tools/oracle/math_modes_harness.py   # writes posix/src/math_modes_oracle.txt

math_harness.py's program, its calls run in each mode by _modes.py, which
keeps every call whose answer differs from its answer to nearest (12,103 of
70,635 when this was written): the exact functions' directed answers are
compared bit for bit, so they must all be there.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import math_harness as mh  # noqa: E402
from _modes import write_modes_oracle  # noqa: E402
from _wsl import POSIX_SRC  # noqa: E402

if __name__ == "__main__":
    write_modes_oracle(mh.gen_c(), POSIX_SRC / "math_oracle.txt",
                       POSIX_SRC / "math_modes_oracle.txt",
                       "the double and float calls of math_harness.py")
