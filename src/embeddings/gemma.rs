use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::config::EmbeddingConfig;
use super::EmbeddingProvider;

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    input: String,
}

#[derive(Deserialize)]
struct OllamaResponse {
    embeddings: Vec<Vec<f32>>,
}

#[derive(Serialize)]
struct HuggingFaceRequest {
    inputs: String,
}

pub struct GemmaProvider {
    config: EmbeddingConfig,
}

impl GemmaProvider {
    pub fn new(config: EmbeddingConfig) -> Result<Self> {
        Ok(Self { config })
    }

    fn call_api(&self, text: &str) -> Result<Vec<f32>> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        let truncated_text = if text.len() > self.config.gemma.max_tokens {
            &text[..self.config.gemma.max_tokens]
        } else {
            text
        };

        match &self.config.gemma.api_key {
            Some(api_key) => self.call_huggingface(&client, truncated_text, api_key),
            None => self.call_ollama(&client, truncated_text),
        }
    }

    fn call_ollama(&self, client: &reqwest::blocking::Client, text: &str) -> Result<Vec<f32>> {
        let base_url = self
            .config
            .gemma
            .base_url
            .as_deref()
            .unwrap_or("http://localhost:11434");

        let request = OllamaRequest {
            model: self.config.gemma.model.clone(),
            input: text.to_string(),
        };

        let response = client
            .post(format!("{}/api/embed", base_url))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .context("Failed to send request to Ollama")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().unwrap_or_default();
            anyhow::bail!("Ollama API error ({}): {}", status, error_text);
        }

        let embedding_response: OllamaResponse = response
            .json()
            .context("Failed to parse Ollama response")?;

        embedding_response
            .embeddings
            .into_iter()
            .next()
            .context("No embedding data in Ollama response")
    }

    fn call_huggingface(
        &self,
        client: &reqwest::blocking::Client,
        text: &str,
        api_key: &str,
    ) -> Result<Vec<f32>> {
        let base_url = self.config.gemma.base_url.as_deref().unwrap_or(
            "https://api-inference.huggingface.co/pipeline/feature-extraction/google/embedding-gemma-300m",
        );

        let request = HuggingFaceRequest {
            inputs: text.to_string(),
        };

        let response = client
            .post(base_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .context("Failed to send request to HuggingFace")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().unwrap_or_default();
            anyhow::bail!("HuggingFace API error ({}): {}", status, error_text);
        }

        let embedding: Vec<f32> = response
            .json()
            .context("Failed to parse HuggingFace response")?;

        Ok(embedding)
    }
}

impl EmbeddingProvider for GemmaProvider {
    fn generate_embedding(&mut self, text: &str) -> Result<Vec<f32>> {
        self.call_api(text)
    }

    fn embedding_dimension(&self) -> usize {
        self.config.gemma.embedding_dim
    }

    fn provider_name(&self) -> &str {
        "EmbeddingGemma"
    }
}
