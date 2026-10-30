import shutil
import tempfile
import unittest
from pathlib import Path

from scripts.sign_update_manifest import decode_key, generate, sign


class UpdateSigningTests(unittest.TestCase):
    def test_bad_keys_are_rejected(self):
        for value in ["", "00", "x" * 64, "00 " * 32]:
            with self.assertRaises(ValueError):
                decode_key(value)

    @unittest.skipUnless(shutil.which("openssl"), "OpenSSL is required for real Ed25519 signing")
    def test_sign_exact_bytes_and_reject_wrong_public_key(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            generate(root / "keys")
            seed = (root / "keys/update-signing-seed.hex").read_text()
            public = (root / "keys/update-public-key.hex").read_text()
            manifest, signature = root / "manifest.json", root / "manifest.sig"
            manifest.write_bytes(b'{"schema_version":1}\n')
            sign(manifest, signature, seed, public)
            original = signature.read_text()
            self.assertEqual(len(original.strip()), 128)
            manifest.write_bytes(b'{"schema_version":1}')
            sign(manifest, signature, seed, public)
            self.assertNotEqual(signature.read_text(), original)
            with self.assertRaises(ValueError):
                sign(manifest, signature, seed, "00" * 32)


if __name__ == "__main__":
    unittest.main()
