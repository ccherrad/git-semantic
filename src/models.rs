use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CodeChunk {
    pub file_path: String,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
    pub commit_sha: String,
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
            commit_sha: "abc123".to_string(),
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
    fn test_code_chunk_with_distance() {
        let chunk = CodeChunk {
            file_path: "src/lib.rs".to_string(),
            start_line: 1,
            end_line: 5,
            content: "pub fn test() {}".to_string(),
            commit_sha: "def456".to_string(),
            embedding: vec![0.5; 1536],
            distance: Some(0.75),
        };

        assert_eq!(chunk.distance, Some(0.75));
    }

    #[test]
    fn test_code_chunk_serialization() {
        let chunk = CodeChunk {
            file_path: "test.rs".to_string(),
            start_line: 0,
            end_line: 1,
            content: "test".to_string(),
            commit_sha: "123".to_string(),
            embedding: vec![1.0, 2.0],
            distance: Some(0.5),
        };

        let json = serde_json::to_string(&chunk).unwrap();
        let deserialized: CodeChunk = serde_json::from_str(&json).unwrap();

        assert_eq!(chunk.file_path, deserialized.file_path);
        assert_eq!(chunk.commit_sha, deserialized.commit_sha);
        assert_eq!(chunk.distance, deserialized.distance);
    }

    #[test]
    fn test_code_chunk_serialization_without_distance() {
        let json = r#"{
            "file_path": "test.rs",
            "start_line": 0,
            "end_line": 1,
            "content": "test",
            "commit_sha": "123",
            "embedding": [1.0, 2.0]
        }"#;

        let chunk: CodeChunk = serde_json::from_str(json).unwrap();
        assert!(chunk.distance.is_none());
    }
}
