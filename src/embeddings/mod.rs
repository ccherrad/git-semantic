pub mod config;
pub mod onnx;
pub mod openai;

use anyhow::Result;

pub trait EmbeddingProvider: Send + Sync {
    fn generate_embedding(&mut self, text: &str) -> Result<Vec<f32>>;

    fn embedding_dimension(&self) -> usize;

    fn provider_name(&self) -> &str;

    fn init(&mut self) -> Result<()> {
        Ok(())
    }
}

pub fn create_provider(config: &config::EmbeddingConfig) -> Result<Box<dyn EmbeddingProvider>> {
    match config.provider {
        config::EmbeddingProviderType::OpenAI => {
            Ok(Box::new(openai::OpenAIProvider::new(config.clone())?))
        }
        config::EmbeddingProviderType::ONNX => {
            Ok(Box::new(onnx::ONNXProvider::new(config.clone())?))
        }
    }
}
