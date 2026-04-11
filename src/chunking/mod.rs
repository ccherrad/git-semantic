pub mod languages;
pub mod parser;

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct CodeChunk {
    pub text: String,
    pub start_line: usize,
    pub end_line: usize,
}

pub fn is_supported_file(path: &str) -> bool {
    let excluded_names = [
        "Cargo.lock",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "poetry.lock",
        "Gemfile.lock",
        "go.sum",
    ];

    let file_name = path.split('/').next_back().unwrap_or(path);
    if excluded_names.contains(&file_name) {
        return false;
    }

    let excluded_extensions = ["png", "jpg", "jpeg", "gif", "svg", "ico", "woff", "woff2",
                               "ttf", "eot", "pdf", "zip", "tar", "gz", "bin", "exe"];
    if let Some(ext) = path.split('.').next_back() {
        if excluded_extensions.contains(&ext.to_lowercase().as_str()) {
            return false;
        }
    }

    true
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
