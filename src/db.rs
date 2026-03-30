use crate::models::CodeChunk;
use anyhow::{Context, Result};
use rusqlite::{Connection, ffi::sqlite3_auto_extension, params};
use sqlite_vec::sqlite3_vec_init;
use std::path::PathBuf;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn init() -> Result<Self> {
        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute::<*const (), unsafe extern "C" fn(*mut rusqlite::ffi::sqlite3, *mut *mut i8, *const rusqlite::ffi::sqlite3_api_routines) -> i32>(sqlite3_vec_init as *const ())));
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
                commit_sha TEXT NOT NULL,
                embedding BLOB
            );",
        )
        .context("Failed to create code_chunks table")?;

        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
                embedding FLOAT[1536]
            );",
        )
        .context("Failed to create vec_chunks virtual table")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS vec_metadata (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chunk_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL,
                commit_sha TEXT NOT NULL
            );",
        )
        .context("Failed to create vec_metadata table")?;

        Ok(Database { conn })
    }

    pub fn insert_chunk(&self, chunk: &CodeChunk) -> Result<()> {
        use zerocopy::IntoBytes;

        let embedding_blob =
            bincode::serialize(&chunk.embedding).context("Failed to serialize embedding")?;

        self.conn.execute(
            "INSERT INTO code_chunks (file_path, start_line, end_line, content, commit_sha, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &chunk.file_path,
                chunk.start_line,
                chunk.end_line,
                &chunk.content,
                &chunk.commit_sha,
                &embedding_blob
            ],
        ).context("Failed to insert chunk into database")?;

        self.conn
            .execute(
                "INSERT INTO vec_chunks (embedding) VALUES (?1)",
                params![chunk.embedding.as_bytes()],
            )
            .context("Failed to insert into vec_chunks")?;

        let chunk_id = self.conn.last_insert_rowid();

        self.conn.execute(
            "INSERT INTO vec_metadata (chunk_id, file_path, start_line, end_line, content, commit_sha)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                chunk_id,
                &chunk.file_path,
                chunk.start_line,
                chunk.end_line,
                &chunk.content,
                &chunk.commit_sha
            ],
        ).context("Failed to insert metadata")?;

        Ok(())
    }

    pub fn search_similar(&self, query_embedding: &[f32], limit: i64) -> Result<Vec<CodeChunk>> {
        use zerocopy::IntoBytes;

        let mut stmt = self.conn.prepare(
            "SELECT m.file_path, m.start_line, m.end_line, m.content, m.commit_sha, c.embedding, distance
             FROM vec_chunks v
             JOIN vec_metadata m ON v.rowid = m.chunk_id
             JOIN code_chunks c ON c.file_path = m.file_path
                AND c.start_line = m.start_line
                AND c.commit_sha = m.commit_sha
             WHERE v.embedding MATCH ?1
               AND k = ?2
             ORDER BY distance"
        )?;

        let chunks = stmt
            .query_map(params![query_embedding.as_bytes(), limit], |row| {
                let embedding_blob: Vec<u8> = row.get(5)?;
                let embedding: Vec<f32> = bincode::deserialize(&embedding_blob)
                    .map_err(|_e| rusqlite::Error::InvalidQuery)?;

                Ok(CodeChunk {
                    file_path: row.get(0)?,
                    start_line: row.get(1)?,
                    end_line: row.get(2)?,
                    content: row.get(3)?,
                    commit_sha: row.get(4)?,
                    embedding,
                    distance: row.get(6).ok(),
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
        let test_db_path = std::env::temp_dir().join(format!("test_semantic_{}_{}.db", std::process::id(), timestamp));
        let _ = fs::remove_file(&test_db_path);

        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute::<*const (), unsafe extern "C" fn(*mut rusqlite::ffi::sqlite3, *mut *mut i8, *const rusqlite::ffi::sqlite3_api_routines) -> i32>(
                sqlite3_vec_init as *const ()
            )));
        }

        let conn = Connection::open(&test_db_path)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS code_chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL,
                commit_sha TEXT NOT NULL,
                embedding BLOB
            );"
        )?;

        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
                embedding FLOAT[1536]
            );"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS vec_metadata (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chunk_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL,
                commit_sha TEXT NOT NULL
            );"
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
            commit_sha: "abc123".to_string(),
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
            commit_sha: "commit1".to_string(),
            embedding: vec![1.0; 1536],
            distance: None,
        };

        let chunk2 = CodeChunk {
            file_path: "file2.rs".to_string(),
            start_line: 10,
            end_line: 20,
            content: "database connection".to_string(),
            commit_sha: "commit2".to_string(),
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
            commit_sha: "abc".to_string(),
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
