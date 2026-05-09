use anyhow::{Context, Result};
use tree_sitter::Parser;

use super::languages::SupportedLanguage;
use super::CodeChunk;

const BODY_NODE_TYPES: &[&str] = &[
    "block",
    "statement_block",
    "compound_statement",
    "class_body",
    "enum_body",
    "field_declaration_list",
    "declaration_list",
];

pub fn extract_signature(text: &str, language: super::languages::SupportedLanguage) -> String {
    let mut parser = tree_sitter::Parser::new();
    let ts_language = language.tree_sitter_language();
    if parser.set_language(&ts_language).is_err() {
        return first_line_fallback(text);
    }

    let tree = match parser.parse(text, None) {
        Some(t) => t,
        None => return first_line_fallback(text),
    };

    let root = tree.root_node();
    let mut cursor = root.walk();

    // Walk into the first meaningful top-level node
    let top = root
        .children(&mut cursor)
        .find(|n| !n.is_extra() && n.child_count() > 0);

    let node = match top {
        Some(n) => n,
        None => return first_line_fallback(text),
    };

    // Find the body child — everything before it is the signature
    let mut body_start_byte: Option<usize> = None;
    let mut node_cursor = node.walk();
    for child in node.children(&mut node_cursor) {
        if BODY_NODE_TYPES.contains(&child.kind()) {
            body_start_byte = Some(child.start_byte());
            break;
        }
    }

    match body_start_byte {
        Some(body_start) => {
            let sig = text[..body_start].trim_end_matches([' ', '\t', '{', ':']);
            sig.trim_end().to_string()
        }
        None => text.trim_end_matches('{').trim_end().to_string(),
    }
}

pub fn extract_name(text: &str, language: super::languages::SupportedLanguage) -> String {
    let mut parser = tree_sitter::Parser::new();
    let ts_language = language.tree_sitter_language();
    if parser.set_language(&ts_language).is_err() {
        return first_line_fallback(text);
    }

    let tree = match parser.parse(text, None) {
        Some(t) => t,
        None => return first_line_fallback(text),
    };

    let root = tree.root_node();
    let mut cursor = root.walk();

    let top = root
        .children(&mut cursor)
        .find(|n| !n.is_extra() && n.child_count() > 0);

    let node = match top {
        Some(n) => n,
        None => return first_line_fallback(text),
    };

    // Name child is typically named "name" in tree-sitter grammars
    let mut node_cursor = node.walk();
    for child in node.children(&mut node_cursor) {
        if child.kind() == "name"
            || child.kind() == "identifier"
            || child.kind() == "type_identifier"
        {
            if let Ok(name) = child.utf8_text(text.as_bytes()) {
                return name.to_string();
            }
        }
    }

    first_line_fallback(text)
}

fn first_line_fallback(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .trim_end_matches(['{', ':'])
        .trim_end()
        .to_string()
}

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
        });
    } else {
        // If the first chunk doesn't start at line 0, there's a preamble
        // (imports, constants, top-level declarations) that would otherwise
        // be invisible to the index. Capture it as its own chunk.
        let first_start = chunks.iter().map(|c| c.start_line).min().unwrap_or(0);
        if first_start > 0 {
            let preamble: String = text
                .lines()
                .take(first_start)
                .collect::<Vec<_>>()
                .join("\n");
            if !preamble.trim().is_empty() {
                chunks.insert(
                    0,
                    CodeChunk {
                        text: preamble,
                        start_line: 0,
                        end_line: first_start,
                    },
                );
            }
        }
    }

    Ok(chunks)
}

fn walk_tree(text: &str, node: tree_sitter::Node, chunks: &mut Vec<CodeChunk>) {
    let node_kind = node.kind();

    if CHUNK_NODE_TYPES.contains(&node_kind) || is_top_level_definition(&node) {
        let start_line = node.start_position().row;
        let end_line = node.end_position().row;
        let start_byte = node.start_byte();
        let end_byte = node.end_byte();

        if let Some(chunk_text) = text.get(start_byte..end_byte) {
            chunks.push(CodeChunk {
                text: chunk_text.to_string(),
                start_line,
                end_line,
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
