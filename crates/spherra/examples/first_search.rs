use spherra::{CreateOptions, Index, IndexBuilder, SearchOptions, Vector};

fn generated_vector(vector_number: usize) -> Vector {
    std::array::from_fn(|coordinate| {
        ((vector_number * 7 + coordinate * 11) % 997) as f32 / 498.0 - 1.0
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Keep training, indexed, and query vectors separate.
    let training: Vec<Vector> = (3..347).map(generated_vector).collect();
    // These ten inputs span a range of actual cosine similarities to the query.
    let indexed = [857, 991, 560, 983, 836, 831, 743, 961, 383, 655].map(generated_vector);
    let query = generated_vector(0);
    let directory = tempfile::tempdir()?;

    let mut builder = IndexBuilder::create(
        directory.path(),
        &training,
        CreateOptions {
            seed: 20260804,
            validation_rows: None,
        },
    )?;
    for vector in &indexed {
        builder.push(vector)?;
    }
    builder.commit()?;

    let index = Index::open(directory.path())?;
    let result = index.search(
        &query,
        SearchOptions {
            k: indexed.len(),
            candidate_budget: None,
        },
    )?;
    for hit in result.hits() {
        // The index assigns IDs 0 through 9 in insertion order.
        println!(
            "vector {}: similarity score {:.3}",
            hit.row().get() + 1,
            hit.score()
        );
    }
    Ok(())
}
