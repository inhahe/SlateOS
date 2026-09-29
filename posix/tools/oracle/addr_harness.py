"""glibc 2.39's address conversions as the oracle for posix/src/inet.rs's
`GLIBC` table: `inet_aton`, `inet_addr`, `inet_pton`, `inet_network`,
`inet_ntop`, `inet_ntoa`, `inet_makeaddr`/`inet_lnaof`/`inet_netof`,
`ether_aton`, `ether_ntoa` and `ether_line`.

    python posix/tools/oracle/addr_harness.py           # print the table
    python posix/tools/oracle/addr_harness.py --check   # compare with inet.rs's

The cases are addr_cases.py's. addr_oracle.c reads them on stdin, one
`<kind> <hex>` line each, and prints glibc's answers, one line each; the
table pairs the two.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import addr_cases  # noqa: E402
from _wsl import emit_table, run, workdir, wsl_path  # noqa: E402

HERE = Path(__file__).resolve().parent


def main():
    ins = list(addr_cases.lines())
    with workdir() as tmp:
        (Path(tmp) / "in.txt").write_text("\n".join(ins) + "\n", encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -Wall -o addr_oracle "
                f"{wsl_path(HERE)}/addr_oracle.c && ./addr_oracle < in.txt")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout}\n{r.stderr}")
    outs = r.stdout.splitlines()
    assert len(ins) == len(outs), (len(ins), len(outs))
    lines = [
        "    /// glibc 2.39's answers (`posix/tools/oracle/addr_harness.py`): the",
        "    /// input (`s` text, `a6`/`a4` address bytes, `mk` net host, `e`",
        "    /// Ethernet bytes, `el` an ethers line -- all hex) and the line",
        "    /// `addr_oracle.c` printed.",
        "    const GLIBC: &[(&str, &str)] = &[",
    ]
    for i, o in zip(ins, outs):
        assert '"' not in o and "\\" not in o, o
        lines.append(f'        ("{i}", "{o}"),')
    lines.append("    ];")
    emit_table("\n".join(lines) + "\n", "inet.rs")


if __name__ == "__main__":
    main()
