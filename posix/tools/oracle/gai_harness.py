"""Run gai_oracle.c under WSL in a sandbox (one IPv4 address, 10.0.2.15/24
with a gateway, IPv6 off, DNS refused), once per gai.conf: the Rust test data
posix/src/gai.rs's tests carry pasted.

    python posix/tools/oracle/gai_harness.py           # print the table
    python posix/tools/oracle/gai_harness.py --check   # compare with gai.rs's

The sandbox is `unshare -r -n -m`: a user, network and mount namespace, no
root needed, with this module's files bind-mounted over /etc.
"""
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import emit_table, workdir, wsl_path  # noqa: E402

HERE = Path(__file__).resolve().parent

HOSTS = (
    b"127.0.0.1 localhost\n"
    b"::1 localhost ip6-localhost\n"
    b"10.0.2.99 near.example near\n"
    b"8.8.8.8 far.example far\n"
    b"10.0.2.99 mixed.example\n"
    b"8.8.8.8 mixed.example\n"
    b"::1 mixed.example\n"
    b"127.0.0.5 mixed.example\n"
    b"2001:db8::1 mixed.example\n"
    b"fe80::1 linklocal.example\n"
    b"169.254.1.1 ll4.example\n"
    b"8.8.4.4 mixedll.example\n"
    b"169.254.1.1 mixedll.example\n"
    b"10.0.2.50 mixedll.example\n"
)
HOSTCONF = b"multi on\n"
SERVICES = (
    b"http 80/tcp www\n"
    b"http 80/udp\n"
    b"ssh 22/tcp\n"
    b"domain 53/tcp\n"
    b"domain 53/udp\n"
    b"dccpsvc 1234/dccp\n"
    b"sctpsvc 5000/sctp\n"
    b"rawsvc 7/raw\n"
    b"udponly 999/udp\n"
)
GAICONFS = {
    "default": b"",
    "custom": (
        b"# a custom table\n"
        b"label ::1/128 0\n"
        b"label ::/0 1\n"
        b"precedence ::ffff:0:0/96 100\n"
        b"precedence ::1/128 50\n"
        b"scopev4 ::ffff:169.254.0.0/112 2\n"
        b"scopev4 ::ffff:10.0.0.0/104 5\n"
        b"scopev4 8.8.0.0/16 7\n"
        b"bogus line here\n"
        b"precedence ::/0 x\n"
    ),
}

CMDS = """\
gai localhost NULL 0 0 0 0
gai localhost http 0 0 0 0
gai localhost http nohints
gai localhost 80 0 0 1 0
gai localhost 80 0 0 2 0
gai localhost 80 0 0 3 0
gai localhost NULL 0 0 3 0
gai localhost NULL 0 0 3 99
gai localhost 80 0 0 0 6
gai localhost 80 0 0 0 17
gai localhost 80 0 0 0 132
gai localhost 80 0 0 5 0
gai localhost 80 0 0 6 0
gai localhost 80 0 0 1 17
gai localhost 80 0 0 7 0
gai localhost 80 0 0 0 99
gai localhost NULL 0 0 0 99
gai localhost ssh 0 0 0 0
gai localhost domain 0 0 0 0
gai localhost udponly 0 0 1 0
gai localhost dccpsvc 0 0 0 0
gai localhost sctpsvc 0 0 0 0
gai localhost rawsvc 0 0 0 0
gai localhost nosuch 0 0 0 0
gai localhost nosuch 0x400 0 0 0
gai localhost +80 0 0 1 0
gai localhost -1 0 0 1 0
gai localhost 70000 0 0 1 0
gai localhost 4294967296 0 0 1 0
gai localhost 2147483648 0 0 1 0
gai localhost 99999999999999999999 0 0 1 0
gai localhost EMPTY 0 0 1 0
gai * 80 0 0 1 0
gai * * 0 0 1 0
gai NULL NULL 0 0 0 0
gai NULL 80 1 0 1 0
gai NULL 80 0 10 1 0
gai NULL 80 1 2 1 0
gai NULL 80 2 0 1 0
gai localhost 80 0x8000 0 1 0
gai localhost 80 0 99 1 0
gai 1.2.3.4 80 0 0 1 0
gai 1.2.3.4 80 0 10 1 0
gai 1.2.3.4 80 8 10 1 0
gai 127.1 80 0 0 1 0
gai 0x7f000001 80 0 0 1 0
gai ::1 80 0 0 1 0
gai ::1 80 0 2 1 0
gai ::ffff:1.2.3.4 80 0 2 1 0
gai fe80::1%1 80 0 0 1 0
gai fe80::1%lo 80 0 0 1 0
gai fe80::1%bogus 80 0 0 1 0
gai 2001:db8::1%5 80 0 0 1 0
gai 1.2.3.4 80 2 0 1 0
gai ::1 80 2 0 1 0
gai near 80 2 0 1 0
gai near 80 2 2 1 0
gai near.example 80 0 0 1 0
gai near 80 4 0 1 0
gai nosuchhost 80 0 0 1 0
gai nosuchhost 80 0 2 1 0
gai nosuchhost 80 0 10 1 0
gai mixed.example 80 0 0 1 0
gai mixed.example 80 2 0 1 0
gai mixed.example 80 0x20 0 1 0
gai mixed.example 80 0 10 1 0
gai mixed.example 80 8 10 1 0
gai mixed.example 80 0 2 1 0
gai mixedll.example 80 0 0 1 0
gai ll4.example 80 0 0 1 0
gai linklocal.example 80 0 0 1 0
gai mixed.example NULL nohints
gai localhost 80 0x40 0 1 0
gai localhost 80 0x300 0 1 0
gni 2 127.0.0.1 80 0 100 100 0
gni 2 127.0.0.1 80 0 100 100 1
gni 2 127.0.0.1 80 0 100 100 2
gni 2 10.0.2.99 53 0 100 100 16
gni 2 1.1.1.1 80 0 100 100 0
gni 2 1.1.1.1 80 0 100 100 1
gni 2 1.1.1.1 80 0 100 100 8
gni 2 127.0.0.1 80 0 5 100 0
gni 2 127.0.0.1 80 0 100 3 0
gni 2 127.0.0.1 12345 0 100 100 0
gni 2 127.0.0.1 12345 0 100 3 0
gni 10 ::1 80 0 100 100 0
gni 10 ::ffff:127.0.0.1 80 0 100 100 0
gni 10 fe80::1 80 1 100 100 1
gni 10 fe80::1 80 7 100 100 1
gni 10 2001:db8::1 80 5 100 100 1
gni 10 fe80::1 80 1 12 100 1
gni 1 /tmp/sock 0 0 100 100 1
gni 1 /tmp/sock 0 0 100 3 1
gni 99 x 0 0 100 100 0
gni 2 1.2.3.4 80 0 100 100 0x400
gni 2 1.2.3.4 80 0 0 0 8
gni 2 127.0.0.1 80 0 0 100 0
gni 2 127.0.0.1 80 0 100 0 0
err
"""


