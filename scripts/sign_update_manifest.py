#!/usr/bin/env python3
"""Sign exact update-manifest.json bytes with an Ed25519 seed held in CI Secrets."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import tempfile

PRIVATE_DER_PREFIX = bytes.fromhex("302e020100300506032b657004220420")
PUBLIC_DER_PREFIX = bytes.fromhex("302a300506032b6570032100")


def decode_key(value: str) -> bytes:
    try:
        key = bytes.fromhex(value.strip())
    except ValueError:
        raise ValueError("Update key must contain 64 hexadecimal characters") from None
    if len(key) != 32 or len(value.strip()) != 64:
        raise ValueError("Update key must contain 64 hexadecimal characters")
    return key


def openssl(*args: str) -> bytes:
    result = subprocess.run(["openssl", *args], capture_output=True, check=False)
    if result.returncode:
        # Never print key material or opaque OpenSSL diagnostics in CI logs.
        raise ValueError("OpenSSL update signing/verification failed")
    return result.stdout


def private_key_file(root: Path, seed: bytes) -> Path:
    key = root / "key.der"
    descriptor = os.open(key, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(PRIVATE_DER_PREFIX + seed)
    return key


def public_key(key: Path) -> bytes:
    encoded = openssl("pkey", "-inform", "DER", "-in", str(key), "-pubout", "-outform", "DER")
    if len(encoded) != 44 or not encoded.startswith(PUBLIC_DER_PREFIX):
        raise ValueError("OpenSSL returned an invalid Ed25519 public key")
    return encoded[len(PUBLIC_DER_PREFIX):]


def sign(manifest: Path, output: Path, seed_hex: str, public_hex: str) -> None:
    seed, expected_public = decode_key(seed_hex), decode_key(public_hex)
    with tempfile.TemporaryDirectory(prefix="tiny-shell-sign-") as directory:
        root = Path(directory)
        key = private_key_file(root, seed)
        if public_key(key) != expected_public:
            raise ValueError("Signing key does not match the public key compiled into the release")
        signature = openssl("pkeyutl", "-sign", "-rawin", "-inkey", str(key), "-keyform", "DER", "-in", str(manifest))
        if len(signature) != 64:
            raise ValueError("Invalid Ed25519 signature length")
        pub = root / "public.der"
        pub.write_bytes(PUBLIC_DER_PREFIX + expected_public)
        sig = root / "signature"
        sig.write_bytes(signature)
        openssl("pkeyutl", "-verify", "-rawin", "-pubin", "-inkey", str(pub), "-keyform", "DER", "-in", str(manifest), "-sigfile", str(sig))
        output.write_text(signature.hex() + "\n", encoding="ascii", newline="\n")


def generate(directory: Path) -> None:
    # The caller chooses a private directory outside the repository. Never overwrite keys.
    directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    seed = os.urandom(32)
    with tempfile.TemporaryDirectory(prefix="tiny-shell-sign-") as temporary:
        public = public_key(private_key_file(Path(temporary), seed))
    descriptor = os.open(directory / "update-signing-seed.hex", os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w", encoding="ascii") as stream:
        stream.write(seed.hex() + "\n")
    (directory / "update-public-key.hex").write_text(public.hex() + "\n", encoding="ascii")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--check-key", action="store_true")
    parser.add_argument("--generate", type=Path, help="New private directory outside the repository")
    args = parser.parse_args()
    try:
        if args.generate:
            generate(args.generate)
            print("Created update key files; keep the seed private.")
        elif args.check_key:
            decode_key(os.environ.get("TINY_SHELL_UPDATE_PUBLIC_KEY", ""))
        else:
            if not args.manifest or not args.output:
                parser.error("--manifest and --output are required")
            sign(args.manifest, args.output, os.environ.get("TINY_SHELL_UPDATE_SIGNING_SEED", ""), os.environ.get("TINY_SHELL_UPDATE_PUBLIC_KEY", ""))
            print("Update manifest signed and signature verified.")
    except (ValueError, OSError) as error:
        parser.exit(1, f"Update signing failed: {error}\n")


if __name__ == "__main__":
    main()
