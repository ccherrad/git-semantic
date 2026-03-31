use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::config::EmbeddingConfig;
use super::EmbeddingProvider;

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

pub struct OpenAIProvider {
    config: EmbeddingConfig,
    api_key: String,
}

impl OpenAIProvider {
    pub fn new(config: EmbeddingConfig) -> Result<Self> {
        let api_key = config.openai.api_key.clone()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .context("OpenAI API key not found. Set OPENAI_API_KEY environment variable or configure in .git/gitsem_config.json")?;

        if api_key.is_empty() {
            anyhow::bail!("OpenAI API key is empty");
        }

        Ok(Self { config, api_key })
    }

    fn call_api(&self, text: &str) -> Result<Vec<f32>> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        let truncated_text = if text.len() > self.config.openai.max_tokens {
            &text[..self.config.openai.max_tokens]
        } else {
            text
        };

        let request = EmbeddingRequest {
            input: truncated_text.to_string(),
            model: self.config.openai.model.clone(),
        };

        let response = client
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.api_key))
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
}

impl EmbeddingProvider for OpenAIProvider {
    fn generate_embedding(&mut self, text: &str) -> Result<Vec<f32>> {
        self.call_api(text)
    }

    fn embedding_dimension(&self) -> usize {
        1536
    }

    fn provider_name(&self) -> &str {
        "OpenAI"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_provider_creation_without_key() {
        std::env::remove_var("OPENAI_API_KEY");
        let mut config = EmbeddingConfig::default();
        config.openai.api_key = None;

        let result = OpenAIProvider::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_embedding_dimension() {
        std::env::set_var("OPENAI_API_KEY", "test-key");
        let config = EmbeddingConfig::default();
        let provider = OpenAIProvider::new(config).unwrap();
        assert_eq!(provider.embedding_dimension(), 1536);
    }
}
