"""Pin an installed signing identity for local Tauri builds. No certificates are created or exported."""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import subprocess


def select_identity(identities, requested=None):
    if requested:
        matches = [(digest, name) for digest, name in identities
                   if digest.casefold() == requested.casefold() or name == requested]
        if len(matches) != 1:
            raise ValueError("The selected signing identity is unavailable. Use --identity with a valid certificate name or SHA-1.")
        return matches[0]
    if len(identities) != 1:
        raise ValueError("Expected one valid Apple Development / Developer ID Application identity. Choose one explicitly with --identity.")
    return identities[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--identity", default=os.environ.get("APPLE_SIGNING_IDENTITY"),
                        help="Installed certificate name or SHA-1; otherwise preserve the existing choice")
    args = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("This helper is for local macOS builds")

    output = subprocess.check_output(["security", "find-identity", "-v", "-p", "codesigning"], text=True)
    identities = [(digest, name) for digest, name in re.findall(r'^\s*\d+\)\s+([A-Fa-f0-9]{40})\s+"([^"]+)"', output, re.MULTILINE)
                  if name.startswith(("Apple Development:", "Developer ID Application:"))]
    path = Path(__file__).resolve().parents[1] / "src-tauri" / "tauri.macos.conf.json"
    config = json.loads(path.read_text()) if path.exists() else {}
    macos = config.setdefault("bundle", {}).setdefault("macOS", {})
    digest, name = select_identity(identities, args.identity or macos.get("signingIdentity"))
    if macos.get("signingIdentity") != digest:
        macos["signingIdentity"] = digest
        temp = path.with_suffix(".json.tmp")
        temp.write_text(json.dumps(config, indent=2) + "\n")
        temp.replace(path)
    print(f"Local macOS signing configured: {name}")
    print("Tauri automatically loads src-tauri/tauri.macos.conf.json for every local macOS bundle build.")


if __name__ == "__main__":
    main()
