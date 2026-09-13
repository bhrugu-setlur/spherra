//! The measured codec/format path: train, encode, scan, rerank, and verify.
//!
//! Two rules shape this module.
//!
//! First, **training never sees the rows being measured.** The quantizer and
//! the PQ codebook are trained on the disjoint calibration split, so a recall
//! number here is not the codec grading its own homework.
//!
//! Second, **every score on the measured path is certified.** Scores come from
//! [`BlockCertificate`], not from the raw scorer, and every primary and refined
//! score is checked against the FP64 original-space truth its bounds claim to
//! enclose. A violation is counted and surfaced; the caller exits nonzero
//! rather than publishing a result that quietly failed its own soundness
//! property.

use std::time::Instant;
use std::{fmt, ops::Range};

use spherra_codec::{
    CertificateBlockId, CertificateError, CertificateRow, DirectCode, ExhaustiveBlock,
    FixedPointScorer, Pq96Code, Pq96Codebook, PreparedQuery, PrimaryCodes, PrimaryScore,
    QuantizerTable, ResidualCodes, ScorerError, TiledSoa32, TransformPlan, TransformedDirection,
    build_exhaustive_certificate, normalize_fp64, rerank_candidates, transform,
};
use spherra_domain::{ChunkId, DIMENSION, DocumentId, DomainError, PutSeq, ValidatedVector};
use spherra_format::{
    DIRECTORY_ENTRY_LEN, FormatError, HEADER_LEN, LayoutId, PrimarySegment, RowEntry,
    SegmentHeader, SegmentIdentity, StoredErrorCertificate, encode_primary_segment,
};

use crate::corpus::CorpusSplits;
use crate::exact::{ExactOracle, Neighbor, recall_at, sort_by_score_then_row};
use crate::results::PercentileSummary;

/// Compatibility name for the representation identity now owned by the codec.
pub use spherra_codec::CODEC_ID as M1_CODEC_ID;

pub fn codec_id_hex() -> String {
    hex(&*M1_CODEC_ID)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        use fmt::Write;
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// A resident primary-code source over the measured `TILED_SOA_32` block.
///
/// The Task 5 primary capability identifies rows; the certified fixed-point
/// comparison belongs to the scorer. Reading the code out of the layout is
/// still the point: a row the layout cannot produce must not reach a rerank.
struct ResidentPrimary<'a> {
    layout: &'a TiledSoa32,
}

impl PrimaryCodes for ResidentPrimary<'_> {
    fn scan_primary(
        &self,
        rows: Range<u32>,
        _query: &PreparedQuery,
        out: &mut [PrimaryScore],
    ) -> Result<usize, spherra_codec::CodecError> {
        let mut written = 0;
        for row in rows {
            if written == out.len() || self.layout.code_at(row as usize).is_none() {
                break;
            }
            out[written] = PrimaryScore::for_row(row);
            written += 1;
        }
        Ok(written)
    }
}

/// A candidate-only residual source. It is handed to `rerank_candidates` and
/// nowhere else, which is what keeps residual rows off the primary scan path.
struct CandidateResiduals<'a> {
    codes: &'a [Pq96Code],
}

impl ResidualCodes for CandidateResiduals<'_> {
    fn load_residual(&self, row: u32) -> Result<Pq96Code, spherra_codec::CodecError> {
        self.codes
            .get(row as usize)
            .copied()
            .ok_or(spherra_codec::CodecError::RowOverflow { row })
    }
}

/// One prepared, encoded, and certified corpus, ready to be measured at any
/// candidate budget.
pub struct CodecFormatRun {
    splits: CorpusSplits,
    plan: TransformPlan,
    quantizer: QuantizerTable,
    codebook: Pq96Codebook,
    scorer: FixedPointScorer,
    primary_codes: Vec<DirectCode>,
    residual_codes: Vec<Pq96Code>,
    layout: TiledSoa32,
    oracle: ExactOracle,
    header_bytes: u64,
}

