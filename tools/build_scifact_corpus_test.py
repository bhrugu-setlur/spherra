#!/usr/bin/env python3

import unittest

import build_scifact_corpus as builder


class RevisionPinningTests(unittest.TestCase):
    def test_explicit_revisions_are_used_without_resolving_moving_heads(self):
        dataset = "c" * 40
        model = "e" * 40
        self.assertEqual(
            builder.resolve_revisions(dataset, model),
            (dataset, model),
        )


if __name__ == "__main__":
    unittest.main()
