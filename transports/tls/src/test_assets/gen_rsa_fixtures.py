"""Generate the RSA certificate fixtures for the handshake tests and the CPU profile.

rsa-2048.pk8 is a copy of identity/src/test/rsa-2048.pk8. It is kept here so
that the tests can sign with a known RSA key.

Each certificate carries a libp2p SignedKey extension. A throwaway Ed25519 host
key signs it, and the script does not keep that host key.

The PSS fixtures use a salt length equal to the digest length.

Run with python3 -I gen_rsa_fixtures.py.
"""

import pathlib
import subprocess
import tempfile


def openssl(*args):
    return subprocess.run(["openssl", *map(str, args)], check=True, capture_output=True).stdout


def der(tag, value):
    size = len(value)
    length = bytes([size]) if size < 128 else bytes([0x81, size])
    return bytes([tag]) + length + value


assets = pathlib.Path(__file__).resolve().parent
key = assets / "rsa-2048.pk8"
spki = openssl("pkey", "-inform", "DER", "-in", key, "-pubout", "-outform", "DER")
with tempfile.TemporaryDirectory() as temp:
    temp = pathlib.Path(temp)
    host = temp / "host.pem"
    openssl("genpkey", "-algorithm", "ED25519", "-out", host)
    public = openssl("pkey", "-in", host, "-pubout", "-outform", "DER")
    assert public[:12] == bytes.fromhex("302a300506032b6570032100") and len(public) == 44
    message = temp / "message"
    message.write_bytes(b"libp2p-tls-handshake:" + spki)
    signature = openssl("pkeyutl", "-sign", "-rawin", "-inkey", host, "-in", message)
    extension = der(0x30, der(0x04, b"\x08\x01\x12\x20" + public[12:]) + der(0x04, signature))
    config = temp / "openssl.cnf"
    config.write_text(
        "[req]\nprompt=no\ndistinguished_name=dn\nx509_extensions=extensions\n"
        "[dn]\nCN=libp2p RSA profiling fixture\n"
        "[extensions]\n1.3.6.1.4.1.53594.1.1=critical,DER:" + extension.hex() + "\n"
    )
    for padding in ("pkcs1", "pss"):
        for digest in ("sha256", "sha384", "sha512"):
            destination = assets / f"rsa_fixture_{padding}_{digest}.der"
            options = [] if padding == "pkcs1" else [
                "-sigopt", "rsa_padding_mode:pss", "-sigopt", "rsa_pss_saltlen:digest"
            ]
            openssl(
                "req", "-new", "-x509", "-key", key, "-keyform", "DER",
                "-days", "36500", f"-{digest}", "-config", config,
                "-outform", "DER", "-out", destination, *options
            )
            print(destination.name)