/// What one candidate budget produced, before it is dressed as a result record.
#[derive(Clone, Debug, PartialEq)]
pub struct BudgetOutcome {
    pub candidate_budget: u64,
    pub recall_at_10: f64,
    pub recall_at_100: f64,
    pub primary_scan_vectors_per_second: f64,
    pub residual_reranks_per_second: f64,
    pub primary_bound_violation_count: u64,
    pub refined_bound_violation_count: u64,
    pub primary_bound_width_percentiles: PercentileSummary,
    pub refined_bound_width_percentiles: PercentileSummary,
}

/// The three terms an `epsilon` is built from, so a loose bound can be
/// attributed rather than guessed at.
///
/// `epsilon = transform_dot_term + reconstruction_term + serving_term`, where
/// `reconstruction_term = query_norm_upper * max_reconstruction_l2_error`.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct EpsilonAttribution {
    pub epsilon: f64,
    pub transform_dot_term: f64,
    pub reconstruction_term: f64,
    pub serving_term: f64,
    pub max_reconstruction_l2_error: f64,
    pub query_norm_upper: f64,
}

impl EpsilonAttribution {
    fn of(certificate: spherra_codec::ErrorCertificate) -> Self {
        Self {
            epsilon: certificate.epsilon(),
            transform_dot_term: certificate.eta_transform_dot(),
            reconstruction_term: certificate.query_norm_upper()
                * certificate.max_reconstruction_l2_error(),
            serving_term: certificate.eta_serving_score(),
            max_reconstruction_l2_error: certificate.max_reconstruction_l2_error(),
            query_norm_upper: certificate.query_norm_upper(),
        }
    }
}

/// What certified pruning achieved at one `k`.
///
/// A row is pruned when its certified upper bound falls below the k-th largest
/// certified lower bound: no such row can be in the true top-k. `survivors` is
/// what a certified search would still have to refine, and is the number that
/// decides whether the bound is useful — a prune rate is only its complement.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct PruneOutcome {
    pub k: usize,
    pub row_count: u64,
    pub query_count: u64,
    /// Fraction of rows pruned, one sample per query.
    pub prune_rate: PercentileSummary,
    /// Rows left to refine, one sample per query.
    pub survivors: PercentileSummary,
    /// Times a true top-k row was pruned. Any value above zero means the bound
    /// is unsound, not merely loose, and the result must not be published.
    pub soundness_failures: u64,
    /// Observed `|certified score - FP64 truth|` over every row and query.
    pub observed_primary_error: PercentileSummary,
    pub primary_attribution: EpsilonAttribution,
    pub refined_attribution: EpsilonAttribution,
    /// The same measurement with one certificate per row instead of one per
    /// block. This is the E3 "per-row certificates" lever, measured rather
    /// than assumed: the block certificate uses the worst reconstruction error
    /// in the block for every row in it, so per-row bounds are the obvious
    /// first tightening.
    pub per_row_survivors: PercentileSummary,
    pub per_row_prune_rate: PercentileSummary,
    /// The spread of per-row primary epsilons. If this is narrow, per-row
    /// certificates cannot help, because the block maximum was already close
    /// to the typical row.
    pub per_row_epsilon: PercentileSummary,
}

impl CodecFormatRun {
    /// Trains on the calibration split, encodes the indexed rows, and computes
    /// the exhaustive certificate for the whole block.
    pub fn prepare(splits: CorpusSplits, seed: u64) -> Result<Self, HarnessError> {
        let plan = TransformPlan::from_seed(seed);

        let calibration: Vec<TransformedDirection> = splits
            .calibration()
            .iter()
            .map(|row| transform_row(&plan, row))
            .collect::<Result<_, _>>()?;
        if calibration.is_empty() {
            return Err(HarnessError::EmptyCalibration);
        }

        let quantizer = QuantizerTable::train(
            &calibration
                .iter()
                .map(|direction| *direction.as_array())
                .collect::<Vec<_>>(),
        )
        .map_err(|error| HarnessError::QuantizerTraining(error.to_string()))?;

        let calibration_residuals: Vec<[f32; DIMENSION]> = calibration
            .iter()
            .map(|direction| residual_of(&quantizer, direction))
            .collect();
        let codebook = Pq96Codebook::train(&calibration_residuals, seed)
            .map_err(|error| HarnessError::CodebookTraining(error.to_string()))?;

        let mut primary_codes = Vec::with_capacity(splits.indexed().len());
        let mut residual_codes = Vec::with_capacity(splits.indexed().len());
        for row in splits.indexed() {
            let transformed = transform_row(&plan, row)?;
            let primary = quantizer.encode(&transformed);
            let residual = residual_of(&quantizer, &transformed);
            let residual = codebook
                .encode(&residual)
                .map_err(|error| HarnessError::ResidualEncode(error.to_string()))?;
            primary_codes.push(primary);
            residual_codes.push(residual);
        }

        let layout = TiledSoa32::from_codes(&primary_codes);
        let oracle = ExactOracle::new(splits.indexed())?;
        let scorer = FixedPointScorer::new();
        let header_bytes = measure_header_bytes(&plan, &quantizer, &codebook, &primary_codes)?;

        Ok(Self {
            splits,
            plan,
            quantizer,
            codebook,
            scorer,
            primary_codes,
            residual_codes,
            layout,
            oracle,
            header_bytes,
        })
    }

