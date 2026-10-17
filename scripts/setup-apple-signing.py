"""Import publisher-provided credentials into an ephemeral GitHub macOS runner keychain."""
import base64
import os
from pathlib import Path
import subprocess
import uuid


def run(arguments):
    result = subprocess.run(arguments, capture_output=True)
    if result.returncode:
        # Never include a command containing passwords in a traceback/log.
        raise SystemExit("Apple signing setup failed; check the configured certificate/keychain credentials")


def main():
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise SystemExit("This helper only operates on ephemeral GitHub Actions runners")
    required = ("APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD", "APPLE_API_KEY_P8", "KEYCHAIN_PASSWORD")
    if any(not os.environ.get(name) for name in required):
        raise SystemExit("Missing Apple signing secrets")
    root = Path(os.environ["RUNNER_TEMP"])
    keychain = root / f"speak-{uuid.uuid4()}.keychain-db"
    certificate = root / f"speak-{uuid.uuid4()}.p12"
    api_key = root / f"speak-{uuid.uuid4()}.p8"
    certificate.write_bytes(base64.b64decode(os.environ["APPLE_CERTIFICATE"], validate=True))
    api_key.write_text(os.environ["APPLE_API_KEY_P8"])
    certificate.chmod(0o600); api_key.chmod(0o600)
    password = os.environ["KEYCHAIN_PASSWORD"]
    try:
        run(["security", "create-keychain", "-p", password, str(keychain)])
        run(["security", "set-keychain-settings", "-lut", "21600", str(keychain)])
        run(["security", "unlock-keychain", "-p", password, str(keychain)])
        run(["security", "import", str(certificate), "-k", str(keychain), "-P", os.environ["APPLE_CERTIFICATE_PASSWORD"], "-T", "/usr/bin/codesign"])
        run(["security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:", "-s", "-k", password, str(keychain)])
        existing = subprocess.check_output(["security", "list-keychains", "-d", "user"], text=True)
        paths = [line.strip().strip('"') for line in existing.splitlines() if line.strip()]
        run(["security", "list-keychains", "-d", "user", "-s", str(keychain), *paths])
        with open(os.environ["GITHUB_ENV"], "a") as env:
            env.write(f"APPLE_API_KEY_PATH={api_key}\nSPEAK_TEMP_KEYCHAIN={keychain}\n")
    finally:
        certificate.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
