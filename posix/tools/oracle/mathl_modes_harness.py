"""glibc 2.39's `long double` <math.h> in the three directed rounding modes,
as the oracle for posix/src/mathl.rs's behaviour under fesetround.

    python posix/tools/oracle/mathl_modes_harness.py   # writes posix/src/mathl_modes_oracle.txt

mathl_harness.py's program, its calls run once per mode -- FE_TONEAREST,
then FE_DOWNWARD, FE_UPWARD and FE_TOWARDZERO -- keeping, as
math_modes_harness.py does for the double and float functions, only the
calls that answer otherwise than to nearest: `<mode> <name> <in...> =
<out...> <errno>`, mode `down`, `up` or `zero`. A call the table does not
list answers as mathl_oracle.txt says.

The to-nearest pass must be mathl_oracle.txt line for line, or the two
tables describe different programs: the harness refuses to write, and asks
for mathl_harness.py to be run first.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import mathl_harness as mh  # noqa: E402
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "mathl_modes_oracle.txt"
NEAREST = POSIX_SRC / "mathl_oracle.txt"
MODES = [("near", "FE_TONEAREST"), ("down", "FE_DOWNWARD"), ("up", "FE_UPWARD"),
         ("zero", "FE_TOWARDZERO")]


def gen_c():
    """mathl_harness.py's program, its calls run once in each mode."""
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


def main():
    with workdir() as tmp:
        (Path(tmp) / "mathl_modes.c").write_text(gen_c(), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -fno-builtin -frounding-math -o mathl_modes "
                "mathl_modes.c -lm && ./mathl_modes")
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
    near = NEAREST.read_text(encoding="utf-8").splitlines()
    if by_mode["near"] != near:
        sys.exit(f"{NEAREST.name} is not this program's round-to-nearest answers: "
                 "run mathl_harness.py first")
    out = []
    for t, _ in MODES[1:]:
        assert len(by_mode[t]) == len(near), (t, len(by_mode[t]), len(near))
        for n, directed in zip(near, by_mode[t]):
            assert n.split(" = ")[0] == directed.split(" = ")[0], (n, directed)
            if directed != n:
                out.append(f"{t} {directed}")
    OUT.write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(out)} of {3 * len(near)} directed calls differ from nearest -> {OUT}")


if __name__ == "__main__":
    main()
