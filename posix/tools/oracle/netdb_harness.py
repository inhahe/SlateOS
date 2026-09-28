"""Run netdb_oracle.c under WSL against this module's database files: the
Rust test data (the files, the commands, glibc's output) posix/src/netdb.rs's
tests carry pasted.

    python posix/tools/oracle/netdb_harness.py           # print the table
    python posix/tools/oracle/netdb_harness.py --check   # compare with netdb.rs's

The sandbox is `unshare -r -m`: a user and mount namespace, no root needed,
with this module's files bind-mounted over /etc.
"""
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import emit_table, workdir, wsl_path  # noqa: E402

HERE = Path(__file__).resolve().parent

FILES = {
    "services": (
        b"# comment line\n"
        b"http\t\t80/tcp\t\twww www-http\t# the web\n"
        b"http\t\t80/udp\n"
        b"  indented\t81/tcp\n"
        b"ftp 21/tcp\n"
        b"ftp 21/udp fsp\n"
        b"noproto 99\n"
        b"octal 010/tcp\n"
        b"hex 0x1f/tcp\n"
        b"big 70000/tcp\n"
        b"slashes 85//tcp\n"
        b"bad 8x/tcp\n"
        b"empty /tcp\n"
        b"dup 7/tcp first\n"
        b"dup 7/tcp second\n"
        b"Case 55/tcp\n"
        b"crlf 56/tcp alias\r\n"
        b"nul 57/tcp\x00hidden\n"
        b"tabs\t58/tcp\tt1\tt2\n"
        b"hash#x 59/tcp\n"
        b"trailing 60/tcp   \n"
        b"spaceport  61 /tcp\n"
        b"sp2 62/ tcp\n"
        b"\n"
        b"   \n"
        b"last 63/tcp"
    ),
    "protocols": (
        b"ip 0 IP\n"
        b"tcp 6 TCP\n"
        b"udp\t17\tUDP\t# user datagram\n"
        b"hex 0x10 HEX\n"
        b"bad 12x BAD\n"
        b"noalias 20\n"
        b"dup 6 DUP\n"
        b"Case 30\n"
        b"endnum 31"
    ),
    "networks": (
        b"loopback 127\n"
        b"link-local 169.254.0.0 ll\n"
        b"ten 10.0 tennet\n"
        b"full 192.168.1.0\n"
        b"Upper 172.16\n"
        b"hex 0x0a.1\n"
        b"bad x.y\n"
        b"toomany 1.2.3.4.5\n"
        b"noaddr\n"
    ),
    "ethers": (
        b"00:11:22:33:44:55 host1\n"
        b"0:1:2:3:4:5 Host2 # comment\n"
        b"aa:bb:cc:dd:ee:ff\thost3\n"
        b"1:2:3:4:5 short\n"
        b"100:1:2:3:4:5 toobig\n"
        b"0x1:2:3:4:5:6 hexpfx\n"
        b" 1:2:3:4:5:7 indented\n"
        b"1:2:3:4:5:8\n"
        b"1:2:3:4:5:9 twice\n"
        b"1:2:3:4:5:9 again\n"
    ),
}

CMDS = """\
serv http tcp
serv http -
serv www tcp
serv www-http udp
serv http udp
serv indented -
serv ftp udp
serv fsp -
serv fsp tcp
serv noproto -
serv noproto tcp
serv octal -
port 8 -
serv hex -
port 31 tcp
serv big -
port 4464 tcp
serv slashes -
serv bad -
serv empty -
serv dup -
port 7 tcp
serv case -
serv Case -
serv crlf -
serv alias -
serv nul -
serv hidden -
serv tabs -
serv t2 -
serv hash -
serv hash#x -
serv trailing tcp
serv spaceport -
serv sp2 -
serv last -
port 80 -
port 80 udp
port 81 tcp
port 999 -
port 63 tcp
rawport 0x7fffffff -
servsmall http tcp 10
servsmall http tcp 60
servsmall http tcp 200
servent
mix ftp
proto tcp
proto TCP
proto udp
proto UDP
protonum 17
protonum 6
proto hex
protonum 16
protonum 0
proto bad
proto noalias
proto case
proto Case
proto endnum
protonum 99
protoent
net loopback
net LOOPBACK
net ll
net LL
net ten
net full
net upper
net hex
net bad
net toomany
net noaddr
net nothere
netaddr 7f000000 2
netaddr 7f000000 0
netaddr 7f000000 10
netaddr a000000 2
netaddr ffffffff 2
netaddr 0 2
netent
ethh host1
ethh HOST1
ethh host2
ethh host3
ethh short
ethh toobig
ethh hexpfx
ethh indented
ethh twice
ethh nothere
ethn 0:11:22:33:44:55
ethn aa:bb:cc:dd:ee:ff
ethn 1:2:3:4:5:6
ethn 1:2:3:4:5:7
ethn 1:2:3:4:5:8
ethn 1:2:3:4:5:9
ethn 9:9:9:9:9:9
"""


def main():
    with workdir() as tmp:
        emit(run(Path(tmp)))


def run(work: Path) -> str:
    for name, text in FILES.items():
        (work / name).write_bytes(text)
    (work / "cmds").write_text(CMDS, newline="\n")
    w = wsl_path(work)
    names = " ".join(f"{w}/{n}" for n in FILES)
    script = (
        f"cd {w} && gcc -O0 -Wall -o netdb_oracle {wsl_path(HERE)}/netdb_oracle.c && "
        f"unshare -r -m sh -c 'd=/tmp/etcx.$$; mkdir -p $d && mount -t tmpfs none $d && "
        f"(cp -a /etc/. $d/ 2>/dev/null; true) && cp {names} $d/ && mount --bind $d /etc && "
        f"./netdb_oracle < {w}/cmds'"
    )
    out = subprocess.run(["wsl", "-d", "Ubuntu", "--exec", "bash", "-c", script],
                         capture_output=True)
    if out.returncode != 0:
        print(out.stderr.decode(errors="replace"))
        raise SystemExit(out.returncode)
    return out.stdout.decode()


def rust_bytes(b: bytes) -> str:
    out = []
    for c in b:
        if c == 0x22:
            out.append('\\"')
        elif c == 0x5C:
            out.append("\\\\")
        elif c == 0x0A:
            out.append("\\n")
        elif c == 0x09:
            out.append("\\t")
        elif c == 0x0D:
            out.append("\\r")
        elif 0x20 <= c < 0x7F:
            out.append(chr(c))
        else:
            out.append("\\x%02x" % c)
    return 'b"' + "".join(out) + '"'


def emit(output: str):
    lines = []
    lines.append("    // Generated by posix/tools/oracle/netdb_harness.py: the database files, the")
    lines.append("    // commands, and what glibc 2.39 printed for them (netdb_oracle.c).")
    for name, text in FILES.items():
        lines.append(f"    const {name.upper()}_FILE: &[u8] = {rust_bytes(text)};")
    lines.append("    const COMMANDS: &str = \"\\")
    for c in CMDS.splitlines():
        lines.append(c + "\\n\\")
    lines.append("\";")
    lines.append("    const GLIBC_OUTPUT: &str = \"\\")
    for l in output.splitlines():
        assert '"' not in l and "\\" not in l, l
        lines.append(l + "\\n\\")
    lines.append("\";")
    emit_table("\n".join(lines) + "\n", "netdb.rs")


if __name__ == "__main__":
    main()
