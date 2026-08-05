use core::cell::RefCell;
use core::ops::Range;

use spherra_codec::{
    CodecError, Pq96Code, Pq96Codebook, PreparedCandidate, PreparedQuery, PrimaryCodes,
    PrimaryScore, ResidualCodes, rerank_candidates, scan_primary,
};
use spherra_domain::DIMENSION;

struct SpyPrimary;

impl PrimaryCodes for SpyPrimary {
    fn scan_primary(
        &self,
        rows: Range<u32>,
        _: &PreparedQuery,
        out: &mut [PrimaryScore],
    ) -> Result<usize, CodecError> {
        let row_count = usize::try_from(rows.end - rows.start)
            .expect("the test range fits in usize")
            .min(out.len());

        for (offset, score) in out.iter_mut().take(row_count).enumerate() {
            *score = PrimaryScore::for_row(rows.start + offset as u32);
        }

        Ok(row_count)
    }
}

struct OverfilledPrimary;

impl PrimaryCodes for OverfilledPrimary {
    fn scan_primary(
        &self,
        _: Range<u32>,
        _: &PreparedQuery,
        out: &mut [PrimaryScore],
    ) -> Result<usize, CodecError> {
        Ok(out.len() + 1)
    }
}

struct MissingPrimary;

impl PrimaryCodes for MissingPrimary {
    fn scan_primary(
        &self,
        _: Range<u32>,
        _: &PreparedQuery,
        _: &mut [PrimaryScore],
    ) -> Result<usize, CodecError> {
        Ok(0)
    }
}

struct UnexpectedPrimary;

impl PrimaryCodes for UnexpectedPrimary {
    fn scan_primary(
        &self,
        rows: Range<u32>,
        _: &PreparedQuery,
        out: &mut [PrimaryScore],
    ) -> Result<usize, CodecError> {
        out[0] = PrimaryScore::for_row(rows.start + 1);
        Ok(1)
    }
}

struct SpyResidual {
    loads: RefCell<Vec<u32>>,
    code: Pq96Code,
}

impl SpyResidual {
    fn new(code: Pq96Code) -> Self {
        Self {
            loads: RefCell::new(Vec::new()),
            code,
        }
    }

    fn loaded_rows(&self) -> Vec<u32> {
        self.loads.borrow().clone()
    }
}

impl ResidualCodes for SpyResidual {
    fn load_residual(&self, row: u32) -> Result<Pq96Code, CodecError> {
        self.loads.borrow_mut().push(row);
        Ok(self.code)
    }
}

fn calibration_residuals() -> Vec<[f32; DIMENSION]> {
    (0..Pq96Code::CENTROIDS)
        .map(|row| {
            core::array::from_fn(|coordinate| {
                row as f32 + (coordinate % Pq96Code::SUBVECTOR_DIMENSION) as f32 * 0.01
            })
        })
        .collect()
}

fn prepared_query() -> PreparedQuery {
    let mut transformed = [0.0; DIMENSION];
    transformed[0] = 1.0;
    PreparedQuery::from_transformed(transformed).expect("the test query is finite")
}

#[test]
fn primary_scan_cannot_load_residual_codes() {
    let primary = SpyPrimary;
    let residual = SpyResidual::new(Pq96Code::from_bytes([0; Pq96Code::BYTE_LEN]));
    let query = prepared_query();
    let mut scores = [PrimaryScore::for_row(0); 3];

    let written = scan_primary(&primary, 4..7, &query, &mut scores)
        .expect("the spy primary source writes the requested rows");

    assert_eq!(written, 3);
    assert_eq!(scores.map(PrimaryScore::row), [4, 5, 6]);
    assert!(residual.loaded_rows().is_empty());
}

#[test]
fn primary_scan_rejects_reversed_ranges() {
    let query = prepared_query();
    let mut scores = [PrimaryScore::for_row(0); 3];
    let start = 7;
    let end = 4;

    assert!(matches!(
        scan_primary(&SpyPrimary, start..end, &query, &mut scores),
        Err(CodecError::InvalidRowRange { start: 7, end: 4 })
    ));
}

#[test]
fn primary_scan_rejects_overfilled_output() {
    let query = prepared_query();
    let mut scores = [PrimaryScore::for_row(0); 1];

    assert!(matches!(
        scan_primary(&OverfilledPrimary, 4..5, &query, &mut scores),
        Err(CodecError::PrimarySourceOverfilled {
            requested_rows: 1,
            output_capacity: 1,
            written: 2,
        })
    ));
}

#[test]
fn candidate_rerank_loads_one_residual_for_each_candidate() {
    let primary = SpyPrimary;
    let codebook =
        Pq96Codebook::train(&calibration_residuals(), 19).expect("calibration residuals are valid");
    let residual_code = Pq96Code::from_bytes([7; Pq96Code::BYTE_LEN]);
    let expected_residual = codebook.decode(&residual_code);
    let residual = SpyResidual::new(residual_code);
    let query = prepared_query();
    let rows = [2, 7, 9];
    let mut reranked = core::array::from_fn(|_| {
        codebook.prepare_candidate(
            PrimaryScore::for_row(0),
            Pq96Code::from_bytes([0; Pq96Code::BYTE_LEN]),
        )
    });

    let written = rerank_candidates(&primary, &residual, &codebook, &query, &rows, &mut reranked)
        .expect("the spies support every candidate row");

    assert_eq!(written, rows.len());
    assert_eq!(reranked.each_ref().map(PreparedCandidate::row), rows);
    for candidate in &reranked {
        assert_eq!(candidate.decoded_residual(), &expected_residual);
    }
    assert_eq!(residual.loaded_rows(), rows);
}

#[test]
fn candidate_rerank_rejects_missing_and_unexpected_primary_rows() {
    let codebook =
        Pq96Codebook::train(&calibration_residuals(), 23).expect("calibration residuals are valid");
    let residual = SpyResidual::new(Pq96Code::from_bytes([0; Pq96Code::BYTE_LEN]));
    let query = prepared_query();
    let mut reranked = [codebook.prepare_candidate(
        PrimaryScore::for_row(0),
        Pq96Code::from_bytes([0; Pq96Code::BYTE_LEN]),
    )];

    assert!(matches!(
        rerank_candidates(
            &MissingPrimary,
            &residual,
            &codebook,
            &query,
            &[12],
            &mut reranked,
        ),
        Err(CodecError::PrimarySourceDidNotReturnCandidate { row: 12 })
    ));
    assert!(matches!(
        rerank_candidates(
            &UnexpectedPrimary,
            &residual,
            &codebook,
            &query,
            &[12],
            &mut reranked,
        ),
        Err(CodecError::PrimarySourceReturnedUnexpectedRow {
            expected: 12,
            actual: 13,
        })
    ));
    assert!(residual.loaded_rows().is_empty());
}

#[test]
fn candidate_rerank_rejects_u32_max_before_loading_residuals() {
    let codebook =
        Pq96Codebook::train(&calibration_residuals(), 29).expect("calibration residuals are valid");
    let residual = SpyResidual::new(Pq96Code::from_bytes([0; Pq96Code::BYTE_LEN]));
    let query = prepared_query();
    let mut reranked = [codebook.prepare_candidate(
        PrimaryScore::for_row(0),
        Pq96Code::from_bytes([0; Pq96Code::BYTE_LEN]),
    )];

    assert!(matches!(
        rerank_candidates(
            &SpyPrimary,
            &residual,
            &codebook,
            &query,
            &[u32::MAX],
            &mut reranked,
        ),
        Err(CodecError::RowOverflow { row: u32::MAX })
    ));
    assert!(residual.loaded_rows().is_empty());
}
