import unittest, heapq
from tools.build_msmarco_accuracy_corpus import key, retain, normalized_text


class SamplingTests(unittest.TestCase):
    def test_smallest_hash_reservoir_is_order_independent(self):
        records = [(str(i), "passage " + str(i)) for i in range(100)]
        expected = {
            r[0]
            for r in sorted(
                records, key=lambda r: (key("msmarco-document-sample-v1", r[0]), r[0])
            )[:17]
        }
        for items in [records, list(reversed(records)), records[::2] + records[1::2]]:
            h = []
            for ident, text in items:
                retain(h, 17, ident, text)
            self.assertEqual({r[1] for r in h}, expected)
            self.assertEqual(len(h), 17)

    def test_namespaces_and_ids_determine_sampling(self):
        self.assertEqual(key("documents", "31"), key("documents", "31"))
        self.assertNotEqual(key("documents", "31"), key("queries", "31"))
        self.assertNotEqual(key("documents", "31"), key("documents", "32"))

    def test_duplicate_normalization_preserves_meaningful_differences(self):
        self.assertEqual(normalized_text("  Same\nPASSAGE\ttext "), "same passage text")
        self.assertNotEqual(normalized_text("version1"), normalized_text("version2"))


if __name__ == "__main__":
    unittest.main()