def run(work: Path, conf: str):
    d = work / conf
    d.mkdir(exist_ok=True)
    files = {
        "hosts": HOSTS,
        "host.conf": HOSTCONF,
        "services": SERVICES,
        "gai.conf": GAICONFS[conf],
        "resolv.conf": b"nameserver 10.0.2.53\noptions timeout:1 attempts:1\n",
        "nsswitch.conf": b"hosts: files dns\nservices: files\n",
    }
    for n, t in files.items():
        (d / n).write_bytes(t)
    (work / "cmds").write_text(CMDS, newline="\n")
    w = wsl_path(d)
    rm = " ".join(f"$e/{n}" for n in files)
    cp = " ".join(f"{w}/{n}" for n in files)
    script = (
        f"cd {wsl_path(work)} && gcc -O0 -Wall -o gai_oracle {wsl_path(HERE)}/gai_oracle.c && "
        "unshare -r -n -m sh -c '"
        "ip link set lo up && ip addr add 10.0.2.15/24 dev lo && "
        "ip route add default via 10.0.2.2 dev lo && "
        "echo 1 > /proc/sys/net/ipv6/conf/all/disable_ipv6 && "
        "echo 1 > /proc/sys/net/ipv6/conf/lo/disable_ipv6 && "
        "e=/tmp/etcg.$$; mkdir -p $e && mount -t tmpfs none $e && "
        f"(cp -a /etc/. $e/ 2>/dev/null; true) && rm -f {rm} && cp {cp} $e/ && "
        "mount --bind $e /etc && "
        f"./gai_oracle < {wsl_path(work)}/cmds'"
    )
    out = subprocess.run(["wsl", "-d", "Ubuntu", "--exec", "bash", "-c", script], capture_output=True)
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
        elif 0x20 <= c < 0x7F:
            out.append(chr(c))
        else:
            out.append("\\x%02x" % c)
    return 'b"' + "".join(out) + '"'


def rust_str_block(name: str, text: str) -> list:
    lines = [f"    const {name}: &str = \"\\"]
    for l in text.splitlines():
        assert '"' not in l and "\\" not in l, l
        lines.append(l + "\\n\\")
    lines.append("\";")
    return lines


def main():
    lines = ["    // Generated by posix/tools/oracle/gai_harness.py: the files, the commands, and what",
             "    // glibc 2.39 printed for them (gai_oracle.c), on one IPv4 address, 10.0.2.15/24."]
    lines.append(f"    const HOSTS_FILE: &[u8] = {rust_bytes(HOSTS)};")
    lines.append(f"    const HOSTCONF_FILE: &[u8] = {rust_bytes(HOSTCONF)};")
    lines.append(f"    const SERVICES_FILE: &[u8] = {rust_bytes(SERVICES)};")
    for n, t in GAICONFS.items():
        lines.append(f"    const GAICONF_{n.upper()}: &[u8] = {rust_bytes(t)};")
    lines += rust_str_block("COMMANDS", CMDS)
    with workdir() as tmp:
        for n in GAICONFS:
            lines += rust_str_block(f"GLIBC_{n.upper()}", run(Path(tmp), n))
    emit_table("\n".join(lines) + "\n", "gai.rs")


if __name__ == "__main__":
    main()
