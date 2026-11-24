use anyhow::Context;
use chrono::Utc;
use directories::ProjectDirs;
use rusqlite::{params, Connection, Result};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

const CREATE_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS folders (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id        INTEGER REFERENCES folders(id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    created_at       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS items (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    folder_id        INTEGER REFERENCES folders(id) ON DELETE CASCADE,
    content_type     TEXT NOT NULL,
    content_data     TEXT NOT NULL,
    content_hash     INTEGER,
    content_blob     BLOB,
    thumbnail_blob   BLOB,
    thumbnail_width  INTEGER,
    thumbnail_height INTEGER,
    label            TEXT,
    created_at       TEXT NOT NULL,
    is_favorite      INTEGER NOT NULL DEFAULT 0
);
";

const ADD_CONTENT_BLOB_MIGRATION_SQL: &str = "ALTER TABLE items ADD COLUMN content_blob BLOB";
const ADD_CONTENT_HASH_MIGRATION_SQL: &str = "ALTER TABLE items ADD COLUMN content_hash INTEGER";
const ADD_THUMBNAIL_BLOB_MIGRATION_SQL: &str = "ALTER TABLE items ADD COLUMN thumbnail_blob BLOB";
const ADD_THUMBNAIL_WIDTH_MIGRATION_SQL: &str =
    "ALTER TABLE items ADD COLUMN thumbnail_width INTEGER";
const ADD_THUMBNAIL_HEIGHT_MIGRATION_SQL: &str =
    "ALTER TABLE items ADD COLUMN thumbnail_height INTEGER";

const CREATE_INDEXES_SQL: &str = "
CREATE INDEX IF NOT EXISTS idx_items_folder_created
    ON items(folder_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_items_dup_hash
    ON items(folder_id, content_type, content_hash);
";

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
        let hash = hash_str(content);
        let mut stmt = conn.prepare_cached(
            "INSERT INTO items (folder_id, content_type, content_data, content_hash, created_at)
             VALUES (NULL, ?1, ?2, ?3, ?4)",
        )?;
        stmt.execute(params![content_type, content, hash, now])?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_history(&self, limit: usize) -> Result<Vec<Item>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(&format!(
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
        let mut stmt = conn.prepare_cached(&format!(
            "{SELECT_ITEM_COLUMNS}
             WHERE folder_id = ?1
             ORDER BY created_at DESC"
        ))?;
        let rows = stmt.query_map(params![folder_id], row_to_item)?;
        rows.collect()
    }

    pub fn move_item_to_folder(&self, item_id: i64, folder_id: Option<i64>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare_cached("UPDATE items SET folder_id = ?1 WHERE id = ?2")?;
        stmt.execute(params![folder_id, item_id])?;
        Ok(())
    }

    pub fn delete_item(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached("DELETE FROM items WHERE id = ?1")?;
        stmt.execute(params![id])?;
        Ok(())
    }

    pub fn check_duplicate(&self, content: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let hash = hash_str(content);
        let mut stmt = conn.prepare_cached(
            "SELECT id
             FROM items
             WHERE folder_id IS NULL
               AND content_type = 'text'
               AND content_hash = ?1
               AND content_data = ?2
             LIMIT 1",
        )?;
        let mut rows = stmt.query(params![hash, content])?;

        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn insert_image_item(
        &self,
        description: &str,
        rgba_data: &[u8],
        thumb_data: &[u8],
        thumb_width: usize,
        thumb_height: usize,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let hash = hash_str(description);
        let mut stmt = conn.prepare_cached(
            "INSERT INTO items (
                folder_id, content_type, content_data, content_hash,
                content_blob, thumbnail_blob, thumbnail_width, thumbnail_height, created_at
             )
             VALUES (NULL, 'image', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        stmt.execute(params![
            description,
            hash,
            rgba_data,
            thumb_data,
            thumb_width as i64,
            thumb_height as i64,
            now
        ])?;
        Ok(conn.last_insert_rowid())
    }

    pub fn check_image_duplicate(&self, description: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let hash = hash_str(description);
        let mut stmt = conn.prepare_cached(
            "SELECT id
             FROM items
             WHERE folder_id IS NULL
               AND content_type = 'image'
               AND content_hash = ?1
               AND content_data = ?2
             LIMIT 1",
        )?;
        let mut rows = stmt.query(params![hash, description])?;

        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn get_item_blob(&self, item_id: i64) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare_cached("SELECT content_blob FROM items WHERE id = ?1 LIMIT 1")?;
        let mut rows = stmt.query(params![item_id])?;

        match rows.next()? {
            Some(row) => row.get(0),
            None => Ok(None),
        }
    }

    pub fn get_item_thumbnail(&self, item_id: i64) -> Result<Option<(Vec<u8>, usize, usize)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT thumbnail_blob, thumbnail_width, thumbnail_height
             FROM items
             WHERE id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query(params![item_id])?;

        match rows.next()? {
            Some(row) => {
                let blob: Option<Vec<u8>> = row.get(0)?;
                let w: Option<i64> = row.get(1)?;
                let h: Option<i64> = row.get(2)?;
                match (blob, w, h) {
                    (Some(b), Some(w), Some(h)) if w > 0 && h > 0 => {
                        Ok(Some((b, w as usize, h as usize)))
                    }
                    _ => Ok(None),
                }
            }
            None => Ok(None),
        }
    }

    pub fn get_folders(&self) -> Result<Vec<Folder>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
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
        let mut stmt = conn.prepare_cached(
            "INSERT INTO folders (parent_id, name, created_at)
             VALUES (?1, ?2, ?3)",
        )?;
        stmt.execute(params![parent_id, name, now])?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_folder(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached("DELETE FROM folders WHERE id = ?1")?;
        stmt.execute(params![id])?;
        Ok(())
    }

    pub fn rename_folder(&self, id: i64, new_name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached("UPDATE folders SET name = ?1 WHERE id = ?2")?;
        stmt.execute(params![new_name, id])?;
        Ok(())
    }
}

fn hash_str(value: &str) -> i64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish() as i64
}

fn data_dir() -> anyhow::Result<std::path::PathBuf> {
    let dirs = ProjectDirs::from("com", "jubako", "Jubako")
        .context("Failed to determine project data directory")?;
    Ok(dirs.data_dir().to_path_buf())
}

fn initialize_connection(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
        .context("Failed to configure WAL mode")?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .context("Failed to enable foreign keys")?;
    conn.execute_batch(CREATE_SCHEMA_SQL)
        .context("Failed to create tables")?;

    run_migrations(conn);

    conn.execute_batch(CREATE_INDEXES_SQL)
        .context("Failed to create indexes")?;

    Ok(())
}

fn run_migrations(conn: &Connection) {
    let _ = conn.execute_batch(ADD_CONTENT_BLOB_MIGRATION_SQL);
    let _ = conn.execute_batch(ADD_CONTENT_HASH_MIGRATION_SQL);
    let _ = conn.execute_batch(ADD_THUMBNAIL_BLOB_MIGRATION_SQL);
    let _ = conn.execute_batch(ADD_THUMBNAIL_WIDTH_MIGRATION_SQL);
    let _ = conn.execute_batch(ADD_THUMBNAIL_HEIGHT_MIGRATION_SQL);
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
