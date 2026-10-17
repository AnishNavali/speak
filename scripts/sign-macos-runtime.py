"""Sign packaged Mach-O helpers/libraries before sealing the macOS app bundle."""
import argparse
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ad-hoc", action="store_true", help="Sign without a certificate or notarization for unsigned beta builds")
    args = parser.parse_args()
    identity = "-" if args.ad_hoc else os.environ.get("APPLE_SIGNING_IDENTITY", "")
    if not args.ad_hoc and not identity.startswith("Developer ID Application:"):
        raise SystemExit("Public runtime signing requires a Developer ID Application identity")
    root = Path(__file__).resolve().parents[1] / "src-tauri/runtime"
    magic = {b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca"}
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.is_symlink():
            continue
        with path.open("rb") as file:
            native = file.read(4) in magic
        if native:
            subprocess.run(["codesign", "--force", "--sign", identity]
                           + ([] if args.ad_hoc else ["--timestamp", "--options", "runtime"])
                           + [str(path)], check=True)


if __name__ == "__main__":
    main()
