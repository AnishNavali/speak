"""Stage verified, isolated llama.cpp runtimes for an explicit Rust target."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = json.loads((ROOT / "src-tauri/runtime-manifest.json").read_text())


def safe_path(name):
    return not name.startswith(("/", "\\")) and ":" not in name and ".." not in name.replace("\\", "/").split("/")


def extract(data, name, destination):
    if name.endswith(".zip"):
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            for entry in archive.infolist():
                if not safe_path(entry.filename) or (entry.external_attr >> 16) & 0o170000 == 0o120000:
                    raise ValueError("Unsafe runtime archive path or symlink")
            archive.extractall(destination)
    else:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
            archive.extractall(destination, filter="data")


def copy_payload(extracted, destination, part):
    """Normalize library search paths without mixing different backends."""
    for source in sorted(extracted.rglob("*")):
        if not source.is_file():
            continue
        resolved = source.resolve()
        if not resolved.is_relative_to(extracted.resolve()):
            raise ValueError("Runtime symlink escapes archive")
        name = source.name
        lower = name.lower()
        if any(word in lower for word in ("license", "copying", "notice", "eula")):
            target = destination / "licenses" / str(part) / source.relative_to(extracted)
        elif name in ("llama-server", "llama-server.exe") or lower.endswith((".dll", ".dylib", ".metal", ".metallib")) or ".so" in lower:
            target = destination / name
        else:
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists():
            if hashlib.sha256(target.read_bytes()).digest() != hashlib.sha256(source.read_bytes()).digest():
                raise ValueError(f"Conflicting runtime libraries: {name}")
            continue
        if source.is_symlink() and not name.lower().endswith(".dll"):
            canonical_target = destination / resolved.name
            if not canonical_target.exists():
                shutil.copy2(resolved, canonical_target)
            if target != canonical_target:
                target.symlink_to(resolved.name)
        else:
            shutil.copy2(source, target)


def prepare(target, backend, cache, output):
    assets = MANIFEST["targets"][target][backend]
    with tempfile.TemporaryDirectory(prefix="speak-runtime-") as temp:
        stage = Path(temp)
        normalized = stage / "normalized"
        normalized.mkdir()
        for part, asset in enumerate(assets):
            cached = cache / asset["name"]
            if not cached.exists():
                print(f"Downloading {asset['name']}", flush=True)
                partial = cached.with_suffix(cached.suffix + ".part")
                with urllib.request.urlopen(f"{MANIFEST['base_url']}/{asset['name']}", timeout=120) as response, partial.open("wb") as file:
                    shutil.copyfileobj(response, file)
                partial.replace(cached)
            data = cached.read_bytes()
            if len(data) != asset["size"] or hashlib.sha256(data).hexdigest() != asset["sha256"]:
                raise ValueError(f"Runtime archive checksum mismatch; remove {cached} and retry")
            extracted = stage / str(part)
            extracted.mkdir()
            extract(data, asset["name"], extracted)
            copy_payload(extracted, normalized, part)
        executable = normalized / ("llama-server.exe" if "windows" in target else "llama-server")
        if not executable.is_file():
            raise ValueError("Runtime archive contains no llama-server")
        executable.chmod(0o755)
        (normalized / "VERSION.json").write_text(json.dumps({"target": target, "backend": backend, "version": MANIFEST["version"], "assets": assets}, indent=2))
        destination = output / backend
        # Replace only the backend directory owned by this tool; never overlay stale GPU libraries.
        if destination.exists():
            if not (destination / "VERSION.json").is_file():
                raise ValueError(f"Refusing to replace an unmanaged directory: {destination}")
            shutil.rmtree(destination)
        shutil.copytree(normalized, destination, symlinks=True)
        print(f"Verified {target}/{backend} prepared in {destination}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", help="Rust target triple, not the build host architecture")
    parser.add_argument("--backend", choices=["default", "cpu", "vulkan", "cuda"], default="default")
    args = parser.parse_args()
    target = args.target or subprocess.check_output(["rustc", "--print", "host-tuple"], text=True).strip()
    if target not in MANIFEST["targets"]:
        parser.error(f"Unsupported release target: {target}")
    cache = ROOT / "target/runtime-downloads"
    cache.mkdir(parents=True, exist_ok=True)
    output = ROOT / "src-tauri/runtime"
    output.mkdir(parents=True, exist_ok=True)
    backends = ["metal"] if "apple" in target else ["cpu", "vulkan"]
    if args.backend != "default":
        backends = ["metal" if "apple" in target and args.backend == "cpu" else args.backend]
    for backend in backends:
        if backend not in MANIFEST["targets"][target]:
            parser.error(f"No {backend} runtime for {target}")
        prepare(target, backend, cache, output)


if __name__ == "__main__":
    main()
