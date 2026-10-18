import contextlib
import io
import tempfile
import unittest
from pathlib import Path
from install import operate

class InstallerTests(unittest.TestCase):
    def test_independent_frontends_and_preservation(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            root = Path(directory)
            build = root / 'build'
            build.mkdir()
            for name in ['egui', 'gtk']:
                (build / f'omarchy-vnc-{name}').write_bytes(b'example binary')
            prefix = root / 'install with spaces'
            operate('install', 'egui', prefix, build)
            self.assertIn('Terminal=true', (prefix / 'share/applications/local.omarchy-vnc.egui.desktop').read_text())
            operate('install', 'gtk', prefix, build)
            operate('install', 'egui', prefix, build)
            operate('uninstall', 'egui', prefix, build)
            self.assertTrue((prefix / 'bin/omarchy-vnc-gtk').exists())
            self.assertTrue((prefix / 'share/icons/hicolor/scalable/apps/omarchy-vnc.svg').exists())
            binary = prefix / 'bin/omarchy-vnc-gtk'
            binary.write_bytes(b'modified')
            with self.assertRaises(ValueError):
                operate('uninstall', 'gtk', prefix, build)
            self.assertEqual(binary.read_bytes(), b'modified')

    def test_unowned_file_is_not_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'build').mkdir()
            (root / 'build/omarchy-vnc-egui').write_bytes(b'new')
            (root / 'prefix/bin').mkdir(parents=True)
            path = root / 'prefix/bin/omarchy-vnc-egui'
            path.write_bytes(b'unrelated')
            with self.assertRaises(ValueError):
                operate('install', 'egui', root / 'prefix', root / 'build')
            self.assertEqual(path.read_bytes(), b'unrelated')

if __name__ == '__main__':
    unittest.main()
