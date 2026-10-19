import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


release = load("release_tools", "release.py")
runtime = load("runtime_tools", "bundle-runtime.py")
mac_signing = load("runtime_signing", "sign-macos-runtime.py")


class ReleaseCheck(unittest.TestCase):
    def test_opt_in_os_signing_does_not_silently_become_unsigned(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(ValueError, "repository"):
                release.configure("aarch64-apple-darwin", True, os_signing=True)

        env = {"SPEAK_RELEASE_REPOSITORY": "publisher/speak", "SPEAK_UPDATER_PUBLIC_KEY": "test-public-key",
               "TAURI_SIGNING_PRIVATE_KEY": "test-key-not-a-real-credential", "APPLE_SIGNING_IDENTITY": "Apple Development: test",
               "APPLE_API_KEY": "test", "APPLE_API_ISSUER": "test", "APPLE_API_KEY_PATH": "/test/key"}
        with patch.dict(os.environ, env, clear=True):
            with self.assertRaisesRegex(ValueError, "Developer ID Application"):
                release.configure("aarch64-apple-darwin", True, os_signing=True)
        with patch.dict(os.environ, {key: value for key, value in env.items() if not key.startswith("APPLE_")}, clear=True):
            with self.assertRaisesRegex(ValueError, "AZURE_SIGNING"):
                release.configure("x86_64-pc-windows-msvc", True, os_signing=True)

    def test_unsigned_public_builds_need_only_repository_and_updater_keys(self):
        env = {"SPEAK_RELEASE_REPOSITORY": "publisher/speak", "SPEAK_UPDATER_PUBLIC_KEY": "test-public-key",
               "TAURI_SIGNING_PRIVATE_KEY": "test-private-key"}
        metadata = json.dumps({"resolve": {"nodes": []}, "packages": []})
        with tempfile.TemporaryDirectory() as temp, patch.object(release, "ROOT", Path(temp)), \
                patch.object(release, "version", return_value="0.1.0"), \
                patch.object(release.subprocess, "check_output", return_value=metadata), \
                patch.dict(os.environ, env, clear=True):
            for target in release.TARGETS:
                with self.subTest(target=target):
                    release.configure(target, True)
                    config = json.loads((Path(temp) / "target/release-config.json").read_text())
                    self.assertTrue(config["bundle"]["createUpdaterArtifacts"])
                    self.assertTrue(config["plugins"]["updater"]["requireSignedVersion"])
                    self.assertEqual(config["plugins"]["updater"]["pubkey"], env["SPEAK_UPDATER_PUBLIC_KEY"])
                    if "apple" in target:
                        self.assertEqual(config["bundle"]["macOS"], {"signingIdentity": "-", "hardenedRuntime": False})
                    self.assertFalse((Path(temp) / "src-tauri/tauri.macos.conf.json").exists())
            del os.environ["TAURI_SIGNING_PRIVATE_KEY"]
            with self.assertRaisesRegex(ValueError, "TAURI_SIGNING_PRIVATE_KEY"):
                release.configure("aarch64-apple-darwin", True)
            with self.assertRaisesRegex(ValueError, "requires --public"):
                release.configure("aarch64-apple-darwin", False, os_signing=True)

    def test_cargo_metadata_is_utf8_even_on_cp1252_windows(self):
        package = {"id": "fixture", "name": "fixture", "version": "0.1.0",
                   "license": "MIT", "repository": "https://example.test/ā"}
        metadata = json.dumps({"resolve": {"nodes": [{"id": "fixture"}]}, "packages": [package]}, ensure_ascii=False).encode("utf-8")
        check_output = release.subprocess.check_output

        def cargo_output(command, **kwargs):
            self.assertEqual(command[:2], ["cargo", "metadata"])
            kwargs.setdefault("encoding", "cp1252")
            return check_output([sys.executable, "-c", f"import sys; sys.stdout.buffer.write({metadata!r})"], **kwargs)

        with tempfile.TemporaryDirectory() as temp, patch.object(release, "ROOT", Path(temp)), \
                patch.object(release, "version", return_value="0.1.0"), \
                patch.object(release.subprocess, "check_output", side_effect=cargo_output):
            release.configure("x86_64-pc-windows-msvc", False)
            licenses = json.loads((Path(temp) / "target/dependency-licenses.json").read_text())
            self.assertEqual(licenses[0]["repository"], package["repository"])

    def test_mac_runtime_signing_is_explicitly_ad_hoc_or_developer_id(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            runtime_dir = root / "src-tauri/runtime/metal"
            runtime_dir.mkdir(parents=True)
            helper = runtime_dir / "llama-server"
            helper.write_bytes(b"\xcf\xfa\xed\xfefixture")
            with patch.object(mac_signing, "__file__", str(root / "scripts/sign-macos-runtime.py")), \
                    patch.object(mac_signing.subprocess, "run") as run:
                with patch.dict(os.environ, {}, clear=True), patch.object(sys, "argv", ["sign-macos-runtime.py", "--ad-hoc"]):
                    mac_signing.main()
                    run.assert_called_once_with(["codesign", "--force", "--sign", "-", str(helper)], check=True)
                run.reset_mock()
                identity = "Developer ID Application: Test"
                with patch.dict(os.environ, {"APPLE_SIGNING_IDENTITY": identity}, clear=True), \
                        patch.object(sys, "argv", ["sign-macos-runtime.py"]):
                    mac_signing.main()
                    run.assert_called_once_with(["codesign", "--force", "--sign", identity,
                                                 "--timestamp", "--options", "runtime", str(helper)], check=True)
                with patch.dict(os.environ, {}, clear=True), patch.object(sys, "argv", ["sign-macos-runtime.py"]):
                    with self.assertRaisesRegex(SystemExit, "Developer ID Application"):
                        mac_signing.main()
    def test_tag_and_repository_are_validated(self):
        with patch.dict(os.environ, {"GITHUB_REF_TYPE": "tag", "GITHUB_REF_NAME": "v999.0.0"}, clear=True):
            with self.assertRaisesRegex(ValueError, "tag"):
                release.version()
        for repo in ("https://github.com/a/b", "../repo", "owner/..", "a/b/c", ""):
            with patch.dict(os.environ, {"SPEAK_RELEASE_REPOSITORY": repo}, clear=True):
                with self.assertRaises(ValueError):
                    release.repository()

    def test_readme_downloads_match_collected_installers_and_versioned_updates(self):
        fixtures = {
            "aarch64-apple-darwin": {"dmg/Speak.dmg": "Speak-macOS-AppleSilicon.dmg",
                                     "macos/Speak.app.tar.gz": "Speak_9.8.7_darwin-aarch64.app.tar.gz"},
            "x86_64-apple-darwin": {"dmg/Speak.dmg": "Speak-macOS-Intel.dmg",
                                   "macos/Speak.app.tar.gz": "Speak_9.8.7_darwin-x86_64.app.tar.gz"},
            "x86_64-pc-windows-msvc": {"nsis/Speak.exe": "Speak-Windows-x64-Setup.exe"},
            "x86_64-unknown-linux-gnu": {"appimage/Speak.AppImage": "Speak-Linux-x64.AppImage",
                                        "deb/Speak.deb": "Speak-Linux-x64.deb", "rpm/Speak.rpm": "Speak-Linux-x64.rpm"},
        }
        readme = (ROOT / "README.md").read_text(encoding="utf-8")

        def sign_or_verify(command, **kwargs):
            if command[:len(release.CLI)] == release.CLI:
                self.assertEqual(command[len(release.CLI):-1], ["signer", "sign", "--app-version", "9.8.7"])
                Path(command[-1] + ".sig").write_text("fixture-signature")
            else:
                self.assertEqual(command[1], "--verify-update-signature")
                self.assertEqual(Path(command[3]).read_text(), "fixture-signature")

        with tempfile.TemporaryDirectory() as temp, patch.object(release, "ROOT", Path(temp)), \
                patch.object(release, "version", return_value="9.8.7"), \
                patch.object(release, "repository", return_value="AnishNavali/speak"), \
                patch.dict(os.environ, {"TAURI_SIGNING_PRIVATE_KEY": "fixture-key"}), \
                patch.object(release.subprocess, "run", side_effect=sign_or_verify):
            root = Path(temp)
            (root / "target").mkdir()
            (root / "target/dependency-licenses.json").write_text("[]")
            for target, assets in fixtures.items():
                with self.subTest(target=target):
                    for path in assets:
                        source = root / "target" / target / "release/bundle" / path
                        source.parent.mkdir(parents=True, exist_ok=True)
                        source.write_bytes(b"installer fixture")
                    release.collect(target)
                    platform = release.TARGETS[target][0]
                    output = root / "dist" / platform
                    checksums = (output / f"SHA256SUMS-{platform}").read_text()
                    for name in assets.values():
                        self.assertEqual((output / name).read_bytes(), b"installer fixture")
                        self.assertIn(f"{release.digest(output / name)}  {name}\n", checksums)
                        if not name.endswith(".app.tar.gz"):
                            self.assertIn(f"https://github.com/AnishNavali/speak/releases/latest/download/{name}", readme)
                    update_name = next(name for name in assets.values() if name.endswith((".app.tar.gz", ".exe", ".AppImage")))
                    manifest = json.loads((output / f"update-{platform}.json").read_text())
                    self.assertEqual(manifest["version"], "9.8.7")
                    self.assertEqual(manifest["platforms"][platform], {
                        "url": f"https://github.com/AnishNavali/speak/releases/download/v9.8.7/{update_name}",
                        "signature": "fixture-signature",
                    })

    def test_complete_platform_manifest_and_checksums(self):
        with tempfile.TemporaryDirectory() as temp:
            input_dir, output = Path(temp) / "input", Path(temp) / "release"
            for index, (platform_key, _) in enumerate(release.TARGETS.values()):
                folder = input_dir / str(index); folder.mkdir(parents=True)
                filename = f"asset-{platform_key}.bin"
                (folder / filename).write_bytes(b"signed artifact fixture")
                data = {"version": "0.1.0", "platforms": {platform_key: {"url": f"https://github.com/publisher/speak/releases/download/v0.1.0/{filename}", "signature": "test-signature"}}}
                (folder / f"update-{index}.json").write_text(json.dumps(data))
            release.merge(input_dir, output)
            self.assertEqual(len(json.loads((output / "latest.json").read_text())["platforms"]), 4)
            self.assertIn("latest.json", (output / "SHA256SUMS").read_text())
            (input_dir / "0/update-0.json").unlink()
            with self.assertRaisesRegex(ValueError, "all four"):
                release.merge(input_dir, Path(temp) / "incomplete")

    def test_runtime_archives_cannot_write_outside_staging(self):
        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp) / "staging"; destination.mkdir()
            archive = io.BytesIO()
            with zipfile.ZipFile(archive, "w") as file:
                file.writestr("../outside", "bad")
            with self.assertRaises(ValueError):
                runtime.extract(archive.getvalue(), "runtime.zip", destination)
            self.assertFalse((Path(temp) / "outside").exists())
            for path in ("C:\\escape", "/escape", "a\\..\\escape"):
                self.assertFalse(runtime.safe_path(path))
