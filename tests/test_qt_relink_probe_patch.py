from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from patch_qt_relink_probe import ANCHOR, MARKER, apply_patch


class QtRelinkProbePatchTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)

    def make_source(self, text: str) -> Path:
        root = Path(self.temp.name) / "source"
        source = root / "qtbase" / "src" / "corelib" / "global" / "qlibraryinfo.cpp"
        source.parent.mkdir(parents=True)
        source.write_text(text, encoding="utf-8")
        return root

    def test_inserts_only_exported_marker_and_emits_patch(self) -> None:
        root = self.make_source("prefix\n" + ANCHOR + "suffix\n")
        patch = Path(self.temp.name) / "evidence" / "qlibraryinfo.patch"
        before, after = apply_patch(root, patch)
        source = root / "qtbase" / "src" / "corelib" / "global" / "qlibraryinfo.cpp"
        actual = source.read_text(encoding="utf-8")
        self.assertNotEqual(before, after)
        self.assertEqual(actual.count(ANCHOR), 1)
        self.assertIn(f'return "{MARKER}";', actual)
        self.assertIn('extern "C" Q_CORE_EXPORT', actual)
        self.assertTrue(patch.is_file())
        self.assertIn("padfire_qt_relink_test_marker", patch.read_text(encoding="utf-8"))

    def test_refuses_missing_anchor(self) -> None:
        root = self.make_source("no qVersion definition here\n")
        with self.assertRaisesRegex(RuntimeError, "found 0 occurrences"):
            apply_patch(root)

    def test_refuses_ambiguous_anchor(self) -> None:
        root = self.make_source(ANCHOR + "middle\n" + ANCHOR)
        with self.assertRaisesRegex(RuntimeError, "found 2 occurrences"):
            apply_patch(root)

    def test_refuses_a_repeated_patch(self) -> None:
        root = self.make_source(ANCHOR)
        apply_patch(root)
        with self.assertRaisesRegex(RuntimeError, "already exists"):
            apply_patch(root)


if __name__ == "__main__":
    unittest.main()
