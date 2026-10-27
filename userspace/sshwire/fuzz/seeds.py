"""Seeds for sshwire's targets: an identification line, a KEXINIT's fields,
base64 and an OpenSSH private key's armour, two big numbers, and clear and
"encrypted" packets. Called by `scripts/fuzz.py` as `seed(src, corpus)`."""
import os
import struct


def ssh_string(b):
    return struct.pack(">I", len(b)) + b


KEXINIT = bytes([20]) + bytes(16) + b"".join(ssh_string(s) for s in (
    b"diffie-hellman-group14-sha256", b"ssh-ed25519", b"aes128-ctr", b"aes128-ctr",
    b"hmac-sha2-256", b"hmac-sha2-256", b"none", b"none", b"", b"")) + bytes([0]) + bytes(4)


def packet(payload, block=8):
    pad = block - (5 + len(payload)) % block
    if pad < 4:
        pad += block
    return struct.pack(">IB", 1 + len(payload) + pad, pad) + payload + bytes(pad)


def seed(src, corpus):
    def put(target, label, data):
        open(os.path.join(corpus(target), label), "wb").write(data)

    put("wire", "ident", b"\x00SSH-2.0-OpenSSH_9.6p1 Ubuntu-3\r\n")
    put("wire", "fields", b"\x01" + bytes([2, 2, 0, 1, 3, 2, 2, 2] * 2) + KEXINIT[17:])
    put("wire", "base64", b"\x02" + b"c3NoLWVkMjU1MTkgQUFBQUMzTnphQzFsWkRJMU5URTVBQUFBSUE=")
    put("wire", "privkey", b"\x02-----BEGIN OPENSSH PRIVATE KEY-----\n"
        b"b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n"
        b"-----END OPENSSH PRIVATE KEY-----\n")
    put("wire", "bignum", b"\x03" + bytes(range(1, 65)) + bytes(range(64, 0, -1)))
    put("packet", "clear-kexinit", b"\x00" + packet(KEXINIT))
    put("packet", "clear-two", b"\x00" + packet(bytes([21])) + packet(bytes([5]) + ssh_string(b"ssh-userauth")))
    put("packet", "encrypted", b"\x01" + bytes(range(96)))
    put("packet", "encrypted-server", b"\x03" + bytes(range(96)))
