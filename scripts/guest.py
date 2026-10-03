#!/usr/bin/env python3
"""A SlateOS guest that stays up, and a way into it from the host.

    python scripts/guest.py start [--port 4555] [--timeout 900]
    python scripts/guest.py try ./build/my-tool [args...]      # put, then run
    python scripts/guest.py put LOCAL GUEST-PATH [--mode 755]
    python scripts/guest.py get GUEST-PATH LOCAL
    python scripts/guest.py run [--seconds 60] [--grants file] GUEST-PATH [args...]
    python scripts/guest.py sh 'ls /bin'
    python scripts/guest.py ping
    python scripts/guest.py stop

C-Q11 idea 1 (design-decisions 1534): instead of a two-and-a-half-hour boot to
try a changed program, keep one guest running and copy the program in. The
guest is QEMU booting what the last boot test built -- its ESP (kernel and
bootloader) and `rootfs.ext4` -- with one extra device, a virtio-serial port
named `org.slateos.agent.0`, which QEMU connects to a TCP socket on
127.0.0.1. Inside, the kernel's guest agent (`kernel/src/guestagent.rs`)
answers on that port: put a file, get a file, run a program with the
capabilities named and hand back its output and exit code, or run a kernel
shell command.

It is for trying a change, not for publishing one. The guest's state carries
over from one request to the next -- files put stay, a program that broke
something left it broken -- and the boot test stays the gate for `main`.

`start` copies the ESP into `build/guest/` first, so a boot test running in the
same worktree never shares a writable directory with the guest, and attaches
both disks with `snapshot=on`, so nothing the guest writes reaches the host's
files. It records the QEMU process's PID in `build/guest/qemu.pid`, and `stop`
ends that process and no other.

The protocol, which the agent's module doc defines: a request is a line of
words -- a verb and decimal numbers (a `put`'s mode is octal) -- then the byte
fields the numbers measure; a reply is the same shape, its last number the
length of the one field that follows (`pong` has none).

Exit status: a `run` or `try` exits with the program's own code (124 when it
ran out of time, as `timeout` does); `sh` with the shell command's; otherwise 0,
or 1 when the agent refused the request, 2 for a usage or connection error.
"""

from __future__ import annotations

import argparse
import os
import shutil
import socket
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
GUEST_DIR = os.path.join(ROOT, "build", "guest")
PIDFILE = os.path.join(GUEST_DIR, "qemu.pid")
PORT_FILE = os.path.join(GUEST_DIR, "port")
DEFAULT_PORT = 4555
AGENT_PORT_NAME = "org.slateos.agent.0"
PROTOCOL_VERSION = 1

# Added to the kernel command line in the guest's copy of the ESP: no boot
# self-tests and no boot benchmarks, so the agent answers once the system is up
# rather than after the whole suite, and a self-test that halts cannot stop
# the guest first (kernel/src/selftest.rs, `skip`). boot-test.sh refuses
# both, so neither can reach a boot test.
GUEST_CMDLINE_WORDS = ("selftest.skip=1", "bench.skip=1")

QEMU_CANDIDATES = (
    "C:/Program Files/qemu/qemu-system-x86_64.exe",
    "/usr/bin/qemu-system-x86_64",
)
OVMF_CANDIDATES = (
    "C:/Program Files/qemu/share/edk2-x86_64-code.fd",
    "/usr/share/OVMF/OVMF_CODE.fd",
    "/usr/share/edk2/ovmf/OVMF_CODE.fd",
)


class AgentError(Exception):
    """The agent answered `err`: the message is its own."""


class Usage(Exception):
    """Something this tool cannot do as asked: no guest, no build, a bad argument."""


# ---------------------------------------------------------------------------
# The protocol
# ---------------------------------------------------------------------------

def encode(verb: str, numbers: list[int | str], fields: list[bytes]) -> bytes:
    """One request: the line, then its fields."""
    words = [verb] + [str(n) for n in numbers]
    return " ".join(words).encode("ascii") + b"\n" + b"".join(fields)


def read_line(sock: socket.socket, limit: int = 1024) -> bytes:
    """Bytes up to a newline, the newline dropped."""
    line = bytearray()
    while True:
        b = sock.recv(1)
        if not b:
            raise ConnectionError("the guest closed the connection mid-reply")
        if b == b"\n":
            return bytes(line)
        line += b
        if len(line) > limit:
            raise ConnectionError("a reply line longer than %d bytes" % limit)


def read_exact(sock: socket.socket, n: int) -> bytes:
    """Exactly `n` bytes."""
    out = bytearray()
    while len(out) < n:
        chunk = sock.recv(min(1 << 20, n - len(out)))
        if not chunk:
            raise ConnectionError("the guest closed the connection mid-reply")
        out += chunk
    return bytes(out)


