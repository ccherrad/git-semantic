use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmbeddingProviderType {
    OpenAI,
    Onnx,
}

impl std::str::FromStr for EmbeddingProviderType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(EmbeddingProviderType::OpenAI),
            "onnx" | "local" => Ok(EmbeddingProviderType::Onnx),
            _ => Err(anyhow::anyhow!(
                "Unknown provider: {}. Valid options: openai, onnx",
                s
            )),
        }
    }
}

impl std::fmt::Display for EmbeddingProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbeddingProviderType::OpenAI => write!(f, "openai"),
            EmbeddingProviderType::Onnx => write!(f, "onnx"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    pub provider: EmbeddingProviderType,
    pub openai: OpenAIConfig,
    pub onnx: ONNXConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIConfig {
    pub api_key: Option<String>,
    pub model: String,
    pub max_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ONNXConfig {
    pub model_name: String,
    pub model_path: Option<PathBuf>,
    pub tokenizer_path: Option<PathBuf>,
    pub embedding_dim: usize,
    pub max_length: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: EmbeddingProviderType::OpenAI,
            openai: OpenAIConfig::default(),
            onnx: ONNXConfig::default(),
        }
    }
}

impl Default for OpenAIConfig {
    fn default() -> Self {
        Self {
            api_key: std::env::var("OPENAI_API_KEY").ok(),
            model: "text-embedding-3-small".to_string(),
            max_tokens: 8000,
        }
    }
}

impl Default for ONNXConfig {
    fn default() -> Self {
        Self {
            model_name: "bge-small-en-v1.5".to_string(),
            model_path: None,
            tokenizer_path: None,
            embedding_dim: 384,
            max_length: 512,
        }
    }
}

impl EmbeddingConfig {
    pub fn get_git_config(key: &str) -> Option<String> {
        Command::new("git")
            .args(["config", "--get", key])
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    String::from_utf8(output.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
    }

    pub fn set_git_config(key: &str, value: &str) -> Result<()> {
        let status = Command::new("git")
            .args(["config", key, value])
            .status()
            .context("Failed to execute git config")?;

        if !status.success() {
            anyhow::bail!("Failed to set git config {}", key);
        }

        Ok(())
    }

    pub fn unset_git_config(key: &str) -> Result<()> {
        let status = Command::new("git")
            .args(["config", "--unset", key])
            .status()
            .context("Failed to execute git config")?;

        if !status.success() {
            anyhow::bail!("Failed to unset git config {}", key);
        }

        Ok(())
    }

    pub fn get_default(key: &str) -> Option<String> {
        let d = Self::default();
        match key {
            "semantic.provider" => Some(d.provider.to_string()),
            "semantic.openai.model" => Some(d.openai.model),
            "semantic.openai.maxTokens" => Some(d.openai.max_tokens.to_string()),
            "semantic.onnx.modelName" => Some(d.onnx.model_name),
            "semantic.onnx.embeddingDim" => Some(d.onnx.embedding_dim.to_string()),
            "semantic.onnx.maxLength" => Some(d.onnx.max_length.to_string()),
            _ => None,
        }
    }

    pub fn load_or_default() -> Result<Self> {
        let provider_str = Self::get_git_config("semantic.provider")
            .or_else(|| std::env::var("SEMANTIC_PROVIDER").ok())
            .unwrap_or_else(|| "openai".to_string());

        let provider = provider_str.parse()?;

        Ok(Self {
            provider,
            openai: OpenAIConfig::load(),
            onnx: ONNXConfig::load(),
        })
    }

    #[allow(dead_code)]
    pub fn save(&self) -> Result<()> {
        Self::set_git_config("semantic.provider", &self.provider.to_string())?;
        self.openai.save()?;
        self.onnx.save()?;
        Ok(())
    }

    pub fn show() -> Result<()> {
        let config = Self::load_or_default()?;

        println!("[semantic]");
        println!("  provider = {}", config.provider);
        println!();

        println!("[semantic.openai]");
        println!("  model = {}", config.openai.model);
        println!("  maxTokens = {}", config.openai.max_tokens);
        if config.openai.api_key.is_some() {
            println!("  apiKey = ***set via OPENAI_API_KEY***");
        }
        println!();

        println!("[semantic.onnx]");
        println!("  modelName = {}", config.onnx.model_name);
        println!("  embeddingDim = {}", config.onnx.embedding_dim);
        println!("  maxLength = {}", config.onnx.max_length);
        if let Some(path) = &config.onnx.model_path {
            println!("  modelPath = {}", path.display());
        }
        if let Some(path) = &config.onnx.tokenizer_path {
            println!("  tokenizerPath = {}", path.display());
        }

        Ok(())
    }
}

impl OpenAIConfig {
    fn load() -> Self {
        Self {
            api_key: std::env::var("OPENAI_API_KEY").ok(),
            model: EmbeddingConfig::get_git_config("semantic.openai.model")
                .unwrap_or_else(|| "text-embedding-3-small".to_string()),
            max_tokens: EmbeddingConfig::get_git_config("semantic.openai.maxTokens")
                .and_then(|s| s.parse().ok())
                .unwrap_or(8000),
        }
    }

    #[allow(dead_code)]
    fn save(&self) -> Result<()> {
        EmbeddingConfig::set_git_config("semantic.openai.model", &self.model)?;
        EmbeddingConfig::set_git_config("semantic.openai.maxTokens", &self.max_tokens.to_string())?;
        Ok(())
    }
}

impl ONNXConfig {
    fn load() -> Self {
        Self {
            model_name: EmbeddingConfig::get_git_config("semantic.onnx.modelName")
                .unwrap_or_else(|| "bge-small-en-v1.5".to_string()),
            model_path: EmbeddingConfig::get_git_config("semantic.onnx.modelPath")
                .map(PathBuf::from),
            tokenizer_path: EmbeddingConfig::get_git_config("semantic.onnx.tokenizerPath")
                .map(PathBuf::from),
            embedding_dim: EmbeddingConfig::get_git_config("semantic.onnx.embeddingDim")
                .and_then(|s| s.parse().ok())
                .unwrap_or(384),
            max_length: EmbeddingConfig::get_git_config("semantic.onnx.maxLength")
                .and_then(|s| s.parse().ok())
                .unwrap_or(512),
        }
    }

    #[allow(dead_code)]
    fn save(&self) -> Result<()> {
        EmbeddingConfig::set_git_config("semantic.onnx.modelName", &self.model_name)?;
        EmbeddingConfig::set_git_config(
            "semantic.onnx.embeddingDim",
            &self.embedding_dim.to_string(),
        )?;
        EmbeddingConfig::set_git_config("semantic.onnx.maxLength", &self.max_length.to_string())?;

        if let Some(path) = &self.model_path {
            EmbeddingConfig::set_git_config("semantic.onnx.modelPath", path.to_str().unwrap())?;
        }
        if let Some(path) = &self.tokenizer_path {
            EmbeddingConfig::set_git_config("semantic.onnx.tokenizerPath", path.to_str().unwrap())?;
        }

        Ok(())
    }
}
