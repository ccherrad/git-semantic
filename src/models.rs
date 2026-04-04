use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CodeChunk {
    pub file_path: String,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
    pub embedding: Vec<f32>,
    #[serde(default)]
    pub distance: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_chunk_creation() {
        let chunk = CodeChunk {
            file_path: "src/main.rs".to_string(),
            start_line: 10,
            end_line: 20,
            content: "fn main() {}".to_string(),
            embedding: vec![0.1, 0.2, 0.3],
            distance: None,
        };

        assert_eq!(chunk.file_path, "src/main.rs");
        assert_eq!(chunk.start_line, 10);
        assert_eq!(chunk.end_line, 20);
        assert_eq!(chunk.embedding.len(), 3);
        assert!(chunk.distance.is_none());
    }

    #[test]
    fn test_code_chunk_serialization() {
        let chunk = CodeChunk {
            file_path: "test.rs".to_string(),
            start_line: 0,
            end_line: 1,
            content: "test".to_string(),
            embedding: vec![1.0, 2.0],
            distance: Some(0.5),
        };

        let json = serde_json::to_string(&chunk).unwrap();
        let deserialized: CodeChunk = serde_json::from_str(&json).unwrap();

        assert_eq!(chunk.file_path, deserialized.file_path);
        assert_eq!(chunk.distance, deserialized.distance);
    }
}