def parse_reply_line(line: bytes) -> tuple[str, list[int], int]:
    """A reply line's verb, its numbers, and the length of the field after it."""
    words = line.decode("ascii").split(" ")
    verb, numbers = words[0], [int(w) for w in words[1:]]
    if verb == "pong":
        return verb, numbers, 0
    if not numbers:
        raise ConnectionError("a reply with no length: %r" % line)
    if numbers[-1] < 0:
        raise ConnectionError("a reply with a negative length: %r" % line)
    return verb, numbers, numbers[-1]


def exchange(sock: socket.socket, request: bytes) -> tuple[str, list[int], bytes]:
    """Send one request; its reply's verb, numbers and field. `err` raises."""
    sock.sendall(request)
    verb, numbers, length = parse_reply_line(read_line(sock))
    field = read_exact(sock, length)
    if verb == "err":
        raise AgentError(field.decode("utf-8", "replace"))
    return verb, numbers, field


def connect(port: int, timeout: float = 30.0, reply_timeout: float = 600.0) -> socket.socket:
    """A connection to the agent's port. `reply_timeout` bounds every wait
    for the agent's answer after that, so a reply lost on the way (the guest
    gone, the port stuck) ends in an error rather than a hang."""
    sock = socket.create_connection(("127.0.0.1", port), timeout=timeout)
    sock.settimeout(reply_timeout)
    return sock


# ---------------------------------------------------------------------------
# Requests
# ---------------------------------------------------------------------------

def ping(port: int, wait: float = 5.0) -> bool:
    """Whether the agent answers within `wait` seconds."""
    try:
        sock = connect(port, timeout=wait)
    except OSError:
        return False
    try:
        sock.settimeout(wait)
        verb, numbers, _ = exchange(sock, encode("ping", [], []))
        return verb == "pong" and numbers == [PROTOCOL_VERSION]
    except (OSError, ConnectionError, AgentError, ValueError):
        return False
    finally:
        sock.close()


def put(port: int, local: str, guest_path: str, mode: str) -> None:
    with open(local, "rb") as f:
        data = f.read()
    path = guest_path.encode("utf-8")
    with connect(port) as sock:
        exchange(sock, encode("put", [mode, len(path), len(data)], [path, data]))


def get(port: int, guest_path: str) -> bytes:
    path = guest_path.encode("utf-8")
    with connect(port) as sock:
        _, _, field = exchange(sock, encode("get", [len(path)], [path]))
    return field


def run(port: int, seconds: int, grants: str, argv: list[str]) -> tuple[int, bytes]:
    """The program's exit code (124 when it ran out of time) and its output."""
    args = [a.encode("utf-8") for a in argv]
    g = grants.encode("ascii")
    # The answer comes when the program ends: give it its own limit and more.
    with connect(port, reply_timeout=seconds + 120.0) as sock:
        verb, numbers, output = exchange(
            sock,
            encode("run", [seconds, len(g), len(args)] + [len(a) for a in args], [g] + args),
        )
    if verb == "timeout":
        return 124, output
    if verb == "exit" and len(numbers) == 2:
        return numbers[0], output
    raise ConnectionError("an unexpected reply to run: %s %s" % (verb, numbers))


def shell(port: int, command: str) -> tuple[int, bytes]:
    cmd = command.encode("utf-8")
    with connect(port) as sock:
        verb, numbers, output = exchange(sock, encode("sh", [len(cmd)], [cmd]))
    if verb != "ok" or len(numbers) != 2:
        raise ConnectionError("an unexpected reply to sh: %s %s" % (verb, numbers))
    return numbers[0], output


# ---------------------------------------------------------------------------
# The guest
# ---------------------------------------------------------------------------

def first_existing(paths: tuple[str, ...], what: str) -> str:
    for p in paths:
        if os.path.exists(p):
            return p
    raise Usage("%s not found (looked in: %s)" % (what, ", ".join(paths)))


