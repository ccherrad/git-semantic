use anyhow::Result;

pub fn generate_embedding(_text: &str) -> Result<Vec<f32>> {
    let embedding_size = 768;
    let mut embedding = Vec::with_capacity(embedding_size);

    for i in 0..embedding_size {
        embedding.push((i as f32 * 0.001) % 1.0);
    }

    Ok(embedding)
}
