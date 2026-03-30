use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use crate::models::CodeChunk;
use std::path::PathBuf;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn init() -> Result<Self> {
        let db_path = PathBuf::from(".git/semantic.db");
        let conn = Connection::open(&db_path)
            .context("Failed to open database connection")?;

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
        ).context("Failed to create code_chunks table")?;

        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
                file_path TEXT,
                start_line INTEGER,
                end_line INTEGER,
                content TEXT,
                commit_sha TEXT,
                embedding FLOAT[768]
            );"
        ).context("Failed to create vec_chunks virtual table")?;

        Ok(Database { conn })
    }

    pub fn insert_chunk(&self, chunk: &CodeChunk) -> Result<()> {
        let embedding_blob = bincode::serialize(&chunk.embedding)
            .context("Failed to serialize embedding")?;

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

        let embedding_json = serde_json::to_string(&chunk.embedding)?;
        self.conn.execute(
            "INSERT INTO vec_chunks (file_path, start_line, end_line, content, commit_sha, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &chunk.file_path,
                chunk.start_line,
                chunk.end_line,
                &chunk.content,
                &chunk.commit_sha,
                &embedding_json
            ],
        ).context("Failed to insert chunk into vec_chunks")?;

        Ok(())
    }

    pub fn search_similar(&self, query_embedding: &[f32], limit: i64) -> Result<Vec<CodeChunk>> {
        let query_json = serde_json::to_string(query_embedding)?;

        let mut stmt = self.conn.prepare(
            "SELECT file_path, start_line, end_line, content, commit_sha, embedding
             FROM vec_chunks
             WHERE embedding MATCH ?1
             ORDER BY distance
             LIMIT ?2"
        )?;

        let chunks = stmt.query_map(params![query_json, limit], |row| {
            let embedding_str: String = row.get(5)?;
            let embedding: Vec<f32> = serde_json::from_str(&embedding_str)
                .map_err(|_e| rusqlite::Error::InvalidQuery)?;

            Ok(CodeChunk {
                file_path: row.get(0)?,
                start_line: row.get(1)?,
                end_line: row.get(2)?,
                content: row.get(3)?,
                commit_sha: row.get(4)?,
                embedding,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }
}