def qemu_command(port: int, esp: str, swap: str, rootfs: str | None, serial: str) -> list[str]:
    qemu = first_existing(QEMU_CANDIDATES, "qemu-system-x86_64")
    ovmf = first_existing(OVMF_CANDIDATES, "OVMF firmware")
    cmd = [
        qemu,
        "-machine", "q35",
        "-m", "3072M",
        "-cpu", "qemu64,+smep,+smap,+umip",
        "-drive", "if=pflash,format=raw,readonly=on,file=%s" % ovmf,
        "-drive", "format=raw,file=fat:rw:%s" % esp,
        # Snapshots: nothing the guest writes reaches the host's files.
        "-device", "virtio-blk-pci,drive=swap-disk",
        "-drive", "id=swap-disk,if=none,format=raw,snapshot=on,file=%s" % swap,
    ]
    if rootfs:
        cmd += [
            "-device", "virtio-blk-pci,drive=rootfs-disk",
            "-drive", "id=rootfs-disk,if=none,format=raw,snapshot=on,file=%s" % rootfs,
        ]
    cmd += [
        "-netdev", "user,id=net0",
        "-device", "virtio-net-pci,netdev=net0",
        "-device", "virtio-serial-pci,max_ports=2",
        "-chardev", "socket,id=agent,host=127.0.0.1,port=%d,server=on,wait=off" % port,
        "-device", "virtserialport,chardev=agent,name=%s" % AGENT_PORT_NAME,
        # The boot test's display devices: two of the boot's halting
        # self-tests are virtio-gpu's.
        "-device", "virtio-gpu-pci",
        "-vga", "std",
        "-display", "none",
        "-serial", "file:%s" % serial,
        "-no-reboot",
    ]
    return cmd


def guest_limine_conf(text: str) -> str:
    """`text`, the boot test's limine.conf, with GUEST_CMDLINE_WORDS on the
    first entry's command line -- the entry Limine starts by itself.

    The boot test gives that entry a `cmdline:` line after its `kernel_path:`
    (boot-test.sh, where it writes $ESP_DIR/limine.conf); the words go on the
    end of it. An entry without one gets one in the same place. Later entries
    (the recovery entry) are left as they are."""
    lines = text.split("\n")
    entry = 0
    cmdline = None
    kernel_path = None
    for i, line in enumerate(lines):
        if line.startswith("/"):
            entry += 1
            continue
        if entry != 1:
            continue
        words = line.split()
        if words[:1] == ["cmdline:"] and cmdline is None:
            cmdline = i
        elif words[:1] == ["kernel_path:"] and kernel_path is None:
            kernel_path = i
    extra = " ".join(GUEST_CMDLINE_WORDS)
    if cmdline is not None:
        lines[cmdline] = lines[cmdline].rstrip() + " " + extra
    elif kernel_path is not None:
        lines.insert(kernel_path + 1, "    cmdline: " + extra)
    else:
        raise Usage("limine.conf's first entry has no kernel_path: line, so the guest's "
                    "command line has nowhere to go")
    return "\n".join(lines)


def running_pid() -> int | None:
    """The PID this tool recorded, when that process is still alive."""
    try:
        with open(PIDFILE, encoding="utf-8") as f:
            pid = int(f.read().strip())
    except (OSError, ValueError):
        return None
    return pid if pid_alive(pid) else None


def pid_alive(pid: int) -> bool:
    if os.name == "nt":
        out = subprocess.run(
            ["tasklist", "/FI", "PID eq %d" % pid, "/NH"],
            capture_output=True, text=True, encoding="utf-8", errors="replace",
        ).stdout
        return str(pid) in out.split()
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def port_free(port: int) -> bool:
    """Whether QEMU will be able to listen on 127.0.0.1:`port`. Windows
    reserves whole ranges of ports (`netsh interface ipv4 show
    excludedportrange protocol=tcp`), and QEMU refused one exits at once."""
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        s.bind(("127.0.0.1", port))
        return True
    except OSError:
        return False
    finally:
        s.close()


