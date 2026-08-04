import unittest

import numpy as np

import accuracy


class AccuracyTest(unittest.TestCase):
    def test_hyperspherical_round_trip(self) -> None:
        vectors = np.array(
            [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 1.0, 1.0],
            ],
            dtype=np.float32,
        )
        vectors /= np.linalg.norm(vectors, axis=1, keepdims=True)

        angles = accuracy.to_hyperspherical(vectors)
        reconstructed = accuracy.from_hyperspherical(angles)

        np.testing.assert_allclose(reconstructed, vectors, atol=1e-6)

    def test_scalar_codec_reconstructs_bin_means(self) -> None:
        calibration = np.array(
            [[-4.0], [-3.0], [-2.0], [-1.0], [1.0], [2.0], [3.0], [4.0]],
            dtype=np.float32,
        )
        codec = accuracy.fit_scalar_codec(calibration, bins=2)
        codes = accuracy.encode_scalar(calibration, codec)
        reconstructed = accuracy.decode_scalar(codes, codec)

        np.testing.assert_allclose(reconstructed[:4], -2.5, atol=1e-6)
        np.testing.assert_allclose(reconstructed[4:], 2.5, atol=1e-6)

    def test_recall_at_k_uses_exact_neighbor_ids(self) -> None:
        exact = np.array([[3, 2, 1], [4, 5, 6]], dtype=np.int64)
        approximate = np.array([[3, 9, 1], [6, 5, 4]], dtype=np.int64)

        self.assertAlmostEqual(accuracy.recall_at_k(exact, approximate, 3), 5.0 / 6.0)

    def test_correlated_dataset_uses_supplied_projection(self) -> None:
        projection = np.array(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], dtype=np.float32
        )
        vectors = accuracy.make_dataset(
            "correlated",
            count=32,
            dimension=3,
            random=np.random.default_rng(7),
            correlated_projection=projection,
            noise_std=0.0,
        )

        np.testing.assert_allclose(vectors[:, 2], 0.0, atol=1e-7)

    def test_approximate_scores_keep_query_high_precision(self) -> None:
        queries = np.array([[0.6, 0.8]], dtype=np.float32)
        decoded_corpus = np.array([[1.0, 0.0], [0.0, 1.0]], dtype=np.float32)

        scores = accuracy.score_decoded_corpus(queries, decoded_corpus)

        np.testing.assert_allclose(scores, [[0.6, 0.8]], atol=1e-7)


if __name__ == "__main__":
    unittest.main()
