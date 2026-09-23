use spherra::{CreateOptions, Index, IndexBuilder, SearchOptions, Vector};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // This small generated set makes the example runnable without an embedding model.
    let training: Vec<Vector> = (0..344)
        .map(|row| {
            std::array::from_fn(|coordinate| {
                ((row * 7 + coordinate * 11) % 101) as f32 / 50.0 - 1.0
            })
        })
        .collect();
    let directory = tempfile::tempdir()?;

    let mut builder = IndexBuilder::create(
        directory.path(),
        &training,
        CreateOptions {
            seed: 20260804,
            validation_rows: None,
        },
    )?;
    for row in &training[..3] {
        builder.push(row)?;
    }
    builder.commit()?;

    let index = Index::open(directory.path())?;
    let result = index.search(
        &training[0],
        SearchOptions {
            k: 3,
            candidate_budget: None,
        },
    )?;
    for hit in result.hits() {
        println!("row {}: score {:.3}", hit.row().get(), hit.score());
    }
    Ok(())
}
