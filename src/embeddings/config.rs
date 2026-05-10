use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmbeddingProviderType {
    OpenAI,
    Gemma,
}

impl std::str::FromStr for EmbeddingProviderType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(EmbeddingProviderType::OpenAI),
            "gemma" => Ok(EmbeddingProviderType::Gemma),
            _ => Err(anyhow::anyhow!(
                "Unknown provider: {}. Valid options: openai, gemma",
                s
            )),
        }
    }
}

impl std::fmt::Display for EmbeddingProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbeddingProviderType::OpenAI => write!(f, "openai"),
            EmbeddingProviderType::Gemma => write!(f, "gemma"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    pub provider: EmbeddingProviderType,
    pub openai: OpenAIConfig,
    pub gemma: GemmaConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GemmaConfig {
    pub embedding_dim: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIConfig {
    pub api_key: Option<String>,
    pub model: String,
    pub max_tokens: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: EmbeddingProviderType::Gemma,
            openai: OpenAIConfig::default(),
            gemma: GemmaConfig::default(),
        }
    }
}

impl Default for GemmaConfig {
    fn default() -> Self {
        Self { embedding_dim: 768 }
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

    pub fn load_or_default() -> Result<Self> {
        let provider_str = Self::get_git_config("topology.provider")
            .or_else(|| std::env::var("TOPOLOGY_PROVIDER").ok())
            .unwrap_or_else(|| "gemma".to_string());

        let provider = provider_str.parse()?;

        Ok(Self {
            provider,
            openai: OpenAIConfig::load(),
            gemma: GemmaConfig::load(),
        })
    }

    pub fn show() -> Result<()> {
        let config = Self::load_or_default()?;

        println!("[topology]");
        println!("  provider = {}", config.provider);
        println!();

        println!("[topology.openai]");
        println!("  model = {}", config.openai.model);
        println!("  maxTokens = {}", config.openai.max_tokens);
        if config.openai.api_key.is_some() {
            println!("  apiKey = ***set via OPENAI_API_KEY***");
        }
        println!();

        println!("[topology.gemma]");
        println!("  embeddingDim = {}", config.gemma.embedding_dim);

        Ok(())
    }
}

impl OpenAIConfig {
    fn load() -> Self {
        Self {
            api_key: std::env::var("OPENAI_API_KEY").ok(),
            model: EmbeddingConfig::get_git_config("topology.openai.model")
                .unwrap_or_else(|| "text-embedding-3-small".to_string()),
            max_tokens: EmbeddingConfig::get_git_config("topology.openai.maxTokens")
                .and_then(|s| s.parse().ok())
                .unwrap_or(8000),
        }
    }
}

impl GemmaConfig {
    fn load() -> Self {
        Self {
            embedding_dim: EmbeddingConfig::get_git_config("topology.gemma.embeddingDim")
                .and_then(|s| s.parse().ok())
                .unwrap_or(768),
        }
    }
}
