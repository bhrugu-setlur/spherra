import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parent))
from compare_search import residual_pages, same_build


class BuildIdentityTests(unittest.TestCase):
    def test_build_timing_roundtrip_does_not_change_identity(self):
        original = dict(corpus_hash='corpus', index_current_blake3='current',
                        source={'seed': 7}, training_rows=344, dirty_worktree=False,
                        build_seconds=0.12345678901234567, build_rows_per_second=123.4)
        rounded = original | dict(build_seconds=0.12345678901234566)
        self.assertTrue(same_build(original, rounded))
        for key, value in [('corpus_hash', 'other'), ('index_current_blake3', 'other'),
                           ('source', {'seed': 8}), ('training_rows', 345),
                           ('dirty_worktree', True)]:
            self.assertFalse(same_build(original, rounded | {key: value}))


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