def start(port: int, timeout: float) -> None:
    if running_pid() is not None:
        raise Usage("a guest is already running (PID in %s); `stop` it first" % PIDFILE)
    if not port_free(port):
        raise Usage("127.0.0.1:%d cannot be listened on -- in use, or in a range Windows "
                    "reserves; pass --port" % port)
    esp_src = os.path.join(ROOT, "build", "esp")
    swap = os.path.join(ROOT, "build", "swap.img")
    for need in (esp_src, swap):
        if not os.path.exists(need):
            raise Usage("%s is missing: run scripts/boot-test.sh once to build it" % need)
    rootfs = os.path.join(ROOT, "rootfs.ext4")
    os.makedirs(GUEST_DIR, exist_ok=True)
    esp = os.path.join(GUEST_DIR, "esp")
    # A copy of the boot test's ESP: QEMU's `fat:rw` writes to the directory it
    # is given, and a boot test in this worktree rewrites its own.
    if os.path.exists(esp):
        shutil.rmtree(esp)
    shutil.copytree(esp_src, esp)
    conf = os.path.join(esp, "limine.conf")
    try:
        with open(conf, encoding="utf-8", newline="") as f:
            text = f.read()
    except OSError as e:
        raise Usage("%s cannot be read (%s): run scripts/boot-test.sh once to build it" % (conf, e))
    with open(conf, "w", encoding="utf-8", newline="") as f:
        f.write(guest_limine_conf(text))
    serial = os.path.join(GUEST_DIR, "serial.txt")
    cmd = qemu_command(port, esp, swap, rootfs if os.path.exists(rootfs) else None, serial)
    with open(os.path.join(GUEST_DIR, "qemu.log"), "wb") as log:
        flags = 0
        if os.name == "nt":
            flags = subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP
        proc = subprocess.Popen(
            cmd, stdin=subprocess.DEVNULL, stdout=log, stderr=log,
            creationflags=flags, close_fds=True,
        )
    with open(PIDFILE, "w", encoding="utf-8", newline="") as f:
        f.write(str(proc.pid))
    with open(PORT_FILE, "w", encoding="utf-8", newline="") as f:
        f.write(str(port))
    print("guest: QEMU started (PID %d), serial log %s; waiting for the agent..." % (proc.pid, serial))
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise Usage("QEMU exited with %s; see %s" % (proc.returncode, os.path.join(GUEST_DIR, "qemu.log")))
        if ping(port, wait=5.0):
            print("guest: the agent answers on 127.0.0.1:%d" % port)
            return
        time.sleep(2.0)
    raise Usage("the agent did not answer within %d s; the guest is still running -- see %s" % (timeout, serial))


def stop() -> None:
    pid = running_pid()
    if pid is None:
        print("guest: none running")
        return
    # Only the process this tool started, by its PID.
    if os.name == "nt":
        subprocess.run(["taskkill", "/PID", str(pid), "/F"], capture_output=True)
    else:
        os.kill(pid, 9)
    os.remove(PIDFILE)
    print("guest: stopped QEMU (PID %d)" % pid)


def guest_port(cli_port: int | None) -> int:
    if cli_port is not None:
        return cli_port
    try:
        with open(PORT_FILE, encoding="utf-8") as f:
            return int(f.read().strip())
    except (OSError, ValueError):
        return DEFAULT_PORT


# ---------------------------------------------------------------------------
# The command line
# ---------------------------------------------------------------------------

def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(prog="guest.py", description=__doc__.split("\n\n")[0])
    ap.add_argument("--port", type=int, default=None, help="the agent's TCP port (default: the one `start` used, else %d)" % DEFAULT_PORT)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("start")
    s.add_argument("--timeout", type=float, default=900.0)
    sub.add_parser("stop")
    sub.add_parser("ping")
    p = sub.add_parser("put")
    p.add_argument("local")
    p.add_argument("guest_path")
    p.add_argument("--mode", default="755")
    g = sub.add_parser("get")
    g.add_argument("guest_path")
    g.add_argument("local")
    for name in ("run", "try"):
        r = sub.add_parser(name)
        r.add_argument("--seconds", type=int, default=60)
        r.add_argument("--grants", default="file")
        r.add_argument("program")
        r.add_argument("args", nargs=argparse.REMAINDER)
    h = sub.add_parser("sh")
    h.add_argument("command")
    args = ap.parse_args(argv)
    port = guest_port(args.port)

    try:
        if args.cmd == "start":
            start(port if args.port is not None else DEFAULT_PORT, args.timeout)
            return 0
        if args.cmd == "stop":
            stop()
            return 0
        if args.cmd == "ping":
            ok = ping(port)
            print("pong" if ok else "no answer on 127.0.0.1:%d" % port)
            return 0 if ok else 2
        if args.cmd == "put":
            put(port, args.local, args.guest_path, args.mode)
            return 0
        if args.cmd == "get":
            data = get(port, args.guest_path)
            with open(args.local, "wb") as f:
                f.write(data)
            return 0
        if args.cmd in ("run", "try"):
            program = args.program
            if args.cmd == "try":
                program = "/tmp/" + os.path.basename(args.program)
                put(port, args.program, program, "755")
            code, output = run(port, args.seconds, args.grants, [program] + args.args)
            sys.stdout.buffer.write(output)
            sys.stdout.flush()
            if code == 124:
                print("guest: %s ran out of its %d s" % (program, args.seconds), file=sys.stderr)
            return code if 0 <= code <= 255 else 1
        if args.cmd == "sh":
            status, output = shell(port, args.command)
            sys.stdout.buffer.write(output)
            sys.stdout.flush()
            return status
    except AgentError as e:
        print("guest: the agent refused: %s" % e, file=sys.stderr)
        return 1
    except (Usage, OSError, ConnectionError, ValueError) as e:
        print("guest: %s" % e, file=sys.stderr)
        return 2
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
