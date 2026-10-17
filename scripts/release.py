"""Configure, collect, and merge versioned GitHub release artifacts. Does not publish."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "aarch64-apple-darwin": ("darwin-aarch64", ["app", "dmg"]),
    "x86_64-apple-darwin": ("darwin-x86_64", ["app", "dmg"]),
    "x86_64-pc-windows-msvc": ("windows-x86_64", ["nsis"]),
    "x86_64-unknown-linux-gnu": ("linux-x86_64", ["appimage", "deb", "rpm"]),
}
CLI = ["npx", "--yes", "@tauri-apps/cli@2.12.3"]


def version():
    config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
    cargo = tomllib.loads((ROOT / "src-tauri/Cargo.toml").read_text())
    if config["version"] != cargo["package"]["version"]:
        raise ValueError("Cargo and Tauri versions differ")
    tag = os.environ.get("GITHUB_REF_NAME", "v" + config["version"])
    if os.environ.get("GITHUB_REF_TYPE") == "tag" and tag != "v" + config["version"]:
        raise ValueError("Release tag must match the application version")
    return config["version"]


def repository():
    value = os.environ.get("SPEAK_RELEASE_REPOSITORY", "")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", value) or any(v in (".", "..") for v in value.split("/")):
        raise ValueError("Set SPEAK_RELEASE_REPOSITORY to the actual GitHub owner/repository")
    return value


def require(names):
    missing = [name for name in names if not os.environ.get(name)]
    if missing:
        raise ValueError("Missing release configuration: " + ", ".join(missing))


def configure(target, public, os_signing=False):
    if os_signing and not public:
        raise ValueError("--os-signing requires --public")
    version()
    config = {"bundle": {"targets": TARGETS[target][1], "createUpdaterArtifacts": public}}
    if "apple" in target and not os_signing:
        # Ad-hoc signatures let Apple Silicon run the bundle without publisher credentials.
        config["bundle"]["macOS"] = {"signingIdentity": "-", "hardenedRuntime": False}
    if public:
        repository()
        require(["SPEAK_UPDATER_PUBLIC_KEY", "TAURI_SIGNING_PRIVATE_KEY"])
        config["plugins"] = {"updater": {"pubkey": os.environ["SPEAK_UPDATER_PUBLIC_KEY"].strip(), "requireSignedVersion": True}}
        if os_signing and "apple" in target:
            require(["APPLE_SIGNING_IDENTITY", "APPLE_API_KEY", "APPLE_API_ISSUER", "APPLE_API_KEY_PATH"])
            if not os.environ["APPLE_SIGNING_IDENTITY"].startswith("Developer ID Application:"):
                raise ValueError("Public Mac builds require Developer ID Application, not Apple Development/ad-hoc")
            config["bundle"]["macOS"] = {"signingIdentity": os.environ["APPLE_SIGNING_IDENTITY"]}
        if os_signing and "windows" in target:
            require(["AZURE_SIGNING_CLIENT_ID", "AZURE_SIGNING_TENANT_ID", "AZURE_SIGNING_SUBSCRIPTION_ID",
                     "AZURE_SIGNING_ENDPOINT", "AZURE_SIGNING_ACCOUNT", "AZURE_SIGNING_PROFILE"])
    path = ROOT / "target/release-config.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(config, indent=2))
    # Include the exact dependency/license metadata with the release downloads.
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--filter-platform", target, "--format-version", "1"], cwd=ROOT, text=True))
    resolved = {node["id"] for node in metadata["resolve"]["nodes"]}
    licenses = [{key: package.get(key) for key in ("name", "version", "license", "repository")}
                for package in metadata["packages"] if package["id"] in resolved]
    (ROOT / "target/dependency-licenses.json").write_text(json.dumps(licenses, indent=2))
    print(path)


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def collect(target):
    release_version = version()
    repo = repository()
    require(["TAURI_SIGNING_PRIVATE_KEY"])
    platform_key = TARGETS[target][0]
    bundle = ROOT / "target" / target / "release/bundle"
    output = ROOT / "dist" / platform_key
    output.mkdir(parents=True, exist_ok=True)
    found = []
    update = None
    for source in sorted(bundle.rglob("*")):
        if not source.is_file():
            continue
        suffix = next((suffix for suffix in (".app.tar.gz", ".AppImage", ".dmg", ".deb", ".rpm", ".exe") if source.name.endswith(suffix)), None)
        if not suffix or ".app/" in source.as_posix() or (suffix == ".exe" and source.parent.name != "nsis"):
            continue
        expected_parent = {".app.tar.gz": "macos", ".AppImage": "appimage", ".dmg": "dmg", ".deb": "deb", ".rpm": "rpm", ".exe": "nsis"}[suffix]
        if source.parent.name != expected_parent:
            continue
        name = f"Speak_{release_version}_{platform_key}" + ("_setup" if suffix == ".exe" else "") + suffix
        destination = output / name
        if destination.exists():
            raise ValueError(f"Ambiguous or stale release output: {destination}; clean dist first")
        shutil.copy2(source, destination)
        if destination.stat().st_size >= 2 * 1024**3:
            raise ValueError("Release assets must be smaller than 2 GiB")
        found.append(destination)
        if suffix in (".app.tar.gz", ".AppImage", ".exe"):
            # Windows Authenticode signing happens after bundling; sign the final bytes again.
            subprocess.run(CLI + ["signer", "sign", "--app-version", release_version, str(destination)], cwd=ROOT, check=True)
            signature = Path(str(destination) + ".sig")
            if not signature.is_file() or not signature.read_text().strip():
                raise ValueError("Updater signature was not generated")
            verifier = ROOT / "target/debug" / ("speak.exe" if os.name == "nt" else "speak")
            subprocess.run([str(verifier), "--verify-update-signature", str(destination), str(signature)], check=True)
            found.append(signature)
            if update:
                raise ValueError("More than one updater artifact for a target")
            update = {"url": f"https://github.com/{repo}/releases/download/v{release_version}/{quote(name)}",
                      "signature": signature.read_text().strip()}
    if not update:
        raise ValueError("No updater artifact was built")
    (output / f"update-{platform_key}.json").write_text(json.dumps({"version": release_version, "platforms": {platform_key: update}}, indent=2))
    shutil.copy2(ROOT / "target/dependency-licenses.json", output / f"dependency-licenses-{platform_key}.json")
    (output / f"SHA256SUMS-{platform_key}").write_text("".join(f"{digest(path)}  {path.name}\n" for path in found))


def merge(input_dir, output_dir):
    if output_dir.resolve().is_relative_to(input_dir.resolve()):
        raise ValueError("Release output must not be inside the input artifacts directory")
    manifests = [json.loads(path.read_text()) for path in input_dir.rglob("update-*.json")]
    versions = {manifest["version"] for manifest in manifests}
    if len(versions) != 1:
        raise ValueError("Release artifact versions differ or no artifacts exist")
    platforms = {}
    for manifest in manifests:
        for key, value in manifest["platforms"].items():
            if key in platforms or not value.get("signature") or not value.get("url", "").startswith("https://github.com/"):
                raise ValueError("Duplicate platform or missing update signature")
            platforms[key] = value
    if set(platforms) != {value[0] for value in TARGETS.values()}:
        raise ValueError("A release must contain all four supported update targets")
    output_dir.mkdir(parents=True, exist_ok=True)
    for source in input_dir.rglob("*"):
        if source.is_file() and not source.name.startswith("update-"):
            target = output_dir / source.name
            if target.exists():
                raise ValueError(f"Duplicate release filename: {target.name}")
            shutil.copy2(source, target)
    for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
        shutil.copy2(ROOT / name, output_dir / name)
    (output_dir / "latest.json").write_text(json.dumps({"version": versions.pop(), "platforms": platforms}, indent=2))
    (output_dir / "SHA256SUMS").write_text("".join(f"{digest(path)}  {path.name}\n" for path in sorted(output_dir.iterdir()) if path.is_file() and path.name != "SHA256SUMS"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["configure", "collect", "merge"])
    parser.add_argument("--target", choices=TARGETS)
    parser.add_argument("--public", action="store_true")
    parser.add_argument("--os-signing", action="store_true", help="Require paid macOS/Windows publisher signing (public builds only)")
    parser.add_argument("--input", type=Path, default=ROOT / "dist/artifacts")
    parser.add_argument("--output", type=Path, default=ROOT / "dist/release")
    args = parser.parse_args()
    if args.action == "merge":
        merge(args.input, args.output)
    elif not args.target:
        parser.error("--target is required")
    elif args.action == "configure":
        configure(args.target, args.public, args.os_signing)
    else:
        collect(args.target)


if __name__ == "__main__":
    main()
