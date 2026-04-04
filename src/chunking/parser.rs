use anyhow::{Context, Result};
use tree_sitter::Parser;

use super::languages::SupportedLanguage;
use super::CodeChunk;

const CHUNK_NODE_TYPES: &[&str] = &[
    "function_item",
    "function_declaration",
    "function_definition",
    "method_declaration",
    "method_definition",
    "class_declaration",
    "class_definition",
    "impl_item",
    "struct_item",
    "enum_item",
    "trait_item",
];

pub fn parse_with_tree_sitter(text: &str, language: SupportedLanguage) -> Result<Vec<CodeChunk>> {
    let mut parser = Parser::new();
    let ts_language = language.tree_sitter_language();

    parser
        .set_language(&ts_language)
        .context("Failed to set tree-sitter language")?;

    let tree = parser
        .parse(text, None)
        .context("Failed to parse code with tree-sitter")?;

    let root_node = tree.root_node();
    let mut chunks = Vec::new();

    walk_tree(text, root_node, &mut chunks);

    if chunks.is_empty() {
        chunks.push(CodeChunk {
            text: text.to_string(),
            start_line: 0,
            end_line: text.lines().count(),
            start_byte: 0,
            end_byte: text.len(),
        });
    }

    Ok(chunks)
}

fn walk_tree(text: &str, node: tree_sitter::Node, chunks: &mut Vec<CodeChunk>) {
    let node_kind = node.kind();

    if CHUNK_NODE_TYPES.contains(&node_kind) || is_top_level_definition(&node) {
        let start_byte = node.start_byte();
        let end_byte = node.end_byte();
        let start_line = node.start_position().row;
        let end_line = node.end_position().row;

        if let Some(chunk_text) = text.get(start_byte..end_byte) {
            chunks.push(CodeChunk {
                text: chunk_text.to_string(),
                start_line,
                end_line,
                start_byte,
                end_byte,
            });
            return;
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree(text, child, chunks);
    }
}

fn is_top_level_definition(node: &tree_sitter::Node) -> bool {
    let kind = node.kind();

    matches!(
        kind,
        "function"
            | "class"
            | "method"
            | "struct"
            | "enum"
            | "trait"
            | "impl"
            | "interface"
            | "type_alias"
            | "const_declaration"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rust_code() {
        let code = r#"
fn main() {
    println!("Hello, world!");
}

fn helper() {
    println!("Helper");
}
        "#;

        let chunks = parse_with_tree_sitter(code, SupportedLanguage::Rust).unwrap();
        assert!(chunks.len() >= 2, "Should find at least 2 functions");
    }

    #[test]
    fn test_parse_python_code() {
        let code = r#"
def main():
    print("Hello, world!")

def helper():
    print("Helper")
        "#;

        let chunks = parse_with_tree_sitter(code, SupportedLanguage::Python).unwrap();
        assert!(chunks.len() >= 2, "Should find at least 2 functions");
    }
}
