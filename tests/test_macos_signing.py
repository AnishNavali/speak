"""Opt-in: verifies local signing durability using two small, never-executed binaries."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest


@unittest.skipUnless(sys.platform == "darwin" and os.environ.get("SPEAK_SIGNING_TEST") == "1",
                     "requires the locally configured signing certificate and signed macOS app")
class MacSigningCheck(unittest.TestCase):
    def test_different_binaries_keep_the_same_certificate_requirement(self):
        root = Path(__file__).resolve().parents[1]
        app = root / "target/debug/bundle/macos/Speak.app"
        config = json.loads((root / "src-tauri/tauri.macos.conf.json").read_text())
        identity = config["bundle"]["macOS"]["signingIdentity"]
        result = subprocess.run(["codesign", "-dr", "-", str(app)], capture_output=True, text=True, check=True)
        requirement = re.search(r"designated => (.+)", result.stdout + result.stderr).group(1)
        self.assertNotIn("cdhash", requirement)
        self.assertIn('identifier "app.speak.local"', requirement)
        self.assertIn("certificate", requirement)

        hashes = []
        with tempfile.TemporaryDirectory(prefix="speak-signing-check-") as temp:
            for number in (0, 1):
                source = Path(temp) / f"version-{number}.c"
                binary = source.with_suffix("")
                source.write_text(f"int main(void) {{ return {number}; }}\n")
                subprocess.run(["clang", str(source), "-o", str(binary)], check=True)
                # The former ad-hoc identity must not satisfy the new certificate-based requirement.
                subprocess.run(["codesign", "--force", "--sign", "-", "--identifier", "app.speak.local", str(binary)], check=True, capture_output=True)
                old = subprocess.run(["codesign", "--verify", "-R", "=" + requirement, str(binary)], capture_output=True)
                self.assertNotEqual(old.returncode, 0)
                subprocess.run(["codesign", "--force", "--sign", identity, "--timestamp=none", "--identifier", "app.speak.local", str(binary)], check=True, capture_output=True)
                checked = subprocess.run(["codesign", "--verify", "--strict", "-R", "=" + requirement, str(binary)], capture_output=True, text=True)
                self.assertEqual(checked.returncode, 0, checked.stderr)
                metadata = subprocess.run(["codesign", "-dv", "--verbose=4", str(binary)], check=True, capture_output=True, text=True).stderr
                hashes.append(re.search(r"^CDHash=(\w+)", metadata, re.MULTILINE).group(1))
        self.assertNotEqual(*hashes, "Test binaries must have different code hashes")
