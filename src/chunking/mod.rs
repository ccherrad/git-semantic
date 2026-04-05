pub mod languages;
pub mod parser;

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct CodeChunk {
    pub text: String,
    pub start_line: usize,
    pub end_line: usize,
}

pub fn chunk_code(text: &str, file_path: Option<&str>) -> Result<Vec<CodeChunk>> {
    if let Some(path) = file_path {
        if let Some(language) = languages::detect_language(path) {
            return parser::parse_with_tree_sitter(text, language);
        }
    }

    Ok(vec![CodeChunk {
        text: text.to_string(),
        start_line: 0,
        end_line: text.lines().count(),
    }])
}
