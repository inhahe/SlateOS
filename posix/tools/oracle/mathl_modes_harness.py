"""glibc 2.39's `long double` <math.h> in the three directed rounding modes,
as the oracle for posix/src/mathl.rs's behaviour under fesetround.

    python posix/tools/oracle/mathl_modes_harness.py   # writes posix/src/mathl_modes_oracle.txt

mathl_harness.py's program, its calls run in each mode by _modes.py, which
keeps every call whose answer differs from its answer to nearest (24,885 of
93,186 when this was written).
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import mathl_harness as mh  # noqa: E402
from _modes import write_modes_oracle  # noqa: E402
from _wsl import POSIX_SRC  # noqa: E402

if __name__ == "__main__":
    write_modes_oracle(mh.gen_c(), POSIX_SRC / "mathl_oracle.txt",
                       POSIX_SRC / "mathl_modes_oracle.txt",
                       "the long double calls of mathl_harness.py")
