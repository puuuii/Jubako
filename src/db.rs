use anyhow::Context;
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use rusqlite::{params, Connection, Result};
use std::sync::Mutex;

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
    pub created_at: DateTime<Utc>,
    pub is_favorite: bool,
    pub content_blob: Option<Vec<u8>>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db")
            .field("conn", &"Mutex<Connection>")
            .finish()
    }
}

fn parse_datetime(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<Item> {
    Ok(Item {
        id: row.get(0)?,
        content_type: row.get(2)?,
        content_data: row.get(3)?,
        label: row.get(4)?,
        created_at: parse_datetime(&row.get::<_, String>(5)?),
        is_favorite: row.get::<_, i64>(6)? != 0,
        content_blob: row.get(7)?,
    })
}

fn row_to_folder(row: &rusqlite::Row<'_>) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        name: row.get(2)?,
    })
}

impl Db {
    /// Create a new Db instance. The database file is stored in the user's
    /// application data directory as determined by the `directories` crate.
    /// Tables are created if they do not already exist. WAL mode and foreign
    /// keys are enabled on the connection.
    pub fn new() -> anyhow::Result<Self> {
        let proj_dirs = ProjectDirs::from("com", "jubako", "Jubako")
            .context("Failed to determine project data directory")?;

        let data_dir = proj_dirs.data_dir();
        std::fs::create_dir_all(data_dir)
            .with_context(|| format!("Failed to create data directory: {}", data_dir.display()))?;

        let db_path = data_dir.join("jubako.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

        // Enable WAL mode for better concurrent read performance.
        conn.execute_batch("PRAGMA journal_mode = WAL;")
            .context("Failed to enable WAL mode")?;

        // Enable foreign key constraint enforcement.
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .context("Failed to enable foreign keys")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS folders (
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
            );",
        )
        .context("Failed to create tables")?;

        // Migration: add content_blob column if it doesn't already exist.
        Self::migrate_add_content_blob(&conn);

        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    /// Attempt to add the `content_blob` column. Silently ignores the error
    /// when the column already exists (i.e. duplicate column name).
    fn migrate_add_content_blob(conn: &Connection) {
        let _ = conn.execute_batch("ALTER TABLE items ADD COLUMN content_blob BLOB");
    }

    // ───────────────────────── Item operations ─────────────────────────

    /// Insert a new clipboard item with no folder (goes into history).
    /// Returns the new item's row id.
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

    /// Get history items (items where folder_id IS NULL), most recent first.
    pub fn get_history(&self, limit: usize) -> Result<Vec<Item>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, folder_id, content_type, content_data, label, created_at, is_favorite, content_blob
             FROM items
             WHERE folder_id IS NULL
             ORDER BY created_at DESC
             LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit as i64], row_to_item)?;
        rows.collect()
    }

    /// Get all items that belong to a specific folder.
    pub fn get_items_in_folder(&self, folder_id: i64) -> Result<Vec<Item>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, folder_id, content_type, content_data, label, created_at, is_favorite, content_blob
             FROM items
             WHERE folder_id = ?1
             ORDER BY created_at DESC",
        )?;

        let rows = stmt.query_map(params![folder_id], row_to_item)?;
        rows.collect()
    }

    /// Move an item into a folder, or back to history when `folder_id` is None.
    pub fn move_item_to_folder(&self, item_id: i64, folder_id: Option<i64>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE items SET folder_id = ?1 WHERE id = ?2",
            params![folder_id, item_id],
        )?;
        Ok(())
    }

    /// Delete an item by id.
    pub fn delete_item(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM items WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Search items whose `content_data` or `label` matches the query
    /// (case-insensitive LIKE). Results are ordered by most recent first.
    pub fn search_items(&self, query: &str, limit: usize) -> Result<Vec<Item>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT id, folder_id, content_type, content_data, label, created_at, is_favorite, content_blob
             FROM items
             WHERE content_data LIKE ?1 OR label LIKE ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![pattern, limit as i64], row_to_item)?;
        rows.collect()
    }

    /// Check if an item with the exact same content already exists in the
    /// history (folder_id IS NULL). Returns `Some(id)` if a duplicate is
    /// found, `None` otherwise.
    pub fn check_duplicate(&self, content: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id FROM items
             WHERE folder_id IS NULL AND content_data = ?1
             LIMIT 1",
        )?;

        let mut rows = stmt.query(params![content])?;
        match rows.next()? {
            Some(row) => {
                let id: i64 = row.get(0)?;
                Ok(Some(id))
            }
            None => Ok(None),
        }
    }

    /// Insert a new image clipboard item with no folder (goes into history).
    /// `description` is a string stored in `content_data` (e.g. dimensions and hash).
    /// `rgba_data` is the raw RGBA pixel data stored in `content_blob`.
    /// Returns the new item's row id.
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

    /// Check if an image item with the exact same description already exists
    /// in the history (folder_id IS NULL, content_type = 'image').
    /// Returns `Some(id)` if a duplicate is found, `None` otherwise.
    pub fn check_image_duplicate(&self, description: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id FROM items WHERE folder_id IS NULL AND content_type = 'image' AND content_data = ?1 LIMIT 1",
        )?;
        let mut rows = stmt.query(params![description])?;
        match rows.next()? {
            Some(row) => {
                let id: i64 = row.get(0)?;
                Ok(Some(id))
            }
            None => Ok(None),
        }
    }

    // ──────────────────────── Folder operations ────────────────────────

    /// Get all folders, ordered by name.
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

    /// Create a new folder. Returns the new folder's row id.
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

    /// Delete a folder by id. Because of ON DELETE CASCADE, all items that
    /// belong to this folder (and any child folders) will also be deleted.
    pub fn delete_folder(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM folders WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Rename a folder.
    pub fn rename_folder(&self, id: i64, new_name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE folders SET name = ?1 WHERE id = ?2",
            params![new_name, id],
        )?;
        Ok(())
    }
}
