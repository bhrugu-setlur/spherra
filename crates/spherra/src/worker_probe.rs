//! A worker-count comparison, separate from the 1,000-query acceptance runs.
#[test]
#[ignore = "Task 10 worker probe requires SPHERRA_LATENCY_INDEX pointing at the measured 1M index"]
fn compare_four_six_and_eight_workers() {
    let path = std::env::var("SPHERRA_LATENCY_INDEX").expect("measured 1M index path");
    let source = spherra_testkit::GeneratedChunks::new(1_000_000, 1_000_000, 20260804).unwrap();
    let queries = source.queries(25);
    eprintln!(
        "worker probe: {:?}; {:?}",
        spherra_testkit::machine::SourceRevision::capture(),
        spherra_testkit::MachineProfile::capture()
    );
    for workers in [4, 6, 8] {
        let index = crate::Index::open_with_workers(std::path::Path::new(&path), workers).unwrap();
        assert_eq!(index.len(), 1_000_000);
        for query in &queries[20..] {
            index
                .search(
                    query,
                    crate::SearchOptions {
                        k: 10,
                        candidate_budget: None,
                    },
                )
                .unwrap();
        }
        let mut times = Vec::new();
        for query in &queries[..20] {
            let start = std::time::Instant::now();
            let result = index
                .search(
                    query,
                    crate::SearchOptions {
                        k: 10,
                        candidate_budget: None,
                    },
                )
                .unwrap();
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            std::hint::black_box(result);
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "workers={workers} queries=20 warmup=5 p50_ms={:.6} p99_ms={:.6}",
            times[9], times[19]
        );
    }
}
