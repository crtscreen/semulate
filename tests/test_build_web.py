import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("build_web", Path(__file__).resolve().parents[1] / "tools/build_web.py")
build_web = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build_web)


class CatalogueTests(unittest.TestCase):
    def test_special_names_metadata_and_save_exclusion(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            roms = root / "roms"
            roms.mkdir()
            name = "game #1 & space.NES"
            data = b"NES\x1a" + bytes([2, 0, 0xa1, 0xd0]) + bytes(8) + bytes(32768)
            (roms / name).write_bytes(data)
            (roms / "game.sav").write_bytes(b"private save")
            (roms / "catalogue.json").write_text(json.dumps({name: {"title": "Custom title"}}))
            items = build_web.catalogue(roms, root / "dist")
            self.assertEqual(len(items), 1)
            self.assertEqual(items[0]["mapper"], 218)
            self.assertEqual(items[0]["title"], "Custom title")
            self.assertEqual(items[0]["url"], "roms/game%20%231%20%26%20space.NES")
            self.assertEqual((root / "dist" / name).read_bytes(), data)
            self.assertFalse((root / "dist/game.sav").exists())

    def test_bad_header_fails_before_shipping_a_broken_catalogue(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "bad.nes").write_bytes(b"bad")
            with self.assertRaisesRegex(ValueError, "not an iNES"):
                build_web.catalogue(root, root / "dist")

    def test_absent_rom_directory_produces_an_import_only_site(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            self.assertEqual(build_web.catalogue(root / "missing", root / "dist"), [])


if __name__ == "__main__":
    unittest.main()
