use core::cell::RefCell;
use core::ops::Range;

use spherra_codec::{
    CodecError, Pq96Code, Pq96Codebook, PreparedQuery, PrimaryCodes, PrimaryScore, ResidualCodes,
    rerank_candidates, scan_primary,
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
fn candidate_rerank_loads_one_residual_for_each_candidate() {
    let primary = SpyPrimary;
    let codebook =
        Pq96Codebook::train(&calibration_residuals(), 19).expect("calibration residuals are valid");
    let residual = SpyResidual::new(Pq96Code::from_bytes([0; Pq96Code::BYTE_LEN]));
    let query = prepared_query();
    let rows = [2, 7, 9];
    let mut reranked = [PrimaryScore::for_row(0); 3];

    let written = rerank_candidates(&primary, &residual, &codebook, &query, &rows, &mut reranked)
        .expect("the spies support every candidate row");

    assert_eq!(written, rows.len());
    assert_eq!(reranked.map(PrimaryScore::row), rows);
    assert_eq!(residual.loaded_rows(), rows);
}
