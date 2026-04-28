use crate::map::{ChunkRef, Edge, SemanticMap, Subsystem};
use crate::semantic_branch::StoredChunk;
use anyhow::Result;
use leiden_rs::{GraphDataBuilder, Leiden, LeidenConfig};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

pub struct ClusterInput {
    pub file: String,
    pub chunk: StoredChunk,
}

struct FileUnit {
    file: String,
    embedding: Vec<f32>,
    chunks: Vec<StoredChunk>,
    defined: HashSet<String>,
    referenced: HashSet<String>,
}

struct DirGroup {
    dir: String,
    files: Vec<FileUnit>,
}

pub fn build_map<F>(inputs: &[ClusterInput], description_embedder: &mut F) -> Result<SemanticMap>
where
    F: FnMut(&str) -> Result<Vec<f32>>,
{
    if inputs.is_empty() {
        return Ok(SemanticMap::empty());
    }

    let file_units = aggregate_by_file(inputs);
    let communities = leiden_cluster(&file_units);

    // Build dir_groups structure for edge detection (reuse existing logic)
    let dir_groups = group_by_directory_refs(&file_units, &communities);

    let mut subsystems = Vec::new();

    for group_files in &communities {
        let dim = group_files[0].embedding.len();

        let centroid = {
            let mut sum = vec![0.0f32; dim];
            for f in group_files.iter() {
                for (d, v) in f.embedding.iter().enumerate() {
                    sum[d] += v;
                }
            }
            let n = group_files.len() as f32;
            sum.iter().map(|v| v / n).collect::<Vec<f32>>()
        };

        let centroid_file = group_files
            .iter()
            .min_by(|a, b| {
                cosine_distance(&a.embedding, &centroid)
                    .partial_cmp(&cosine_distance(&b.embedding, &centroid))
                    .unwrap()
            })
            .unwrap();

        let dir = file_dir(&centroid_file.file);
        let description = build_description(&dir, centroid_file);
        let description_embedding = description_embedder(&description)?;

        let mut sorted_files: Vec<&&FileUnit> = group_files.iter().collect();
        sorted_files.sort_by(|a, b| {
            cosine_distance(&a.embedding, &centroid)
                .partial_cmp(&cosine_distance(&b.embedding, &centroid))
                .unwrap()
        });

        let mut chunks: Vec<ChunkRef> = Vec::new();
        for file in &sorted_files {
            for c in &file.chunks {
                chunks.push(ChunkRef {
                    file: file.file.clone(),
                    start_line: c.start_line,
                    end_line: c.end_line,
                });
            }
        }

        subsystems.push(Subsystem {
            name: slugify(&description),
            description,
            description_embedding,
            chunks,
        });
    }

    subsystems.sort_by(|a, b| b.chunks.len().cmp(&a.chunks.len()));

    let edges = build_edges(&dir_groups);

    Ok(SemanticMap {
        version: 1,
        subsystems,
        edges,
    })
}

fn leiden_cluster(file_units: &[FileUnit]) -> Vec<Vec<&FileUnit>> {
    let n = file_units.len();

    if n <= 1 {
        return file_units.iter().map(|f| vec![f]).collect();
    }

    // Build similarity graph: add edge between files if cosine similarity > threshold
    // similarity = 1 - cosine_distance, must be positive for leiden-rs
    const SIMILARITY_THRESHOLD: f32 = 0.35;

    let mut builder = GraphDataBuilder::new(n);
    for i in 0..n {
        for j in (i + 1)..n {
            let sim = 1.0 - cosine_distance(&file_units[i].embedding, &file_units[j].embedding);
            if sim > SIMILARITY_THRESHOLD {
                // leiden-rs requires f64 weights
                let _ = builder.add_edge(i, j, sim as f64);
            }
        }
    }

    let graph = match builder.build() {
        Ok(g) => g,
        Err(_) => return file_units.iter().map(|f| vec![f]).collect(),
    };

    let config = LeidenConfig {
        seed: Some(42),
        ..Default::default()
    };

    let partition = match Leiden::new(config).run(&graph) {
        Ok(result) => result.partition,
        Err(_) => return file_units.iter().map(|f| vec![f]).collect(),
    };

    // Group file_units by community id
    let mut community_map: HashMap<usize, Vec<&FileUnit>> = HashMap::new();
    for (node_idx, file_unit) in file_units.iter().enumerate() {
        let community = partition.community_of(node_idx);
        community_map.entry(community).or_default().push(file_unit);
    }

    let mut communities: Vec<Vec<&FileUnit>> = community_map.into_values().collect();
    communities.sort_by_key(|b| Reverse(b.len()));
    communities
}

