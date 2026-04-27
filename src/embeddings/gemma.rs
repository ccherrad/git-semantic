use anyhow::{Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

use super::EmbeddingProvider;

pub struct GemmaProvider {
    model: Option<TextEmbedding>,
}

impl GemmaProvider {
    pub fn new() -> Result<Self> {
        Ok(Self { model: None })
    }
}

impl EmbeddingProvider for GemmaProvider {
    fn init(&mut self) -> Result<()> {
        if self.model.is_some() {
            return Ok(());
        }

        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::EmbeddingGemma300M).with_show_download_progress(true),
        )
        .context("Failed to initialize EmbeddingGemma300M")?;

        self.model = Some(model);
        Ok(())
    }

    fn generate_embedding(&mut self, text: &str) -> Result<Vec<f32>> {
        if self.model.is_none() {
            self.init()?;
        }

        let model = self.model.as_mut().unwrap();
        let mut embeddings = model
            .embed(vec![text.to_string()], None)
            .context("Failed to generate Gemma embedding")?;

        embeddings
            .pop()
            .context("No embedding returned from Gemma model")
    }

    fn embedding_dimension(&self) -> usize {
        768
    }

    fn provider_name(&self) -> &str {
        "Gemma (Local)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemma_provider_creation() {
        let provider = GemmaProvider::new();
        assert!(provider.is_ok());
    }

    #[test]
    fn test_embedding_dimension() {
        let provider = GemmaProvider::new().unwrap();
        assert_eq!(provider.embedding_dimension(), 768);
    }
}
