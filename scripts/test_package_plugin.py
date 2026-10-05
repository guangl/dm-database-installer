import hashlib
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

from package_plugin import package, LABELS


class PackageTests(unittest.TestCase):
    def test_platform_assets_and_checksums(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for target, label in LABELS.items():
                archive = root / f"{target}.tar.xz"
                binary = b"test plugin binary"
                with tarfile.open(archive, "w:xz") as tar:
                    entry = tarfile.TarInfo("release/dm-installer")
                    entry.size = len(binary)
                    tar.addfile(entry, io.BytesIO(binary))
                package(archive, target, root / "assets")
                asset = root / "assets" / f"dm-installer-{label}"
                self.assertEqual(asset.read_bytes(), binary)
                self.assertEqual(asset.stat().st_mode & 0o777, 0o755)
                self.assertEqual(Path(str(asset) + ".sha256").read_text(),
                                 f"{hashlib.sha256(binary).hexdigest()}  {asset.name}\n")

    def test_missing_or_duplicate_binary_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for names in [[], ["a/dm-installer", "b/dm-installer"]]:
                archive = root / "invalid.tar.gz"
                with tarfile.open(archive, "w:gz") as tar:
                    for name in names:
                        entry = tarfile.TarInfo(name)
                        entry.size = 1
                        tar.addfile(entry, io.BytesIO(b"x"))
                with self.assertRaises(ValueError):
                    package(archive, next(iter(LABELS)), root / "assets")
                self.assertFalse((root / "assets").exists())


if __name__ == "__main__":
    unittest.main()