// Adapter: convert community groups into DirGroup structure for edge building
fn group_by_directory_refs<'a>(
    file_units: &'a [FileUnit],
    _communities: &[Vec<&'a FileUnit>],
) -> Vec<DirGroup> {
    group_by_directory(
        file_units
            .iter()
            .map(|f| FileUnit {
                file: f.file.clone(),
                embedding: f.embedding.clone(),
                chunks: f.chunks.clone(),
                defined: f.defined.clone(),
                referenced: f.referenced.clone(),
            })
            .collect(),
    )
}

fn file_dir(file: &str) -> String {
    std::path::Path::new(file)
        .parent()
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(".")
        .to_string()
}

fn aggregate_by_file(inputs: &[ClusterInput]) -> Vec<FileUnit> {
    let mut by_file: HashMap<String, Vec<&ClusterInput>> = HashMap::new();
    for input in inputs {
        by_file.entry(input.file.clone()).or_default().push(input);
    }

    let mut units: Vec<FileUnit> = by_file
        .into_iter()
        .map(|(file, chunks)| {
            let dim = chunks[0].chunk.embedding.len();
            let mut sum = vec![0.0f32; dim];
            for c in &chunks {
                for (d, v) in c.chunk.embedding.iter().enumerate() {
                    sum[d] += v;
                }
            }
            let n = chunks.len() as f32;
            let embedding = sum.iter().map(|v| v / n).collect();
            let stored: Vec<StoredChunk> = chunks.iter().map(|c| c.chunk.clone()).collect();

            let mut defined = HashSet::new();
            let mut referenced = HashSet::new();
            for c in &chunks {
                extract_names(&c.chunk.text, &mut defined, &mut referenced);
            }
            // references to self-defined names are not cross-file edges
            for name in &defined {
                referenced.remove(name);
            }

            FileUnit {
                file,
                embedding,
                chunks: stored,
                defined,
                referenced,
            }
        })
        .collect();

    units.sort_by(|a, b| a.file.cmp(&b.file));
    units
}

fn group_by_directory(file_units: Vec<FileUnit>) -> Vec<DirGroup> {
    let mut by_dir: HashMap<String, Vec<FileUnit>> = HashMap::new();
    for unit in file_units {
        let dir = file_dir(&unit.file);
        by_dir.entry(dir).or_default().push(unit);
    }

    let mut groups: Vec<DirGroup> = by_dir
        .into_iter()
        .map(|(dir, files)| DirGroup { dir, files })
        .collect();

    groups.sort_by(|a, b| a.dir.cmp(&b.dir));
    groups
}

fn build_description(dir: &str, centroid_file: &FileUnit) -> String {
    let stem = std::path::Path::new(&centroid_file.file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&centroid_file.file)
        .to_string();

    let names: Vec<String> = centroid_file
        .chunks
        .iter()
        .filter_map(|c| extract_item_name(&c.text))
        .take(5)
        .collect();

    let dir_label = if dir == "." {
        stem.clone()
    } else {
        format!("{}/{}", dir, stem)
    };

    if names.is_empty() {
        dir_label
    } else {
        format!("{}: {}", dir_label, names.join(", "))
    }
}

fn extract_item_name(text: &str) -> Option<String> {
    for line in text.lines().map(|l| l.trim()) {
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = find_keyword(line, "fn ") {
            return Some(take_ident(rest));
        }
        if let Some(rest) = find_keyword(line, "struct ") {
            return Some(take_ident(rest));
        }
        if let Some(rest) = find_keyword(line, "impl") {
            let rest = rest.trim_start_matches(|c: char| {
                c == '<' || c.is_alphanumeric() || c == '_' || c == ',' || c == ' ' || c == '>'
            });
            return Some(take_ident(rest.trim_start()));
        }
        if let Some(rest) = find_keyword(line, "trait ") {
            return Some(take_ident(rest));
        }
        if let Some(rest) = find_keyword(line, "enum ") {
            return Some(take_ident(rest));
        }
        if let Some(rest) = find_keyword(line, "def ") {
            return Some(take_ident(rest));
        }
        if let Some(rest) = find_keyword(line, "class ") {
            return Some(take_ident(rest));
        }
        if let Some(rest) = find_keyword(line, "func ") {
            return Some(take_ident(rest));
        }
        break;
    }
    None
}

fn find_keyword<'a>(line: &'a str, kw: &str) -> Option<&'a str> {
    line.find(kw).map(|pos| &line[pos + kw.len()..])
}

fn take_ident(s: &str) -> String {
    s.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<String>()
        .chars()
        .take(40)
        .collect()
}

