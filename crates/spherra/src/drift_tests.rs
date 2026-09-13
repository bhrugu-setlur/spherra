use crate::{
    drift::{DriftSample, summarize},
    model::DriftBaseline,
};
#[test]
fn reservoir_is_bounded_batch_independent_and_exact_for_small_commits() {
    let values = |r: u64| {
        [
            (r % 997) as f32 / 997.0,
            (r % 491) as f32 / 982.0,
            (r % 11) as f32 / 11.0,
        ]
    };
    let baseline = DriftBaseline {
        primary: [1.0; 3],
        refined: [1.0; 3],
        outside_fraction: 0.0,
    };
    let mut a = DriftSample::new([7; 16]);
    let mut b = DriftSample::new([7; 16]);
    for r in 0..200_000 {
        a.add(r, values(r), &baseline);
    }
    for batch in (0..200_000).step_by(65_536) {
        for r in batch..(batch + 65_536).min(200_000) {
            b.add(r, values(r), &baseline);
        }
    }
    assert_eq!(a.selected(), b.selected());
    assert_eq!(a.len(), 65536);
    let mut small = DriftSample::new([7; 16]);
    let exact: Vec<_> = (0..3000).map(values).collect();
    for (r, v) in exact.iter().enumerate() {
        small.add(r as u64, *v, &baseline);
    }
    assert_eq!(small.report(&baseline).stats(), &summarize(&exact));
    assert_eq!(small.report(&baseline).sample_size(), 3000);
    assert!(!small.report(&baseline).warned());
}
