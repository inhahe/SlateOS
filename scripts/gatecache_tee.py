"""Run one read-only git command for a traced gate, and record what it answered.

    python scripts/gatecache_tee.py <record.json> piped|none -- git <args...>

Started by `gatecache_trace.py` in place of a `git` child that reads a
repository the run did not create. It relays the caller's stdin to git and
git's stdout and stderr back, byte for byte, while hashing all three. The
record it writes lets `gate-cache.py` re-run the same command at lookup and
compare: if git would answer differently now, the cached verdict is stale.

Recording here, rather than re-running git after the gate finishes, is what
makes the recorded answer the one the gate actually saw. A ref another lane
moved while the gate ran would otherwise be recorded as the new answer beside
a verdict computed from the old one.

`piped`: the caller supplied stdin (a pipe or a file), and it is relayed and
recorded. `none`: the caller did not, and git gets the null device -- the
tracer refuses to cache a command that would read an inherited stdin.
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import subprocess
import sys
import threading

#: Past this much stdin the record would be unwieldy; the run is not stored.
MAX_STDIN = 16 << 20


def main() -> int:
    if len(sys.argv) < 5 or sys.argv[3] != "--":
        print("usage: gatecache_tee.py <record.json> piped|none -- git <args...>",
              file=sys.stderr)
        return 2
    record, mode, argv = sys.argv[1], sys.argv[2], sys.argv[4:]
    piped = mode == "piped"
    proc = subprocess.Popen(argv, stdin=subprocess.PIPE if piped else subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stdin_kept = bytearray()
    overflow = [False]
    hashes = {"stdout": hashlib.sha256(), "stderr": hashlib.sha256()}

    def pump_in() -> None:
        src = sys.stdin.buffer
        try:
            while True:
                chunk = src.read1(1 << 16)
                if not chunk:
                    break
                if len(stdin_kept) + len(chunk) > MAX_STDIN:
                    overflow[0] = True
                else:
                    stdin_kept.extend(chunk)
                proc.stdin.write(chunk)
                proc.stdin.flush()
        except (BrokenPipeError, OSError, ValueError):
            pass
        finally:
            try:
                proc.stdin.close()
            except OSError:
                pass

    def pump_out(name: str, src, dst) -> None:
        try:
            while True:
                chunk = src.read1(1 << 16)
                if not chunk:
                    break
                hashes[name].update(chunk)
                try:
                    dst.write(chunk)
                    dst.flush()
                except (BrokenPipeError, OSError, ValueError):
                    pass  # the caller stopped reading; the hash still sees it all
        except (OSError, ValueError):
            pass

    threads = [threading.Thread(target=pump_out, daemon=True,
                                args=("stderr", proc.stderr, sys.stderr.buffer))]
    if piped:
        threads.append(threading.Thread(target=pump_in, daemon=True))
    for th in threads:
        th.start()
    pump_out("stdout", proc.stdout, sys.stdout.buffer)
    rc = proc.wait()
    for th in threads:
        th.join(timeout=10)
    out = {
        "argv": argv,
        "cwd": os.getcwd(),
        "piped": piped,
        "stdin": base64.b64encode(bytes(stdin_kept)).decode("ascii"),
        "stdin_overflow": overflow[0],
        "stdout_sha": hashes["stdout"].hexdigest(),
        "stderr_sha": hashes["stderr"].hexdigest(),
        "rc": rc,
    }
    tmp = record + ".tmp"
    try:
        with open(tmp, "w", encoding="utf-8", newline="\n") as fh:
            json.dump(out, fh)
        os.replace(tmp, record)
    except OSError:
        pass  # no record: the driver refuses to store the run
    return rc


if __name__ == "__main__":
    sys.exit(main())
