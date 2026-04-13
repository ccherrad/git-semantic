use crate::models::CodeChunk;
use anyhow::{Context, Result};
use rusqlite::{ffi::sqlite3_auto_extension, params, Connection};
use sqlite_vec::sqlite3_vec_init;
use std::path::PathBuf;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn init() -> Result<Self> {
        Self::init_with_dimension(None)
    }

    pub fn init_with_dimension(embedding_dim: Option<usize>) -> Result<Self> {
        use crate::embed::EmbeddingConfig;

        let config = EmbeddingConfig::load_or_default()?;
        let dim = embedding_dim.unwrap_or(match config.provider {
            crate::embed::EmbeddingProviderType::OpenAI => 1536,
            crate::embed::EmbeddingProviderType::Onnx => config.onnx.embedding_dim,
        });

        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut i8,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> i32,
            >(sqlite3_vec_init as *const ())));
        }

        let db_path = PathBuf::from(".git/semantic.db");
        let conn = Connection::open(&db_path).context("Failed to open database connection")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS code_chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL,
                embedding BLOB
            );",
        )
        .context("Failed to create code_chunks table")?;

        let table_exists: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='vec_chunks'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !table_exists {
            let create_vec_table = format!(
                "CREATE VIRTUAL TABLE vec_chunks USING vec0(embedding FLOAT[{}]);",
                dim
            );
            conn.execute_batch(&create_vec_table)
                .context("Failed to create vec_chunks virtual table")?;
        }

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS vec_metadata (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chunk_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL
            );",
        )
        .context("Failed to create vec_metadata table")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS subsystems (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                chunks_json TEXT NOT NULL
            );",
        )
        .context("Failed to create subsystems table")?;

        let subsystem_vec_exists: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='vec_subsystems'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !subsystem_vec_exists {
            let create_subsystem_vec = format!(
                "CREATE VIRTUAL TABLE vec_subsystems USING vec0(embedding FLOAT[{}]);",
                dim
            );
            conn.execute_batch(&create_subsystem_vec)
                .context("Failed to create vec_subsystems virtual table")?;
        }

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS edges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_file TEXT NOT NULL,
                to_file TEXT NOT NULL,
                via_json TEXT NOT NULL
            );",
        )
        .context("Failed to create edges table")?;

        Ok(Database { conn })
    }

    pub fn clear(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "DELETE FROM vec_metadata;
                 DELETE FROM vec_chunks;
                 DELETE FROM code_chunks;
                 DELETE FROM subsystems;
                 DELETE FROM vec_subsystems;
                 DELETE FROM edges;",
            )
            .context("Failed to clear database")
    }

    pub fn insert_subsystem(&self, subsystem: &crate::map::Subsystem) -> Result<()> {
        use zerocopy::IntoBytes;

        let chunks_json = serde_json::to_string(&subsystem.chunks)
            .context("Failed to serialize subsystem chunks")?;

        self.conn.execute(
            "INSERT INTO subsystems (name, description, chunks_json) VALUES (?1, ?2, ?3)",
            params![&subsystem.name, &subsystem.description, &chunks_json],
        )?;

        let subsystem_id = self.conn.last_insert_rowid();

        self.conn.execute(
            "INSERT INTO vec_subsystems (rowid, embedding) VALUES (?1, ?2)",
            params![subsystem_id, subsystem.description_embedding.as_bytes()],
        )?;

        Ok(())
    }

    pub fn insert_edge(&self, edge: &crate::map::Edge) -> Result<()> {
        let via_json = serde_json::to_string(&edge.via).context("Failed to serialize edge via")?;
        self.conn.execute(
            "INSERT INTO edges (from_file, to_file, via_json) VALUES (?1, ?2, ?3)",
            params![&edge.from, &edge.to, &via_json],
        )?;
        Ok(())
    }

    pub fn query_map(&self, query_embedding: &[f32]) -> Result<Option<crate::map::Subsystem>> {
        use zerocopy::IntoBytes;

        let mut stmt = self.conn.prepare(
            "SELECT s.name, s.description, s.chunks_json, v.distance
             FROM vec_subsystems v
             JOIN subsystems s ON v.rowid = s.id
             WHERE v.embedding MATCH ?1 AND k = 1
             ORDER BY distance",
        )?;

        let mut rows = stmt.query_map(params![query_embedding.as_bytes()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        if let Some(row) = rows.next() {
            let (name, description, chunks_json) = row?;
            let chunks: Vec<crate::map::ChunkRef> = serde_json::from_str(&chunks_json)
                .map_err(|e| anyhow::anyhow!("Failed to parse chunks: {}", e))?;
            Ok(Some(crate::map::Subsystem {
                name,
                description,
                description_embedding: vec![],
                chunks,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn all_subsystems(&self) -> Result<Vec<crate::map::Subsystem>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, description, chunks_json FROM subsystems ORDER BY id")?;

        let subsystems = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .map(|row| {
                let (name, description, chunks_json) = row?;
                let chunks: Vec<crate::map::ChunkRef> = serde_json::from_str(&chunks_json)
                    .map_err(|e| anyhow::anyhow!("Failed to parse chunks: {}", e))?;
                Ok(crate::map::Subsystem {
                    name,
                    description,
                    description_embedding: vec![],
                    chunks,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(subsystems)
    }

    pub fn edges_into(&self, subsystem_files: &[&str]) -> Result<Vec<crate::map::Edge>> {
        if subsystem_files.is_empty() {
            return Ok(vec![]);
        }

        let n = subsystem_files.len();
        let in_placeholders = (1..=n)
            .map(|i| format!("?{}", i))
            .collect::<Vec<_>>()
            .join(", ");
        let not_in_placeholders = (n + 1..=2 * n)
            .map(|i| format!("?{}", i))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "SELECT from_file, to_file, via_json FROM edges
             WHERE to_file IN ({}) AND from_file NOT IN ({})",
            in_placeholders, not_in_placeholders
        );

        let mut stmt = self.conn.prepare(&sql)?;

        let params: Vec<&dyn rusqlite::ToSql> = subsystem_files
            .iter()
            .chain(subsystem_files.iter())
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();

        let edges = stmt
            .query_map(params.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .map(|row| {
                let (from, to, via_json) = row?;
                let via: Vec<String> = serde_json::from_str(&via_json)
                    .map_err(|e| anyhow::anyhow!("Failed to parse via: {}", e))?;
                Ok(crate::map::Edge { from, to, via })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(edges)
    }

    pub fn insert_chunk(&self, chunk: &CodeChunk) -> Result<()> {
        use zerocopy::IntoBytes;

        let embedding_blob =
            bincode::serialize(&chunk.embedding).context("Failed to serialize embedding")?;

        self.conn
            .execute(
                "INSERT INTO code_chunks (file_path, start_line, end_line, content, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    &chunk.file_path,
                    chunk.start_line,
                    chunk.end_line,
                    &chunk.content,
                    &embedding_blob
                ],
            )
            .context("Failed to insert chunk into database")?;

        let chunk_id = self.conn.last_insert_rowid();

        self.conn
            .execute(
                "INSERT INTO vec_chunks (rowid, embedding) VALUES (?1, ?2)",
                params![chunk_id, chunk.embedding.as_bytes()],
            )
            .context("Failed to insert into vec_chunks")?;

        self.conn
            .execute(
                "INSERT INTO vec_metadata (chunk_id, file_path, start_line, end_line, content)
             VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    chunk_id,
                    &chunk.file_path,
                    chunk.start_line,
                    chunk.end_line,
                    &chunk.content,
                ],
            )
            .context("Failed to insert metadata")?;

        Ok(())
    }

    pub fn get_chunk_by_location(
        &self,
        file_path: &str,
        start_line: i64,
        end_line: i64,
    ) -> Result<Option<CodeChunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT file_path, start_line, end_line, content, embedding
             FROM code_chunks
             WHERE file_path = ?1 AND start_line = ?2 AND end_line = ?3
             LIMIT 1",
        )?;

        let mut rows = stmt.query_map(params![file_path, start_line, end_line], |row| {
            let embedding_blob: Vec<u8> = row.get(4)?;
            let embedding: Vec<f32> = bincode::deserialize(&embedding_blob)
                .map_err(|_e| rusqlite::Error::InvalidQuery)?;
            Ok(CodeChunk {
                file_path: row.get(0)?,
                start_line: row.get(1)?,
                end_line: row.get(2)?,
                content: row.get(3)?,
                embedding,
                distance: None,
            })
        })?;

        if let Some(chunk) = rows.next().transpose()? {
            return Ok(Some(chunk));
        }

        self.get_chunks_overlapping(file_path, start_line, end_line)
    }

    fn get_chunks_overlapping(
        &self,
        file_path: &str,
        start_line: i64,
        end_line: i64,
    ) -> Result<Option<CodeChunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT file_path, start_line, end_line, content, embedding
             FROM code_chunks
             WHERE file_path = ?1
               AND start_line < ?3
               AND end_line > ?2
             ORDER BY start_line",
        )?;

        let chunks: Vec<CodeChunk> = stmt
            .query_map(params![file_path, start_line, end_line], |row| {
                let embedding_blob: Vec<u8> = row.get(4)?;
                let embedding: Vec<f32> = bincode::deserialize(&embedding_blob)
                    .map_err(|_e| rusqlite::Error::InvalidQuery)?;
                Ok(CodeChunk {
                    file_path: row.get(0)?,
                    start_line: row.get(1)?,
                    end_line: row.get(2)?,
                    content: row.get(3)?,
                    embedding,
                    distance: None,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        if chunks.is_empty() {
            return Ok(None);
        }

        let merged_start = chunks.first().unwrap().start_line;
        let merged_end = chunks.last().unwrap().end_line;
        let merged_content = chunks
            .iter()
            .map(|c| c.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(Some(CodeChunk {
            file_path: file_path.to_string(),
            start_line: merged_start,
            end_line: merged_end,
            content: merged_content,
            embedding: chunks.into_iter().next().unwrap().embedding,
            distance: None,
        }))
    }

    pub fn search_similar(&self, query_embedding: &[f32], limit: i64) -> Result<Vec<CodeChunk>> {
        use zerocopy::IntoBytes;

        let mut stmt = self.conn.prepare(
            "SELECT m.file_path, m.start_line, m.end_line, m.content, c.embedding, distance
             FROM vec_chunks v
             JOIN vec_metadata m ON v.rowid = m.chunk_id
             JOIN code_chunks c ON c.id = m.chunk_id
             WHERE v.embedding MATCH ?1
               AND k = ?2
             ORDER BY distance",
        )?;

        let chunks = stmt
            .query_map(params![query_embedding.as_bytes(), limit], |row| {
                let embedding_blob: Vec<u8> = row.get(4)?;
                let embedding: Vec<f32> = bincode::deserialize(&embedding_blob)
                    .map_err(|_e| rusqlite::Error::InvalidQuery)?;

                Ok(CodeChunk {
                    file_path: row.get(0)?,
                    start_line: row.get(1)?,
                    end_line: row.get(2)?,
                    content: row.get(3)?,
                    embedding,
                    distance: row.get(5).ok(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CodeChunk;
    use std::fs;

    fn create_test_db() -> Result<Database> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let test_db_path = std::env::temp_dir().join(format!(
            "test_semantic_{}_{}.db",
            std::process::id(),
            timestamp
        ));
        let _ = fs::remove_file(&test_db_path);

        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut i8,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> i32,
            >(sqlite3_vec_init as *const ())));
        }

        let conn = Connection::open(&test_db_path)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS code_chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL,
                embedding BLOB
            );",
        )?;

        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
                embedding FLOAT[1536]
            );",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS vec_metadata (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chunk_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL
            );",
        )?;

        Ok(Database { conn })
    }

    #[test]
    fn test_database_init() {
        let db = create_test_db();
        assert!(db.is_ok());
    }

    #[test]
    fn test_insert_chunk() {
        let db = create_test_db().unwrap();
        let chunk = CodeChunk {
            file_path: "test.rs".to_string(),
            start_line: 1,
            end_line: 10,
            content: "test content".to_string(),
            embedding: vec![0.5; 1536],
            distance: None,
        };

        let result = db.insert_chunk(&chunk);
        assert!(result.is_ok());
    }

    #[test]
    fn test_insert_and_search() {
        let db = create_test_db().unwrap();

        let chunk1 = CodeChunk {
            file_path: "file1.rs".to_string(),
            start_line: 1,
            end_line: 5,
            content: "authentication logic".to_string(),
            embedding: vec![1.0; 1536],
            distance: None,
        };

        let chunk2 = CodeChunk {
            file_path: "file2.rs".to_string(),
            start_line: 10,
            end_line: 20,
            content: "database connection".to_string(),
            embedding: vec![0.5; 1536],
            distance: None,
        };

        db.insert_chunk(&chunk1).unwrap();
        db.insert_chunk(&chunk2).unwrap();

        let query_embedding = vec![0.9; 1536];
        let results = db.search_similar(&query_embedding, 2).unwrap();

        assert_eq!(results.len(), 2);
        assert!(results[0].distance.is_some());
    }

    #[test]
    fn test_search_similar_ordering() {
        let db = create_test_db().unwrap();

        let chunk = CodeChunk {
            file_path: "test.rs".to_string(),
            start_line: 1,
            end_line: 5,
            content: "test".to_string(),
            embedding: vec![1.0; 1536],
            distance: None,
        };

        db.insert_chunk(&chunk).unwrap();

        let results = db.search_similar(&vec![1.0; 1536], 1).unwrap();
        assert_eq!(results.len(), 1);

        if let Some(dist) = results[0].distance {
            assert!(dist >= 0.0);
        }
    }
}
