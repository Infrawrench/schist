#!/usr/bin/env python3
"""Exercise the translation audit's command-line success and failure cases."""

import contextlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "check_i18n", Path(__file__).with_name("check-i18n.py"))
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class AuditTests(unittest.TestCase):
    def run_audit(self, english, french, *args):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "data").mkdir()
            (root / "data/plural-categories.json").write_text('{}')
            (root / "locales.tsv").write_text("en\tEn\nfr\tFr\n")
            for tag, value in (("en", english), ("fr", french)):
                catalog = root / "locales" / tag
                catalog.mkdir(parents=True)
                (catalog / "example.lang").write_text(f"example.message = {value}\n")
            output = io.StringIO()
            with patch.object(CHECK, "I18N", root), \
                    patch("sys.argv", ["check-i18n.py", *args]), \
                    contextlib.redirect_stdout(output), \
                    self.assertRaises(SystemExit) as result:
                CHECK.main()
            return result.exception.code, output.getvalue()

    def test_copied_english_prose_fails_strict_audit(self):
        source = "Close this photo and its virtual copies before moving them."
        code, output = self.run_audit(source, source, "--strict-audit")
        self.assertEqual(code, 1)
        self.assertIn("fr: 1 unchanged English sentences: example.message", output)
        self.assertNotIn("en: 1 unchanged", output)

    def test_review_only_audit_remains_nonfatal(self):
        source = "Close this photo and its virtual copies before moving them."
        code, output = self.run_audit(source, source, "--audit")
        self.assertEqual(code, 0)
        self.assertIn("fr: review 1 unchanged English sentences", output)

    def test_translated_prose_passes(self):
        code, _ = self.run_audit(
            "Close this photo and its virtual copies before moving them.",
            "Fermez cette photo et ses copies virtuelles avant de les déplacer.",
            "--strict-audit")
        self.assertEqual(code, 0)

    def test_shared_terms_and_placeholder_notation_pass(self):
        for value in ("JPEG", "Schist Cloud", "{width} × {height} px @ {ppi} ppi · {size}",
                      "{first} {second} {third} {fourth} {fifth} {sixth} {seventh}"):
            with self.subTest(value=value):
                code, _ = self.run_audit(value, value, "--strict-audit")
                self.assertEqual(code, 0)

    def test_structural_errors_still_fail(self):
        code, output = self.run_audit(
            "Save {name}", "Enregistrer", "--strict-audit")
        self.assertEqual(code, 1)
        self.assertIn("placeholders differ from English", output)

    def test_lensfun_name_is_preserved(self):
        code, output = self.run_audit(
            "Import Lensfun XML…", "Importer XML…", "--strict-audit")
        self.assertEqual(code, 1)
        self.assertIn("preserve the product name 'Lensfun'", output)


if __name__ == "__main__":
    unittest.main()
