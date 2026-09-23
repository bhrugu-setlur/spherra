import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parent))
from compare_search import residual_pages


@unittest.skipUnless(sys.platform == 'darwin', 'macOS cache control')
class ResidualCacheTests(unittest.TestCase):
    def test_eviction_is_verified_and_preserves_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'scratch.residual'
            data = os.urandom(1024 * 1024 + 1)
            path.write_bytes(data)
            with path.open('rb') as stream:
                os.fsync(stream.fileno())
                stream.read()
            before = residual_pages([path])
            self.assertGreater(before['resident'], 0)
            cold = residual_pages([path], evict=True)
            self.assertEqual(cold['resident'], 0)
            self.assertEqual(cold['bytes'], len(data))
            self.assertEqual(hashlib.sha256(path.read_bytes()).digest(), hashlib.sha256(data).digest())

    def test_empty_file_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'empty.residual'
            path.touch()
            with self.assertRaises(ValueError):
                residual_pages([path], evict=True)


if __name__ == '__main__':
    unittest.main()
