import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location("configure_signing", Path(__file__).resolve().parents[1] / "scripts/configure-macos-signing.py")
signing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signing)


class SigningCheck(unittest.TestCase):
    def test_preserves_identity_and_requires_an_explicit_choice_when_ambiguous(self):
        first = ("A" * 40, "Apple Development: First")
        second = ("B" * 40, "Apple Development: Second")
        self.assertEqual(signing.select_identity([first]), first)
        self.assertEqual(signing.select_identity([second, first], first[0].lower()), first)
        self.assertEqual(signing.select_identity([first, second], second[1]), second)
        for identities, requested in (([], None), ([first, second], None), ([second], first[0])):
            with self.subTest(identities=identities, requested=requested):
                with self.assertRaises(ValueError):
                    signing.select_identity(identities, requested)
