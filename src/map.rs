use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SemanticMap {
    pub version: u8,
    pub subsystems: Vec<Subsystem>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub via: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Subsystem {
    pub name: String,
    pub description: String,
    pub description_embedding: Vec<f32>,
    pub chunks: Vec<ChunkRef>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChunkRef {
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
}

impl ChunkRef {
    pub fn display(&self) -> String {
        format!("{}:{}-{}", self.file, self.start_line, self.end_line)
    }

    pub fn parse(s: &str) -> Option<Self> {
        let (file, range) = s.rsplit_once(':')?;
        let (start, end) = range.split_once('-')?;
        Some(ChunkRef {
            file: file.to_string(),
            start_line: start.parse().ok()?,
            end_line: end.parse().ok()?,
        })
    }
}

impl SemanticMap {
    pub fn empty() -> Self {
        SemanticMap {
            version: 1,
            subsystems: vec![],
            edges: vec![],
        }
    }
}
