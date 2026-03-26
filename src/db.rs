use anyhow::Context;
use chrono::Utc;
use directories::ProjectDirs;
use rusqlite::{params, Connection, Result};
use std::sync::Mutex;

const CREATE_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS folders (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id  INTEGER REFERENCES folders(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS items (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    folder_id    INTEGER REFERENCES folders(id) ON DELETE CASCADE,
    content_type TEXT NOT NULL,
    content_data TEXT NOT NULL,
    label        TEXT,
    created_at   TEXT NOT NULL,
    is_favorite  INTEGER NOT NULL DEFAULT 0
);
";

const ADD_CONTENT_BLOB_MIGRATION_SQL: &str = "ALTER TABLE items ADD COLUMN content_blob BLOB";

const SELECT_ITEM_COLUMNS: &str = "
SELECT id, folder_id, content_type, content_data, label, is_favorite
FROM items
";

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct Folder {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: i64,
    pub content_type: String,
    pub content_data: String,
    pub label: Option<String>,
    pub is_favorite: bool,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db")
            .field("conn", &"Mutex<Connection>")
            .finish()
    }
}

impl Db {
    pub fn new() -> anyhow::Result<Self> {
        let path = data_dir()?;
        std::fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create data directory: {}", path.display()))?;

        let db_path = path.join("jubako.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

        initialize_connection(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert_item(&self, content: &str, content_type: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO items (folder_id, content_type, content_data, created_at)
             VALUES (NULL, ?1, ?2, ?3)",
            params![content_type, content, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_history(&self, limit: usize) -> Result<Vec<Item>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{SELECT_ITEM_COLUMNS}
             WHERE folder_id IS NULL
             ORDER BY created_at DESC
             LIMIT ?1"
        ))?;
        let rows = stmt.query_map(params![limit as i64], row_to_item)?;
        rows.collect()
    }

    pub fn clear_history(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM items WHERE folder_id IS NULL", [])?;
        Ok(())
    }

    pub fn get_items_in_folder(&self, folder_id: i64) -> Result<Vec<Item>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{SELECT_ITEM_COLUMNS}
             WHERE folder_id = ?1
             ORDER BY created_at DESC"
        ))?;
        let rows = stmt.query_map(params![folder_id], row_to_item)?;
        rows.collect()
    }

    pub fn move_item_to_folder(&self, item_id: i64, folder_id: Option<i64>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE items SET folder_id = ?1 WHERE id = ?2",
            params![folder_id, item_id],
        )?;
        Ok(())
    }

    pub fn delete_item(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM items WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn check_duplicate(&self, content: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id
             FROM items
             WHERE folder_id IS NULL AND content_data = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query(params![content])?;

        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn insert_image_item(&self, description: &str, rgba_data: &[u8]) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO items (folder_id, content_type, content_data, content_blob, created_at)
             VALUES (NULL, 'image', ?1, ?2, ?3)",
            params![description, rgba_data, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn check_image_duplicate(&self, description: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id
             FROM items
             WHERE folder_id IS NULL
               AND content_type = 'image'
               AND content_data = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query(params![description])?;

        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn get_item_blob(&self, item_id: i64) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT content_blob FROM items WHERE id = ?1 LIMIT 1")?;
        let mut rows = stmt.query(params![item_id])?;

        match rows.next()? {
            Some(row) => row.get(0),
            None => Ok(None),
        }
    }

    pub fn get_folders(&self) -> Result<Vec<Folder>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, parent_id, name, created_at
             FROM folders
             ORDER BY name",
        )?;
        let rows = stmt.query_map([], row_to_folder)?;
        rows.collect()
    }

    pub fn create_folder(&self, name: &str, parent_id: Option<i64>) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO folders (parent_id, name, created_at)
             VALUES (?1, ?2, ?3)",
            params![parent_id, name, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_folder(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM folders WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn rename_folder(&self, id: i64, new_name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE folders SET name = ?1 WHERE id = ?2",
            params![new_name, id],
        )?;
        Ok(())
    }
}

fn data_dir() -> anyhow::Result<std::path::PathBuf> {
    let dirs = ProjectDirs::from("com", "jubako", "Jubako")
        .context("Failed to determine project data directory")?;
    Ok(dirs.data_dir().to_path_buf())
}

fn initialize_connection(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch("PRAGMA journal_mode = WAL;")
        .context("Failed to enable WAL mode")?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .context("Failed to enable foreign keys")?;
    conn.execute_batch(CREATE_SCHEMA_SQL)
        .context("Failed to create tables")?;

    run_migrations(conn);

    Ok(())
}

fn run_migrations(conn: &Connection) {
    let _ = conn.execute_batch(ADD_CONTENT_BLOB_MIGRATION_SQL);
}

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<Item> {
    Ok(Item {
        id: row.get(0)?,
        content_type: row.get(2)?,
        content_data: row.get(3)?,
        label: row.get(4)?,
        is_favorite: row.get::<_, i64>(5)? != 0,
    })
}

fn row_to_folder(row: &rusqlite::Row<'_>) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        name: row.get(2)?,
    })
}