    pub fn splits(&self) -> &CorpusSplits {
        &self.splits
    }

    pub fn transform_id(&self) -> String {
        hex(self.plan.identity())
    }

    pub fn quantizer_id(&self) -> String {
        hex(self.quantizer.identity())
    }

    pub fn pq_codebook_id(&self) -> String {
        hex(self.codebook.codebook_id())
    }

    pub fn scorer_version(&self) -> u32 {
        self.scorer.metadata().scorer_version()
    }

    pub const fn header_bytes(&self) -> u64 {
        self.header_bytes
    }

    pub fn logical_primary_bytes(&self) -> u64 {
        self.layout.logical_direction_bytes() as u64
    }

    pub fn physical_primary_bytes(&self) -> u64 {
        self.layout.physical_direction_bytes() as u64
    }

    pub fn tail_padding_bytes(&self) -> u64 {
        self.layout.padding_direction_bytes() as u64
    }

    /// Measures how much of the corpus certified bounds can prove out of the
    /// top-`k`, using the same scan the serving path already performs.
    ///
    /// This does not rerank and does not use a candidate budget: the point is
    /// what the certificate alone can establish, before any heuristic is
    /// applied.
    pub fn measure_prune(&self, k: usize) -> Result<PruneOutcome, HarnessError> {
        if k == 0 {
            return Err(HarnessError::ZeroBudget);
        }
        let row_count = u32::try_from(self.primary_codes.len())
            .map_err(|_| HarnessError::BlockTooLarge(self.primary_codes.len()))?;

        let block = ExhaustiveBlock::from_rows(
            CertificateBlockId::from_bytes(*M1_CODEC_ID),
            row_count,
            self.splits
                .indexed()
                .iter()
                .zip(&self.primary_codes)
                .zip(&self.residual_codes)
                .enumerate()
                .map(|(row, ((original, primary), residual))| {
                    CertificateRow::new(row as u32, original, primary, residual)
                }),
        )?;
        let certificate = build_exhaustive_certificate(
            &self.scorer,
            &self.plan,
            &self.quantizer,
            &self.codebook,
            &block,
        )?;

        // One single-row block per row, so each row gets a certificate built
        // from its own reconstruction error rather than the block maximum.
        // Certificates do not depend on the query, so this is built once.
        let mut row_blocks = Vec::with_capacity(row_count as usize);
        for row in 0..row_count {
            let index = row as usize;
            row_blocks.push(ExhaustiveBlock::from_rows(
                CertificateBlockId::from_bytes(*M1_CODEC_ID),
                1,
                std::iter::once(CertificateRow::new(
                    0,
                    &self.splits.indexed()[index],
                    &self.primary_codes[index],
                    &self.residual_codes[index],
                )),
            )?);
        }
        let row_certificates = row_blocks
            .iter()
            .map(|block| {
                build_exhaustive_certificate(
                    &self.scorer,
                    &self.plan,
                    &self.quantizer,
                    &self.codebook,
                    block,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut row_epsilons: Vec<f64> = row_certificates
            .iter()
            .map(|certificate| certificate.primary().epsilon())
            .collect();

        let mut prune_rates = Vec::with_capacity(self.splits.queries().len());
        let mut survivor_counts = Vec::with_capacity(self.splits.queries().len());
        let mut per_row_prune_rates = Vec::with_capacity(self.splits.queries().len());
        let mut per_row_survivor_counts = Vec::with_capacity(self.splits.queries().len());
        let mut observed_errors = Vec::new();
        let mut soundness_failures = 0_u64;

        for query in self.splits.queries() {
            let prepared =
                self.scorer
                    .prepare_query(&self.plan, query, &self.quantizer, &self.codebook)?;
            let normalized_query = normalize_fp64(query)?;

            let mut lowers = Vec::with_capacity(row_count as usize);
            let mut uppers = Vec::with_capacity(row_count as usize);
            let mut row_lowers = Vec::with_capacity(row_count as usize);
            let mut row_uppers = Vec::with_capacity(row_count as usize);
            for row in 0..row_count {
                let candidate = block.candidate(row)?;
                let score = certificate.score_primary(&self.scorer, &prepared, &candidate)?;
                let estimate = score.as_f64();
                let bounds = certificate.primary_bounds(score)?;
                let truth = self
                    .oracle
                    .true_score(&normalized_query, row as usize)
                    .ok_or(HarnessError::MissingOracleRow { row })?;
                observed_errors.push((estimate - truth).abs());
                lowers.push(bounds.lower);
                uppers.push(bounds.upper);

                let row_certificate = &row_certificates[row as usize];
                let row_candidate = row_blocks[row as usize].candidate(0)?;
                let row_score =
                    row_certificate.score_primary(&self.scorer, &prepared, &row_candidate)?;
                let row_bounds = row_certificate.primary_bounds(row_score)?;
                row_lowers.push(row_bounds.lower);
                row_uppers.push(row_bounds.upper);
            }

            // The k-th largest lower bound. Nothing certified below it can
            // displace the k rows that are certified at or above it.
            let mut sorted_lowers = lowers.clone();
            sorted_lowers.sort_by(|left, right| right.total_cmp(left));
            let threshold = sorted_lowers[(k - 1).min(sorted_lowers.len() - 1)];

            let survivors = uppers.iter().filter(|upper| **upper >= threshold).count();
            survivor_counts.push(survivors as f64);
            prune_rates.push((row_count as f64 - survivors as f64) / row_count as f64);

            let mut sorted_row_lowers = row_lowers.clone();
            sorted_row_lowers.sort_by(|left, right| right.total_cmp(left));
            let row_threshold = sorted_row_lowers[(k - 1).min(sorted_row_lowers.len() - 1)];
            let row_survivors = row_uppers
                .iter()
                .filter(|upper| **upper >= row_threshold)
                .count();
            per_row_survivor_counts.push(row_survivors as f64);
            per_row_prune_rates.push((row_count as f64 - row_survivors as f64) / row_count as f64);

            for neighbor in self.oracle.top_k_normalized(&normalized_query, k) {
                if uppers[neighbor.row as usize] < threshold {
                    soundness_failures += 1;
                }
            }
        }

        Ok(PruneOutcome {
            k,
            row_count: u64::from(row_count),
            query_count: self.splits.queries().len() as u64,
            prune_rate: PercentileSummary::from_samples(&mut prune_rates),
            survivors: PercentileSummary::from_samples(&mut survivor_counts),
            soundness_failures,
            observed_primary_error: PercentileSummary::from_samples(&mut observed_errors),
            primary_attribution: EpsilonAttribution::of(certificate.primary()),
            refined_attribution: EpsilonAttribution::of(certificate.refined()),
            per_row_survivors: PercentileSummary::from_samples(&mut per_row_survivor_counts),
            per_row_prune_rate: PercentileSummary::from_samples(&mut per_row_prune_rates),
            per_row_epsilon: PercentileSummary::from_samples(&mut row_epsilons),
        })
    }

    /// Scans every query once against the whole block, then reranks at each
    /// requested budget.
    ///
    /// The primary scan does not depend on the budget, so it is measured once
    /// and reported identically for each entry; only the rerank stage is
    /// re-run per budget.
    pub fn measure(&self, budgets: &[u64]) -> Result<Vec<BudgetOutcome>, HarnessError> {
        if budgets.is_empty() {
            return Err(HarnessError::NoBudgets);
        }
        let row_count = u32::try_from(self.primary_codes.len())
            .map_err(|_| HarnessError::BlockTooLarge(self.primary_codes.len()))?;

        let block = ExhaustiveBlock::from_rows(
            CertificateBlockId::from_bytes(*M1_CODEC_ID),
            row_count,
            self.splits
                .indexed()
                .iter()
                .zip(&self.primary_codes)
                .zip(&self.residual_codes)
                .enumerate()
                .map(|(row, ((original, primary), residual))| {
                    CertificateRow::new(row as u32, original, primary, residual)
                }),
        )?;
        let certificate = build_exhaustive_certificate(
            &self.scorer,
            &self.plan,
            &self.quantizer,
            &self.codebook,
            &block,
        )?;

        let mut primary_widths = Vec::new();
        let mut primary_violations = 0_u64;
        let mut scanned_rows = 0_u64;
        let mut scan_seconds = 0.0_f64;

        // Per query: the certified primary ranking, retained so that every
        // budget reranks the same scan rather than re-scanning.
        let mut rankings: Vec<Vec<Neighbor>> = Vec::with_capacity(self.splits.queries().len());
        let mut prepared_queries = Vec::with_capacity(self.splits.queries().len());
        let mut normalized_queries = Vec::with_capacity(self.splits.queries().len());
        let mut exact_neighbors = Vec::with_capacity(self.splits.queries().len());

        for query in self.splits.queries() {
            let prepared =
                self.scorer
                    .prepare_query(&self.plan, query, &self.quantizer, &self.codebook)?;
            let normalized_query = normalize_fp64(query)?;
            let exact = self.oracle.top_k_normalized(&normalized_query, 100);

            let mut ranked = Vec::with_capacity(row_count as usize);
            let mut serving_scores = Vec::with_capacity(row_count as usize);
            let started = Instant::now();
            for row in 0..row_count {
                let primary = self
                    .layout
                    .code_at(row as usize)
                    .ok_or(HarnessError::MissingPrimaryRow { row })?;
                serving_scores.push(self.scorer.score_primary(&prepared, &primary));
            }
            scan_seconds += started.elapsed().as_secs_f64();
            scanned_rows += serving_scores.len() as u64;

            for (row, serving_score) in (0..row_count).zip(serving_scores) {
                let candidate = block.candidate(row)?;
                let score = certificate.score_primary(&self.scorer, &prepared, &candidate)?;
                if score.raw() != serving_score.raw() {
                    return Err(HarnessError::ServingScoreMismatch {
                        row,
                        kind: "primary",
                    });
                }
                ranked.push(Neighbor {
                    row,
                    score: score.as_f64(),
                });
                let bounds = certificate.primary_bounds(score)?;
                let truth = self
                    .oracle
                    .true_score(&normalized_query, row as usize)
                    .ok_or(HarnessError::MissingOracleRow { row })?;
                if truth < bounds.lower || truth > bounds.upper {
                    primary_violations += 1;
                }
                primary_widths.push(bounds.upper - bounds.lower);
            }

            sort_by_score_then_row(&mut ranked);
            rankings.push(ranked);
            prepared_queries.push(prepared);
            normalized_queries.push(normalized_query);
            exact_neighbors.push(exact);
        }

        let primary_scan_vectors_per_second = throughput(scanned_rows, scan_seconds);
        let primary_bound_width_percentiles = PercentileSummary::from_samples(&mut primary_widths);

        let mut outcomes = Vec::with_capacity(budgets.len());
        for budget in budgets.iter().copied() {
            let budget_rows = usize::try_from(budget)
                .unwrap_or(usize::MAX)
                .min(row_count as usize);
            if budget_rows == 0 {
                return Err(HarnessError::ZeroBudget);
            }

            let mut refined_widths = Vec::new();
            let mut refined_violations = 0_u64;
            let mut reranked = 0_u64;
            let mut rerank_seconds = 0.0_f64;
            let mut recall_10 = 0.0_f64;
            let mut recall_100 = 0.0_f64;

            for (index, ranking) in rankings.iter().enumerate() {
                let prepared = &prepared_queries[index];
                let normalized_query = &normalized_queries[index];
                let candidate_rows: Vec<u32> =
                    ranking.iter().take(budget_rows).map(|n| n.row).collect();

                let primary_source = ResidentPrimary {
                    layout: &self.layout,
                };
                let residual_source = CandidateResiduals {
                    codes: &self.residual_codes,
                };
                let rerank_query = PreparedQuery::from_transformed(*prepared.transformed())
                    .map_err(|error| HarnessError::ResidualEncode(error.to_string()))?;
                let mut prepared_candidates =
                    vec![
                        self.codebook
                            .prepare_candidate(PrimaryScore::for_row(0), self.residual_codes[0]);
                        candidate_rows.len()
                    ];
                let mut serving_scores = Vec::with_capacity(candidate_rows.len());

                let started = Instant::now();
                let written = rerank_candidates(
                    &primary_source,
                    &residual_source,
                    &self.codebook,
                    &rerank_query,
                    &candidate_rows,
                    &mut prepared_candidates,
                )
                .map_err(|error| HarnessError::Rerank(error.to_string()))?;
                for prepared_candidate in prepared_candidates.iter().take(written) {
                    let row = prepared_candidate.row();
                    let primary = self
                        .layout
                        .code_at(row as usize)
                        .ok_or(HarnessError::MissingPrimaryRow { row })?;
                    serving_scores.push(self.scorer.score_prepared_candidate(
                        prepared,
                        &primary,
                        prepared_candidate,
                    )?);
                }
                rerank_seconds += started.elapsed().as_secs_f64();
                reranked += written as u64;

                let mut refined: Vec<Neighbor> = Vec::with_capacity(written);
                for (prepared_candidate, serving_score) in
                    prepared_candidates.iter().take(written).zip(serving_scores)
                {
                    let candidate = block.candidate(prepared_candidate.row())?;
                    let score = certificate.score_refined(
                        &self.scorer,
                        prepared,
                        &candidate,
                        prepared_candidate,
                    )?;
                    if score.raw() != serving_score.raw() {
                        return Err(HarnessError::ServingScoreMismatch {
                            row: prepared_candidate.row(),
                            kind: "refined",
                        });
                    }
                    refined.push(Neighbor {
                        row: prepared_candidate.row(),
                        score: score.as_f64(),
                    });
                    let bounds = certificate.refined_bounds(score)?;
                    let truth = self
                        .oracle
                        .true_score(normalized_query, prepared_candidate.row() as usize)
                        .ok_or(HarnessError::MissingOracleRow {
                            row: prepared_candidate.row(),
                        })?;
                    if truth < bounds.lower || truth > bounds.upper {
                        refined_violations += 1;
                    }
                    refined_widths.push(bounds.upper - bounds.lower);
                }

                sort_by_score_then_row(&mut refined);
                recall_10 += recall_at(&exact_neighbors[index], &refined, 10);
                recall_100 += recall_at(&exact_neighbors[index], &refined, 100);
            }

            let queries = rankings.len() as f64;
            outcomes.push(BudgetOutcome {
                candidate_budget: budget,
                recall_at_10: recall_10 / queries,
                recall_at_100: recall_100 / queries,
                primary_scan_vectors_per_second,
                residual_reranks_per_second: throughput(reranked, rerank_seconds),
                primary_bound_violation_count: primary_violations,
                refined_bound_violation_count: refined_violations,
                primary_bound_width_percentiles,
                refined_bound_width_percentiles: PercentileSummary::from_samples(
                    &mut refined_widths,
                ),
            });
        }

        Ok(outcomes)
    }
}

fn throughput(items: u64, seconds: f64) -> f64 {
    if seconds > 0.0 {
        items as f64 / seconds
    } else {
        0.0
    }
}

/// Validates and normalizes the raw FP32 row exactly once, then transforms it
/// through the public ingest path.
fn transform_row(
    plan: &TransformPlan,
    row: &[f32; DIMENSION],
) -> Result<TransformedDirection, HarnessError> {
    let validated = ValidatedVector::new(row.to_vec())?;
    let direction = validated
        .normalized_direction()
        .ok_or(HarnessError::UnreliableDirection)?;
    Ok(transform(plan, direction))
}

fn residual_of(quantizer: &QuantizerTable, direction: &TransformedDirection) -> [f32; DIMENSION] {
    let reconstruction = quantizer.decode(&quantizer.encode(direction));
    let values = direction.as_array();
    std::array::from_fn(|coordinate| values[coordinate] - reconstruction[coordinate])
}

/// Encodes a real primary segment and reads back what the durable header and
/// its checked section directory actually cost.
fn measure_header_bytes(
    plan: &TransformPlan,
    quantizer: &QuantizerTable,
    codebook: &Pq96Codebook,
    primary_codes: &[DirectCode],
) -> Result<u64, HarnessError> {
    let identity = SegmentIdentity {
        collection_id: [0; 16],
        segment_id: [0; 16],
        codec_id: *M1_CODEC_ID,
        scorer_version: FixedPointScorer::new().metadata().scorer_version(),
        transform_id: *plan.identity(),
        quantizer_id: *quantizer.identity(),
        pq_codebook_id: *codebook.codebook_id(),
        layout: LayoutId::TiledSoa32,
    };
    let zero_certificate = StoredErrorCertificate {
        max_reconstruction_l2_error: 0.0,
        eta_transform_dot: 0.0,
        query_norm_upper: 0.0,
        eta_serving_score: 0.0,
        epsilon: 0.0,
    };
    let segment = PrimarySegment {
        identity,
        rows: (0..primary_codes.len())
            .map(|row| {
                Ok(RowEntry {
                    chunk_id: ChunkId::from_u128(row as u128),
                    document_id: DocumentId::from_u128(row as u128),
                    put_seq: PutSeq::new(0, row as u64)?,
                })
            })
            .collect::<Result<Vec<_>, DomainError>>()?,
        radius_flags: vec![[0; 4]; primary_codes.len()],
        primary_codes: primary_codes.iter().map(|code| *code.as_bytes()).collect(),
        quantizer_table: quantizer.centers().to_vec(),
        primary_certificate: zero_certificate,
        refined_certificate: zero_certificate,
    };

    let bytes = encode_primary_segment(&segment)?;
    let mut header = [0_u8; HEADER_LEN];
    header.copy_from_slice(bytes.get(..HEADER_LEN).ok_or(HarnessError::ShortSegment)?);
    let header = SegmentHeader::decode(&header)?;
    Ok(HEADER_LEN as u64 + u64::from(header.section_count) * DIRECTORY_ENTRY_LEN as u64)
}

#[derive(Debug)]
pub enum HarnessError {
    EmptyCalibration,
    NoBudgets,
    ZeroBudget,
    BlockTooLarge(usize),
    ShortSegment,
    UnreliableDirection,
    MissingPrimaryRow { row: u32 },
    MissingOracleRow { row: u32 },
    ServingScoreMismatch { row: u32, kind: &'static str },
    QuantizerTraining(String),
    CodebookTraining(String),
    ResidualEncode(String),
    Rerank(String),
    Domain(DomainError),
    Scorer(ScorerError),
    Certificate(CertificateError),
    Format(FormatError),
}

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCalibration => {
                write!(formatter, "the calibration split contains no rows")
            }
            Self::NoBudgets => write!(formatter, "at least one candidate budget is required"),
            Self::ZeroBudget => write!(formatter, "a candidate budget of zero measures nothing"),
            Self::BlockTooLarge(rows) => {
                write!(
                    formatter,
                    "a block of {rows} rows exceeds the u32 row space"
                )
            }
            Self::ShortSegment => write!(formatter, "the encoded segment is shorter than a header"),
            Self::UnreliableDirection => write!(
                formatter,
                "a corpus row normalizes to an unreliable direction",
            ),
            Self::MissingPrimaryRow { row } => {
                write!(formatter, "TILED_SOA_32 has no primary row {row}")
            }
            Self::MissingOracleRow { row } => {
                write!(formatter, "the exact oracle has no row {row}")
            }
            Self::ServingScoreMismatch { row, kind } => write!(
                formatter,
                "{kind} TILED_SOA_32 score for row {row} disagrees with the certified block",
            ),
            Self::QuantizerTraining(message) => {
                write!(formatter, "quantizer training failed: {message}")
            }
            Self::CodebookTraining(message) => {
                write!(formatter, "PQ codebook training failed: {message}")
            }
            Self::ResidualEncode(message) => {
                write!(formatter, "residual encoding failed: {message}")
            }
            Self::Rerank(message) => write!(formatter, "candidate rerank failed: {message}"),
            Self::Domain(error) => write!(formatter, "{error}"),
            Self::Scorer(error) => write!(formatter, "{error}"),
            Self::Certificate(error) => write!(formatter, "{error}"),
            Self::Format(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for HarnessError {}

impl From<DomainError> for HarnessError {
    fn from(error: DomainError) -> Self {
        Self::Domain(error)
    }
}

impl From<ScorerError> for HarnessError {
    fn from(error: ScorerError) -> Self {
        Self::Scorer(error)
    }
}

impl From<CertificateError> for HarnessError {
    fn from(error: CertificateError) -> Self {
        Self::Certificate(error)
    }
}

impl From<FormatError> for HarnessError {
    fn from(error: FormatError) -> Self {
        Self::Format(error)
    }
}

#[cfg(test)]
mod tests {
    use core::array;

    use spherra_domain::{DIMENSION, ValidatedVector};

    use spherra_codec::{DirectCode, TiledSoa32};

    use crate::corpus::CorpusDescriptor;

    use super::{CodecFormatRun, HarnessError, TransformPlan, transform, transform_row};

    #[test]
    fn corpus_rows_follow_the_single_normalization_ingest_path() {
        let plan = TransformPlan::from_seed(0x69_6e_67_65_73_74);
        let mut state = 0x1234_5678_u64;
        let raw = (0..128)
            .find_map(|seed| {
                let values: [f64; DIMENSION] = array::from_fn(|coordinate| {
                    state = state
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    let fraction = ((state >> 40) as u32) as f64 / (1_u32 << 24) as f64;
                    let exponent = ((coordinate * 37 + seed * 19) % 151) as i32 - 75;
                    fraction.mul_add(2.0, -1.0) * 2.0_f64.powi(exponent)
                });
                let norm = values
                    .iter()
                    .fold(0.0, |sum, value| value.mul_add(*value, sum))
                    .sqrt();
                let raw = array::from_fn(|coordinate| (values[coordinate] / norm) as f32);
                let first = ValidatedVector::new(raw.to_vec()).ok()?;
                let first = first.normalized_direction()?.as_array();
                let second = ValidatedVector::new(first.to_vec()).ok()?;
                (first != second.normalized_direction()?.as_array()).then_some(raw)
            })
            .expect("fixture search finds a vector whose second normalization drifts");
        let validated = ValidatedVector::new(raw.to_vec()).expect("finite 768D fixture");
        let expected = transform(
            &plan,
            validated
                .normalized_direction()
                .expect("fixture has a reliable direction"),
        );

        assert_eq!(
            transform_row(&plan, &raw).expect("fixture transforms"),
            expected,
            "the corpus row was normalized more than once",
        );
    }

    #[test]
    fn measurement_scores_the_selected_tiled_layout() {
        let splits = CorpusDescriptor::resolve("generated-gaussian-768x256")
            .expect("generated descriptor")
            .load(17, 1)
            .expect("small disjoint corpus");
        let mut run = CodecFormatRun::prepare(splits, 17).expect("small run prepares");
        let replacement = DirectCode::from_nibbles([0; DIMENSION]).expect("four-bit code");
        run.layout = TiledSoa32::from_codes(&vec![replacement; run.primary_codes.len()]);

        assert!(matches!(
            run.measure(&[1]),
            Err(HarnessError::ServingScoreMismatch { .. })
        ));
    }
}
