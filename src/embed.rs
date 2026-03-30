use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct EmbeddingRequest {
    input: String,
    model: String,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

pub fn generate_embedding(text: &str) -> Result<Vec<f32>> {
    let api_key = match std::env::var("OPENAI_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("Warning: OPENAI_API_KEY not set, using placeholder embeddings");
            eprintln!("Set OPENAI_API_KEY environment variable for real semantic search:");
            eprintln!("  export OPENAI_API_KEY='sk-...'");
            return generate_placeholder_embedding();
        }
    };

    if api_key.is_empty() {
        eprintln!("Warning: OPENAI_API_KEY is empty, using placeholder embeddings");
        return generate_placeholder_embedding();
    }

    match call_openai_api(&api_key, text) {
        Ok(embedding) => Ok(embedding),
        Err(e) => {
            eprintln!("Warning: OpenAI API call failed: {}", e);
            eprintln!("Falling back to placeholder embeddings");
            generate_placeholder_embedding()
        }
    }
}

fn call_openai_api(api_key: &str, text: &str) -> Result<Vec<f32>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("Failed to create HTTP client")?;

    let truncated_text = if text.len() > 8000 {
        &text[..8000]
    } else {
        text
    };

    let request = EmbeddingRequest {
        input: truncated_text.to_string(),
        model: "text-embedding-3-small".to_string(),
    };

    let response = client
        .post("https://api.openai.com/v1/embeddings")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .context("Failed to send request to OpenAI API")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().unwrap_or_default();
        anyhow::bail!("OpenAI API error ({}): {}", status, error_text);
    }

    let embedding_response: EmbeddingResponse = response
        .json()
        .context("Failed to parse OpenAI API response")?;

    embedding_response
        .data
        .into_iter()
        .next()
        .map(|d| d.embedding)
        .context("No embedding data in response")
}

fn generate_placeholder_embedding() -> Result<Vec<f32>> {
    let embedding_size = 1536;
    let mut embedding = Vec::with_capacity(embedding_size);

    for i in 0..embedding_size {
        embedding.push((i as f32 * 0.001) % 1.0);
    }

    Ok(embedding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder_embedding_size() {
        let embedding = generate_placeholder_embedding().unwrap();
        assert_eq!(embedding.len(), 1536);
    }

    #[test]
    fn test_placeholder_embedding_values() {
        let embedding = generate_placeholder_embedding().unwrap();
        assert!(embedding.iter().all(|&v| v >= 0.0 && v < 1.0));
    }

    #[test]
    fn test_generate_embedding_without_api_key() {
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
        }
        let result = generate_embedding("test text");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1536);
    }

    #[test]
    fn test_generate_embedding_with_empty_text() {
        let result = generate_embedding("");
        assert!(result.is_ok());
    }
}
