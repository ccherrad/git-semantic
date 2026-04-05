use anyhow::Result;

pub use crate::embeddings::config::{EmbeddingConfig, EmbeddingProviderType};
pub use crate::embeddings::{create_provider, EmbeddingProvider};

pub fn generate_embedding(text: &str) -> Result<Vec<f32>> {
    generate_embedding_with_config(text, None)
}

pub fn generate_embedding_with_config(
    text: &str,
    config: Option<EmbeddingConfig>,
) -> Result<Vec<f32>> {
    let config = config.unwrap_or_else(|| EmbeddingConfig::load_or_default().unwrap_or_default());
    let mut provider = create_provider(&config)?;
    provider.init()?;
    provider.generate_embedding(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_embedding_with_default_config() {
        let result = generate_embedding("test text");
        assert!(result.is_ok());
    }

    #[test]
    fn test_generate_embedding_with_onnx() {
        let mut config = EmbeddingConfig::default();
        config.provider = EmbeddingProviderType::Onnx;

        let result = generate_embedding_with_config("test", Some(config));
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 384);
    }
}