fn build_edges(dir_groups: &[DirGroup]) -> Vec<Edge> {
    // Count how many files define each name — names defined in multiple files
    // are common trait impls (from_str, default, fmt, etc.), not unique symbols.
    let mut definition_count: HashMap<&str, usize> = HashMap::new();
    for group in dir_groups {
        for file in &group.files {
            for name in &file.defined {
                *definition_count.entry(name.as_str()).or_insert(0) += 1;
            }
        }
    }

    // Only index names that are defined in exactly one file (unique symbols)
    let mut name_to_file: HashMap<&str, &str> = HashMap::new();
    for group in dir_groups {
        for file in &group.files {
            for name in &file.defined {
                if definition_count.get(name.as_str()).copied().unwrap_or(0) == 1 {
                    name_to_file.insert(name.as_str(), file.file.as_str());
                }
            }
        }
    }

    let mut edges: Vec<Edge> = Vec::new();

    for group in dir_groups {
        for file in &group.files {
            // For each name this file references, check if another file defines it
            let mut targets: HashMap<&str, Vec<&str>> = HashMap::new();
            for name in &file.referenced {
                if let Some(&defining_file) = name_to_file.get(name.as_str()) {
                    if defining_file != file.file.as_str() {
                        targets
                            .entry(defining_file)
                            .or_default()
                            .push(name.as_str());
                    }
                }
            }

            for (to_file, names) in targets {
                let mut via: Vec<String> = names.iter().map(|s| s.to_string()).collect();
                via.sort();
                via.dedup();
                edges.push(Edge {
                    from: file.file.clone(),
                    to: to_file.to_string(),
                    via,
                });
            }
        }
    }

    edges.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));
    edges
}

fn extract_names(text: &str, defined: &mut HashSet<String>, referenced: &mut HashSet<String>) {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
        }

        // Defined: top-level declarations
        for kw in &[
            "fn ", "struct ", "trait ", "enum ", "def ", "class ", "func ",
        ] {
            if let Some(rest) = find_keyword(trimmed, kw) {
                let name = take_ident(rest);
                if !name.is_empty() {
                    defined.insert(name);
                }
            }
        }
        // impl blocks define the type they implement for
        if let Some(rest) = find_keyword(trimmed, "impl") {
            let rest = rest.trim_start_matches(|c: char| {
                c == '<' || c.is_alphanumeric() || c == '_' || c == ',' || c == ' ' || c == '>'
            });
            let name = take_ident(rest.trim_start());
            if !name.is_empty() {
                defined.insert(name);
            }
        }

        // Referenced: identifiers followed by ( — function/method calls
        // and identifiers after :: — type/module paths
        extract_call_references(trimmed, referenced);
    }
}

fn extract_call_references(line: &str, referenced: &mut HashSet<String>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip non-ident chars
        if !is_ident_start(bytes[i]) {
            i += 1;
            continue;
        }
        // Read the identifier
        let start = i;
        while i < bytes.len() && is_ident_continue(bytes[i]) {
            i += 1;
        }
        let ident = &line[start..i];

        // Skip whitespace
        let mut j = i;
        while j < bytes.len() && bytes[j] == b' ' {
            j += 1;
        }

        // Referenced if followed by ( or ::
        if j < bytes.len()
            && (bytes[j] == b'('
                || (j + 1 < bytes.len() && bytes[j] == b':' && bytes[j + 1] == b':'))
        {
            let name = ident.to_string();
            // Filter out keywords and very short names
            if name.len() > 2 && !is_keyword(&name) {
                referenced.insert(name);
            }
        }

        i = i.max(start + 1);
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        // Rust keywords
        "fn" | "let" | "mut" | "pub" | "use" | "mod" | "impl" | "struct"
        | "enum" | "trait" | "for" | "in" | "if" | "else" | "match"
        | "return" | "self" | "Self" | "super" | "crate" | "where"
        | "async" | "await" | "move" | "ref" | "type" | "const"
        | "static" | "unsafe" | "extern" | "true" | "false"
        // Common stdlib/trait method names that appear in many files
        | "new" | "default" | "clone" | "from" | "into" | "as_ref"
        | "deref" | "drop" | "fmt" | "eq" | "hash" | "from_str"
        | "to_string" | "display" | "debug" | "serialize" | "deserialize"
        | "unwrap" | "expect" | "map" | "and_then" | "ok" | "err"
        | "is_empty" | "len" | "iter" | "collect" | "push" | "pop"
        | "get" | "set" | "insert" | "remove" | "contains" | "extend"
        // Common enum variants
        | "None" | "Some" | "Ok" | "Err"
        // Other language keywords
        | "def" | "class" | "import" | "func" | "var"
        | "this" | "null" | "void" | "show" | "load" | "read" | "write"
    )
}

pub fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 1.0;
    }
    1.0 - (dot / (norm_a * norm_b))
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(50)
        .collect()
}
