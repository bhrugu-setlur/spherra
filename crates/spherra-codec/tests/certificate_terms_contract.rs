use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;
use spherra_codec::{
    CertificateBlockId, CertificateError, CertificateRow, CertificateTerms, DirectCode,
    ExhaustiveBlock, FixedPointScorer, Pq96Code, Pq96Codebook, PrimaryScore, QuantizerTable,
    ScoreKind, TransformPlan, build_exhaustive_certificate, dot_f64, normalize_fp64, transform,
    validate_certificate_terms,
};
use spherra_domain::{DIMENSION, ValidatedVector};
use std::array;
use std::sync::OnceLock;

struct Fixture {
    plan: TransformPlan,
    table: QuantizerTable,
    book: Pq96Codebook,
    rows: Vec<[f32; DIMENSION]>,
    queries: Vec<[f32; DIMENSION]>,
    primary: Vec<DirectCode>,
    residual: Vec<Pq96Code>,
}
fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let mut rng = ChaCha20Rng::seed_from_u64(20260913);
        let rows: Vec<[f32; DIMENSION]> = (0..3276)
            .map(|_| array::from_fn(|_| ((rng.next_u32() >> 8) as f32 / 16_777_216.0) * 2.0 - 1.0))
            .collect();
        let plan = TransformPlan::from_seed(20260804);
        let directions: Vec<_> = rows
            .iter()
            .map(|row| {
                let v = ValidatedVector::new(row.to_vec()).unwrap();
                transform(&plan, v.normalized_direction().unwrap())
            })
            .collect();
        let table = QuantizerTable::train(
            &directions[..256]
                .iter()
                .map(|d| *d.as_array())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let primary: Vec<_> = directions.iter().map(|d| table.encode(d)).collect();
        let residual_vectors: Vec<[f32; DIMENSION]> = directions
            .iter()
            .zip(&primary)
            .map(|(d, code)| {
                let decoded = table.decode(code);
                array::from_fn(|i| d.as_array()[i] - decoded[i])
            })
            .collect();
        let book = Pq96Codebook::train(&residual_vectors[..256], 20260804).unwrap();
        let residual = residual_vectors[256..3256]
            .iter()
            .map(|v| book.encode(v).unwrap())
            .collect();
        Fixture {
            plan,
            table,
            book,
            rows: rows[256..3256].to_vec(),
            queries: rows[3256..].to_vec(),
            primary: primary[256..3256].to_vec(),
            residual,
        }
    })
}
fn block(f: &Fixture, count: usize) -> ExhaustiveBlock<'_> {
    ExhaustiveBlock::from_rows(
        CertificateBlockId::from_bytes([3; 32]),
        count as u32,
        (0..count)
            .map(|i| CertificateRow::new(i as u32, &f.rows[i], &f.primary[i], &f.residual[i])),
    )
    .unwrap()
}

#[test]
fn arithmetic_intersections_equal_block_bounds_and_enclose_3000_rows_over_20_queries() {
    let f = fixture();
    let scorer = FixedPointScorer::new();
    let block = block(f, 3000);
    let cert = build_exhaustive_certificate(&scorer, &f.plan, &f.table, &f.book, &block).unwrap();
    let primary = validate_certificate_terms(cert.primary().terms(), ScoreKind::Primary).unwrap();
    let refined = validate_certificate_terms(cert.refined().terms(), ScoreKind::Refined).unwrap();
    assert_eq!(primary.kind(), ScoreKind::Primary);
    assert_eq!(refined.kind(), ScoreKind::Refined);
    for query in &f.queries {
        let prepared = scorer
            .prepare_query(&f.plan, query, &f.table, &f.book)
            .unwrap();
        let q = normalize_fp64(query).unwrap();
        for i in 0..3000 {
            let candidate = block.candidate(i as u32).unwrap();
            let p = cert.score_primary(&scorer, &prepared, &candidate).unwrap();
            let residual = f
                .book
                .prepare_candidate(PrimaryScore::for_row(i as u32), f.residual[i]);
            let r = cert
                .score_refined(&scorer, &prepared, &candidate, &residual)
                .unwrap();
            let pi = primary.arithmetic_interval(p.raw());
            let ri = refined.arithmetic_interval(r.raw());
            let pb = cert.primary_bounds(p).unwrap();
            let rb = cert.refined_bounds(r).unwrap();
            assert_eq!((pi.lower(), pi.upper()), (pb.lower, pb.upper));
            assert_eq!((ri.lower(), ri.upper()), (rb.lower, rb.upper));
            let interval = pi.intersection(ri).unwrap();
            let truth = dot_f64(&q, &normalize_fp64(&f.rows[i]).unwrap());
            assert!(
                interval.lower() <= truth && truth <= interval.upper(),
                "row {i}: {truth:?}, {interval:?}"
            );
        }
    }
}

fn valid_terms() -> CertificateTerms {
    let f = fixture();
    build_exhaustive_certificate(
        &FixedPointScorer::new(),
        &f.plan,
        &f.table,
        &f.book,
        &block(f, 1),
    )
    .unwrap()
    .primary()
    .terms()
}

#[test]
fn changed_epsilon_transform_or_query_norm_is_rejected() {
    let valid = valid_terms();
    for index in 0..3 {
        let mut terms = valid;
        let field = match index {
            0 => &mut terms.epsilon,
            1 => &mut terms.eta_transform_dot,
            _ => &mut terms.query_norm_upper,
        };
        *field = f64::from_bits(field.to_bits() + 1);
        if index != 0 {
            // Even a self-consistent recomputed epsilon must not authorize
            // transform/query-norm terms from another arithmetic contract.
            let up = |v: f64| f64::from_bits(v.to_bits() + 1);
            terms.epsilon = up(up(terms.eta_transform_dot
                + up(terms.query_norm_upper * terms.max_reconstruction_l2_error))
                + terms.eta_serving_score);
        }
        let field = ["epsilon", "eta_transform_dot", "query_norm_upper"][index];
        assert_eq!(
            validate_certificate_terms(terms, ScoreKind::Primary),
            Err(CertificateError::InvalidTerms { field })
        );
    }
}

#[test]
fn negative_or_nonfinite_certificate_fields_are_rejected() {
    for bad in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for index in 0..5 {
            let mut terms = valid_terms();
            let field = match index {
                0 => &mut terms.max_reconstruction_l2_error,
                1 => &mut terms.eta_transform_dot,
                2 => &mut terms.query_norm_upper,
                3 => &mut terms.eta_serving_score,
                _ => &mut terms.epsilon,
            };
            *field = bad;
            assert!(matches!(
                validate_certificate_terms(terms, ScoreKind::Primary),
                Err(CertificateError::InvalidTerms { .. })
            ));
        }
    }
}

#[test]
fn disjoint_arithmetic_intervals_report_empty_intersection() {
    let terms = validate_certificate_terms(valid_terms(), ScoreKind::Primary).unwrap();
    assert!(matches!(
        terms
            .arithmetic_interval(i64::MIN)
            .intersection(terms.arithmetic_interval(i64::MAX)),
        Err(CertificateError::EmptyIntersection)
    ));
    let interval = terms.arithmetic_interval(0);
    assert_eq!(interval.intersection(interval).unwrap(), interval);
}
