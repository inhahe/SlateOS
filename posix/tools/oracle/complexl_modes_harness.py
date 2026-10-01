"""glibc 2.39's `long double` <complex.h> in the three directed rounding
modes, as the oracle for posix/src/complexl.rs's behaviour under fesetround.

    python posix/tools/oracle/complexl_modes_harness.py   # writes posix/src/complexl_modes_oracle.txt

complexl_harness.py's program, its calls run in each mode by _modes.py,
which keeps only the calls whose answer changes in kind or in errno, as
complex_modes_harness.py does (4,757 of the 46,793 that differ at all, of
81,402, when this was written); the full table would be 4.9 MB.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import complexl_harness as ch  # noqa: E402
from _modes import changes_kind, write_modes_oracle  # noqa: E402
from _wsl import POSIX_SRC  # noqa: E402

if __name__ == "__main__":
    write_modes_oracle(ch.gen_c(), POSIX_SRC / "complexl_oracle.txt",
                       POSIX_SRC / "complexl_modes_oracle.txt",
                       "the long double calls of complexl_harness.py", keep=changes_kind)
