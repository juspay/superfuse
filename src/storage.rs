use fuser::Filesystem;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};
use superposition_provider::SuperpositionProvider;
use tracing::{debug, info, trace, warn};

use crate::{config::DataPaths, error::SuperfuseError};

/// A file mapping from a virtual path to a template file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMapping {
    pub id: i64,
    pub virtual_path: String,
    pub template_path: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Initialize the SQLite database and create the mappings table.
pub async fn init_db(db_path: &std::path::Path) -> Result<SqlitePool, SuperfuseError> {
    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
    trace!(url = %db_url, "connecting to database");

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&db_url)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS mappings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            virtual_path TEXT NOT NULL UNIQUE,
            template_path TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&pool)
    .await?;

    info!("database initialized");
    Ok(pool)
}

/// Add a new file mapping.
pub async fn add_mapping(
    pool: &SqlitePool,
    virtual_path: &str,
    template_path: &str,
) -> Result<FileMapping, SuperfuseError> {
    debug!(virtual_path = %virtual_path, template_path = %template_path, "adding mapping");

    let result = sqlx::query(
        r#"
        INSERT INTO mappings (virtual_path, template_path)
        VALUES (?, ?)
        RETURNING id, virtual_path, template_path, created_at, updated_at
        "#,
    )
    .bind(virtual_path)
    .bind(template_path)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.message().contains("UNIQUE constraint") {
                return SuperfuseError::MappingAlreadyExists(virtual_path.to_string());
            }
        }
        SuperfuseError::Database(e)
    })?;

    let mapping = FileMapping {
        id: result.get("id"),
        virtual_path: result.get("virtual_path"),
        template_path: result.get("template_path"),
        created_at: result.get("created_at"),
        updated_at: result.get("updated_at"),
    };

    info!(virtual_path = %mapping.virtual_path, "mapping added");
    Ok(mapping)
}

/// Remove a file mapping by virtual path.
pub async fn remove_mapping(pool: &SqlitePool, virtual_path: &str) -> Result<(), SuperfuseError> {
    debug!(virtual_path = %virtual_path, "removing mapping");

    let result = sqlx::query("DELETE FROM mappings WHERE virtual_path = ?")
        .bind(virtual_path)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(SuperfuseError::MappingNotFound(virtual_path.to_string()));
    }

    info!(virtual_path = %virtual_path, "mapping removed");
    Ok(())
}

/// Update an existing file mapping's template path.
pub async fn update_mapping(
    pool: &SqlitePool,
    virtual_path: &str,
    new_template_path: &str,
) -> Result<FileMapping, SuperfuseError> {
    debug!(virtual_path = %virtual_path, new_template_path = %new_template_path, "updating mapping");

    let result = sqlx::query(
        r#"
        UPDATE mappings
        SET template_path = ?, updated_at = CURRENT_TIMESTAMP
        WHERE virtual_path = ?
        RETURNING id, virtual_path, template_path, created_at, updated_at
        "#,
    )
    .bind(new_template_path)
    .bind(virtual_path)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if matches!(e, sqlx::Error::RowNotFound) {
            SuperfuseError::MappingNotFound(virtual_path.to_string())
        } else {
            SuperfuseError::Database(e)
        }
    })?;

    let mapping = FileMapping {
        id: result.get("id"),
        virtual_path: result.get("virtual_path"),
        template_path: result.get("template_path"),
        created_at: result.get("created_at"),
        updated_at: result.get("updated_at"),
    };

    info!(virtual_path = %mapping.virtual_path, "mapping updated");
    Ok(mapping)
}

/// List all file mappings.
pub async fn list_mappings(pool: &SqlitePool) -> Result<Vec<FileMapping>, SuperfuseError> {
    trace!("listing all mappings");

    let rows = sqlx::query(
        r#"
        SELECT id, virtual_path, template_path, created_at, updated_at
        FROM mappings
        ORDER BY virtual_path
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mappings: Vec<FileMapping> = rows
        .into_iter()
        .map(|row| FileMapping {
            id: row.get("id"),
            virtual_path: row.get("virtual_path"),
            template_path: row.get("template_path"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
        .collect();

    debug!(count = mappings.len(), "retrieved mappings");
    Ok(mappings)
}

pub struct SuperfuseFileSystem {
    pool: SqlitePool,
    paths: DataPaths,
    provider: SuperpositionProvider,
}

impl Filesystem for SuperfuseFileSystem {
    fn lookup(
        &self,
        _req: &fuser::Request,
        parent: fuser::INodeNo,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEntry,
    ) {
        warn!("[Not Implemented] lookup(parent: {parent:#x?}, name {name:?})");
        reply.error(fuser::Errno::ENOSYS);
    }

    fn getattr(
        &self,
        _req: &fuser::Request,
        ino: fuser::INodeNo,
        fh: Option<fuser::FileHandle>,
        reply: fuser::ReplyAttr,
    ) {
        warn!("[Not Implemented] getattr(ino: {ino:#x?}, fh: {fh:#x?})");
        reply.error(fuser::Errno::ENOSYS);
    }

    fn open(
        &self,
        _req: &fuser::Request,
        _ino: fuser::INodeNo,
        _flags: fuser::OpenFlags,
        reply: fuser::ReplyOpen,
    ) {
        reply.opened(fuser::FileHandle(0), fuser::FopenFlags::empty());
    }

    fn read(
        &self,
        _req: &fuser::Request,
        ino: fuser::INodeNo,
        fh: fuser::FileHandle,
        offset: u64,
        size: u32,
        flags: fuser::OpenFlags,
        lock_owner: Option<fuser::LockOwner>,
        reply: fuser::ReplyData,
    ) {
        warn!(
            "[Not Implemented] read(ino: {ino:#x?}, fh: {fh}, offset: {offset}, \
            size: {size}, flags: {flags:#x?}, lock_owner: {lock_owner:?})"
        );
        reply.error(fuser::Errno::ENOSYS);
    }

    fn readdir(
        &self,
        _req: &fuser::Request,
        ino: fuser::INodeNo,
        fh: fuser::FileHandle,
        offset: u64,
        reply: fuser::ReplyDirectory,
    ) {
        warn!("[Not Implemented] readdir(ino: {ino:#x?}, fh: {fh}, offset: {offset})");
        reply.error(fuser::Errno::ENOSYS);
    }
}
