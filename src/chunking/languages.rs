use tree_sitter::Language;

#[derive(Debug, Clone, Copy)]
pub enum SupportedLanguage {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Java,
    C,
    Cpp,
    Go,
}

impl SupportedLanguage {
    pub fn tree_sitter_language(&self) -> Language {
        match self {
            SupportedLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
            SupportedLanguage::Python => tree_sitter_python::LANGUAGE.into(),
            SupportedLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            SupportedLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            SupportedLanguage::Java => tree_sitter_java::LANGUAGE.into(),
            SupportedLanguage::C => tree_sitter_c::LANGUAGE.into(),
            SupportedLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            SupportedLanguage::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }
}

pub fn detect_language(file_path: &str) -> Option<SupportedLanguage> {
    let extension = file_path.split('.').last()?.to_lowercase();

    match extension.as_str() {
        "rs" => Some(SupportedLanguage::Rust),
        "py" | "pyw" | "pyi" => Some(SupportedLanguage::Python),
        "js" | "mjs" | "cjs" => Some(SupportedLanguage::JavaScript),
        "ts" => Some(SupportedLanguage::TypeScript),
        "tsx" => Some(SupportedLanguage::TypeScript),
        "java" => Some(SupportedLanguage::Java),
        "c" | "h" => Some(SupportedLanguage::C),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => Some(SupportedLanguage::Cpp),
        "go" => Some(SupportedLanguage::Go),
        _ => None,
    }
}
