"""Mutation test for `seal`, the plain API over the vendored RustCrypto crates.

Breaks one piece of `seal`'s own code at a time and checks that the test which
claims to cover it is the one that fails. The vendored crates are not mutated
here -- they are upstream's, checked by their published vectors in
`tests/vectors.rs` -- but every choice `seal` makes on the way to them is: the
algorithm, the version, every parameter, the associated data, the nonce, and
which error a failure becomes.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

ROUND_TRIP = "what_is_sealed_opens_with_the_same_key_nonce_and_data"
CHANGED = "nothing_opens_that_was_changed_in_any_way"
DEPENDS = "a_key_depends_on_the_password_the_salt_and_every_parameter"
REFUSED = "parameters_argon2_refuses_are_refused"
RECOMMENDED = "the_recommended_setting_is_rfc_9106s_second"
XCHACHA = "xchacha20_poly1305_draft_a_3_1"
WYCHEPROOF = "wycheproof_xchacha20_poly1305_through_seal"
REFERENCE = "argon2id_reference_vectors_through_seal"

MUTATIONS = [
    (
        "the associated data is not bound",
        "                msg: plaintext,\n                aad,\n",
        "                msg: plaintext,\n                aad: b\"\",\n",
        [ROUND_TRIP, XCHACHA, WYCHEPROOF],
    ),
    (
        "decrypt ignores the nonce it is given",
        "        .decrypt(&Array(*nonce), Payload { msg: sealed, aad })",
        "        .decrypt(&Array([0; NONCE_LEN]), Payload { msg: sealed, aad })",
        [ROUND_TRIP, XCHACHA, WYCHEPROOF],
    ),
    (
        "a forgery opens",
        "        .decrypt(&Array(*nonce), Payload { msg: sealed, aad })\n"
        "        .map_err(|_| Error::Unauthentic)",
        "        .decrypt(&Array(*nonce), Payload { msg: sealed, aad })\n"
        "        .or_else(|_| Ok::<Vec<u8>, Error>(sealed.to_vec()))",
        [CHANGED, WYCHEPROOF],
    ),
    (
        "Argon2i instead of Argon2id",
        "        argon2::Algorithm::Argon2id,",
        "        argon2::Algorithm::Argon2i,",
        [REFERENCE],
    ),
    (
        "the old Argon2 version",
        "        argon2::Version::V0x13,",
        "        argon2::Version::V0x10,",
        [REFERENCE],
    ),
    (
        "the lanes asked for are ignored",
        "        params.lanes,\n        Some(KEY_LEN),",
        "        1,\n        Some(KEY_LEN),",
        [DEPENDS, REFERENCE],
    ),
    (
        "the passes asked for are ignored",
        "        params.iterations,\n        params.lanes,",
        "        2,\n        params.lanes,",
        [DEPENDS, REFERENCE],
    ),
    (
        "a bad salt reads as bad parameters",
        "            argon2::Error::SaltTooShort | argon2::Error::SaltTooLong => Error::BadSalt,\n",
        "",
        [REFUSED],
    ),
    (
        "the recommended setting makes one pass",
        "        iterations: 3,\n        lanes: 4,",
        "        iterations: 1,\n        lanes: 4,",
        [RECOMMENDED],
    ),
]

if __name__ == "__main__":
    raise SystemExit(sweep(SRC, MUTATIONS, "seal", timeout=900, only=sys.argv[1:] or None))
