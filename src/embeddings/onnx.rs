use anyhow::{Context, Result};
use std::path::PathBuf;

use super::config::EmbeddingConfig;
use super::EmbeddingProvider;

pub struct ONNXProvider {
    config: EmbeddingConfig,
    initialized: bool,
}

impl ONNXProvider {
    pub fn new(config: EmbeddingConfig) -> Result<Self> {
        Ok(Self {
            config,
            initialized: false,
        })
    }

    fn get_model_dir(&self) -> Result<PathBuf> {
        if let Some(path) = &self.config.onnx.model_path {
            return Ok(path.parent().context("Invalid model path")?.to_path_buf());
        }

        let home = std::env::var("HOME").context("HOME environment variable not set")?;
        let model_dir = PathBuf::from(home).join(".gitsem").join("models");

        std::fs::create_dir_all(&model_dir)?;
        Ok(model_dir)
    }

    fn model_path(&self) -> Result<PathBuf> {
        if let Some(path) = &self.config.onnx.model_path {
            return Ok(path.clone());
        }

        Ok(self.get_model_dir()?.join("model.onnx"))
    }

    fn tokenizer_path(&self) -> Result<PathBuf> {
        if let Some(path) = &self.config.onnx.tokenizer_path {
            return Ok(path.clone());
        }

        Ok(self.get_model_dir()?.join("tokenizer.json"))
    }

    fn is_model_downloaded(&self) -> Result<bool> {
        Ok(self.model_path()?.exists() && self.tokenizer_path()?.exists())
    }

    fn download_model(&self) -> Result<()> {
        println!("Downloading ONNX model: {}", self.config.onnx.model_name);

        let base_url = "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main";

        let files = vec![
            ("model.onnx", "onnx/model.onnx"),
            ("tokenizer.json", "tokenizer.json"),
        ];

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;

        let model_dir = self.get_model_dir()?;

        for (filename, remote_path) in files {
            let url = format!("{}/{}", base_url, remote_path);
            let target_path = model_dir.join(filename);

            println!("Downloading {} from {}", filename, url);

            let response = client.get(&url).send()?;

            if !response.status().is_success() {
                anyhow::bail!(
                    "Failed to download {}: HTTP {}",
                    filename,
                    response.status()
                );
            }

            let total_size = response
                .content_length()
                .context("Missing content length")?;

            println!("Downloading {} ({} bytes)...", filename, total_size);

            let mut file = std::fs::File::create(&target_path)?;
            let mut content = response;

            use std::io::Write;
            let mut buffer = [0; 8192];

            loop {
                let bytes_read = std::io::Read::read(&mut content, &mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                file.write_all(&buffer[..bytes_read])?;
            }

            println!("✓ Downloaded {}", filename);
        }

        println!("All model files downloaded successfully");
        Ok(())
    }

    fn ensure_model(&mut self) -> Result<()> {
        if !self.is_model_downloaded()? {
            println!("ONNX model not found. Downloading...");
            self.download_model()?;
        }
        Ok(())
    }
}

impl EmbeddingProvider for ONNXProvider {
    fn init(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        self.ensure_model()?;

        println!("Note: Full ONNX inference will be implemented in the next phase.");
        println!("For now, using placeholder embeddings with correct dimensions.");

        self.initialized = true;
        Ok(())
    }

    fn generate_embedding(&mut self, _text: &str) -> Result<Vec<f32>> {
        if !self.initialized {
            self.init()?;
        }

        let embedding_size = self.config.onnx.embedding_dim;
        let mut embedding = Vec::with_capacity(embedding_size);

        for i in 0..embedding_size {
            embedding.push((i as f32 * 0.001) % 1.0);
        }

        Ok(embedding)
    }

    fn embedding_dimension(&self) -> usize {
        self.config.onnx.embedding_dim
    }

    fn provider_name(&self) -> &str {
        "ONNX (Local)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_onnx_provider_creation() {
        let config = EmbeddingConfig::default();
        let provider = ONNXProvider::new(config);
        assert!(provider.is_ok());
    }

    #[test]
    fn test_embedding_dimension() {
        let config = EmbeddingConfig::default();
        let provider = ONNXProvider::new(config).unwrap();
        assert_eq!(provider.embedding_dimension(), 384);
    }

    #[test]
    fn test_placeholder_embedding() {
        let config = EmbeddingConfig::default();
        let mut provider = ONNXProvider::new(config).unwrap();
        let result = provider.generate_embedding("test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 384);
    }
}
