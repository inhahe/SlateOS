"""glibc 2.39's double and float <complex.h> in the three directed rounding
modes, as the oracle for posix/src/complex.rs's behaviour under fesetround.

    python posix/tools/oracle/complex_modes_harness.py   # writes posix/src/complex_modes_oracle.txt

complex_harness.py's program, its calls run in each mode by _modes.py,
which keeps only the calls whose answer changes in kind -- an infinity, a
NaN or a zero where the answer to nearest has none, or of the other sign --
or in errno (6,475 of the 70,831 that differ at all, of 134,874, when this
was written). Every other directed answer is within a few ulps of the one
to nearest, and complex.rs's test holds its own to that; the full table
would be 4.7 MB.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import complex_harness as ch  # noqa: E402
from _modes import changes_kind, write_modes_oracle  # noqa: E402
from _wsl import POSIX_SRC  # noqa: E402

if __name__ == "__main__":
    write_modes_oracle(ch.gen_c(), POSIX_SRC / "complex_oracle.txt",
                       POSIX_SRC / "complex_modes_oracle.txt",
                       "the double and float calls of complex_harness.py", keep=changes_kind)
