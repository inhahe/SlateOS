"""glibc 2.39's libm in the three directed rounding modes, as the oracle for
posix/src/math.rs's behaviour under fesetround.

    python posix/tools/oracle/math_modes_harness.py   # writes posix/src/math_modes_oracle.txt

The same program as math_harness.py, run once per mode -- FE_TONEAREST,
then FE_DOWNWARD, FE_UPWARD and FE_TOWARDZERO. Most calls answer the same in
every mode (58,532 of the 70,635 directed calls when this was written), so
the table keeps only the others: a line `<mode> <name> <inputs> = <outputs>
<errno>`, mode `down`, `up` or `zero`, for each call whose outputs or errno
differ from the same call's to nearest. A call the table does not list
answers as math_oracle.txt says.

The to-nearest pass must be math_oracle.txt line for line, or the two tables
describe different programs: the harness refuses to write, and asks for
math_harness.py to be run first.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import math_cases as mc  # noqa: E402
import math_harness as mh  # noqa: E402
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "math_modes_oracle.txt"
NEAREST = POSIX_SRC / "math_oracle.txt"
MODES = [("near", "FE_TONEAREST"), ("down", "FE_DOWNWARD"), ("up", "FE_UPWARD"),
         ("zero", "FE_TOWARDZERO")]


def gen_c():
    """math_harness.py's program, its calls run once in each mode."""
    c = mh.gen_c()
    assert c.count("int main(void) {") == 1
    c = c.replace("int main(void) {", "static void run_all(void) {")
    tail = "  return 0;\n}\n"
    assert c.endswith(tail)
    c = c[: -len(tail)] + "}\n"
    c = c.replace("#include <math.h>", "#include <math.h>\n#include <fenv.h>", 1)
    c += "int main(void) {\n"
    for tag, mode in MODES:
        c += ('  printf("@' + tag + '\\n"); fflush(stdout); fesetround(' + mode
              + '); run_all(); fflush(stdout); fesetround(FE_TONEAREST);\n')
    c += "  return 0;\n}\n"
    return c


def calls(path):
    """The call lines of an oracle table, comments and blanks dropped."""
    return [line for line in path.read_text(encoding="utf-8").splitlines()
            if line and not line.startswith("#")]


def main():
    with workdir() as tmp:
        (Path(tmp) / "math_modes.c").write_text(gen_c(), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -fno-builtin -frounding-math -w -o math_modes "
                "math_modes.c -lm && ./math_modes")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-4000:]}")
    by_mode, tag = {t: [] for t, _ in MODES}, None
    for line in r.stdout.splitlines():
        if not line.strip():
            continue
        if line.startswith("@"):
            tag = line[1:]
            continue
        by_mode[tag].append(line)
    n = sum(len(rows) for _, _, rows in mc.cases())
    for t, rows in by_mode.items():
        assert len(rows) == n, (t, len(rows), n)
    if by_mode["near"] != calls(NEAREST):
        sys.exit(f"{NEAREST.name} is not this program's round-to-nearest answers: "
                 "run math_harness.py first")
    out = []
    for t, _ in MODES[1:]:
        for near, directed in zip(by_mode["near"], by_mode[t]):
            assert near.split(" = ")[0] == directed.split(" = ")[0], (near, directed)
            if directed != near:
                out.append(f"{t} {directed}")
    header = ("# glibc 2.39 libm under WSL in the directed rounding modes "
              "(posix/tools/oracle/math_modes_harness.py):\n"
              "# <mode> <function> <inputs> = <outputs> <errno>, for each call that answers\n"
              "# otherwise than math_oracle.txt says it does to nearest; floats as bit\n"
              "# patterns in hex.\n")
    OUT.write_text(header + "\n".join(out) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(out)} of {3 * n} directed calls differ from nearest -> {OUT}")


if __name__ == "__main__":
    main()
