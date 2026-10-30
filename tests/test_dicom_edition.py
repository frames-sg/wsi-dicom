import importlib.util
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "check-dicom-edition.py"


def load_script():
    spec = importlib.util.spec_from_file_location("check_dicom_edition", SCRIPT)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class DicomEditionTests(unittest.TestCase):
    def setUp(self):
        self.script = load_script()

    def test_pinned_edition_is_the_generator_edition(self):
        pinned = self.script.pinned_edition(self.script.GENERATOR.read_text(encoding="utf-8"))
        self.assertRegex(pinned, r"^\d{4}[a-e]$")

    def test_published_edition_is_read_from_the_part03_title(self):
        head = '<book><title>DICOM PS3.3 2027a - Information Object Definitions</title>'
        self.assertEqual(self.script.published_edition(head), "2027a")

    def test_a_title_without_an_edition_is_an_error(self):
        with self.assertRaises(SystemExit):
            self.script.published_edition("<book><title>Information Object Definitions</title>")

    def test_editions_order_by_year_then_letter(self):
        order = self.script.order
        self.assertLess(order("2026d"), order("2026e"))
        self.assertLess(order("2026e"), order("2027a"))
        with self.assertRaises(SystemExit):
            order("2026f")


if __name__ == "__main__":
    unittest.main()
