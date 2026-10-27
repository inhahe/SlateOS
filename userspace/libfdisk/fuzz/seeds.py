"""Seed corpora for libfdisk's targets: `scripts/fuzz.py` calls `seed(src, corpus)`.

`probe` is seeded with the first two mebibytes of disks Ubuntu's `sfdisk`
lays out -- an MBR with an extended chain, a GPT, an MBR holding a BSD
label's partition type -- made here, since none is checked in; `script`
with the scripts that make them, and the shapes of line sfdisk reads. See
README.md."""
import os
import subprocess
import tempfile

SCRIPTS = {
    "dos": "label: dos\nlabel-id: 0x12345678\n,256K,83\n,256K,82,*\n,1M,5\n,128K,83\n,,8e\n",
    "gpt": "label: gpt\nlabel-id: 4A6F7B1C-1111-4222-8333-944455566677\n"
    'size=256K, type=L, uuid=01234567-89AB-4CDE-8F01-23456789ABCD, name="first"\n'
    "size=512K, type=S\n",
    "bsd": "label: dos\n,1M,a5\n,,83\n",
    "sun": "label: sun\n,512K,83\n,,82\n",
    "sgi": "label: sgi\n,512K,83\n,,82\n",
    "oldstyle": "2048,1024,83,*\n4096,,82\n",
    "headers": "unit: sectors\nfirst-lba: 64\nlast-lba: 4000\ntable-length: 16\nlabel: gpt\n,64\n",
}


def seed(src, corpus):
    probe = corpus("probe")
    script = corpus("script")
    for name, text in SCRIPTS.items():
        with open(os.path.join(script, name), "w", encoding="utf-8", newline="") as f:
            f.write(text)
        with tempfile.TemporaryDirectory() as d:
            img = os.path.join(d, "img")
            with open(img, "wb") as f:
                f.truncate(2 * 1024 * 1024)
            r = subprocess.run(
                ["sfdisk", "-q", "--no-reread", "--no-tell-kernel", img],
                input=text.encode("utf-8"),
                capture_output=True,
                check=False,
            )
            if r.returncode != 0:
                continue
            with open(img, "rb") as f:
                data = f.read()
            # The image less its zeros at the end: the fuzzer's inputs are
            # the image's front, and the rest is zeros again.
            with open(os.path.join(probe, name), "wb") as f:
                f.write(data.rstrip(b"\0"))
