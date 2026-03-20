use std::{sync::Arc, time::Duration};

use libc::{getgid, getuid};

use chrono::NaiveDateTime;
use fuser::{FileAttr, FileType, Filesystem, Generation, INodeNo};
use handlebars::Handlebars;
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use superposition_provider::{EvaluationContext, SuperpositionProvider};
use tokio::runtime::Handle;
use tracing::{debug, error, info, trace};

use crate::{config::DataPaths, error::SuperfuseError};

/// A file mapping from a virtual path to a template file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMapping {
    pub id: i64,
    pub virtual_path: String,
    pub template_path: String,
    pub created_at: String,
    pub last_modified_at: String,
    pub last_accessed_at: String,
}

/// Type alias for the SQLite connection pool.
pub type SqlitePool = Pool<SqliteConnectionManager>;

/// Type alias for a pooled SQLite connection.
pub type SqliteConn = PooledConnection<SqliteConnectionManager>;

/// Initialize the SQLite database and create the mappings table.
pub fn init_db(db_path: &std::path::Path) -> Result<SqlitePool, SuperfuseError> {
    let db_path_str = db_path.display().to_string();
    trace!(path = %db_path_str, "connecting to database");

    let manager = SqliteConnectionManager::file(db_path);
    let pool = Pool::builder()
        .max_size(1)
        .build(manager)
        .map_err(SuperfuseError::Pool)?;

    // Initialize the schema using a connection from the pool
    let conn = pool.get().map_err(|e| {
        error!(error = %e, "failed to get connection from pool");
        SuperfuseError::Pool(e)
    })?;

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS mappings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            virtual_path TEXT NOT NULL UNIQUE,
            template_path TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            last_modified_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            last_accessed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )?;

    info!("database initialized at {}", db_path_str);
    Ok(pool)
}

/// Add a new file mapping.
pub fn add_mapping(
    conn: &SqliteConn,
    virtual_path: &str,
    template_path: &str,
) -> Result<FileMapping, SuperfuseError> {
    debug!(virtual_path = %virtual_path, template_path = %template_path, "adding mapping");

    conn.execute(
        "INSERT INTO mappings (virtual_path, template_path) VALUES (?1, ?2)",
        params![virtual_path, template_path],
    )
    .map_err(|e| {
        if let rusqlite::Error::SqliteFailure(ref sql_err, ref msg) = e {
            if sql_err.code == rusqlite::ErrorCode::ConstraintViolation {
                if let Some(m) = msg {
                    if m.contains("UNIQUE") {
                        return SuperfuseError::MappingAlreadyExists(virtual_path.to_string());
                    }
                }
            }
        }
        SuperfuseError::Database(e)
    })?;

    let id = conn.last_insert_rowid();

    let mapping = conn.query_row("SELECT * FROM mappings WHERE id = ?1", params![id], |row| {
        Ok(FileMapping {
            id: row.get(0)?,
            virtual_path: row.get(1)?,
            template_path: row.get(2)?,
            created_at: row.get(3)?,
            last_modified_at: row.get(4)?,
            last_accessed_at: row.get(5)?,
        })
    })?;

    info!(virtual_path = %mapping.virtual_path, "mapping added");
    Ok(mapping)
}

/// Remove a file mapping by virtual path.
pub fn remove_mapping(conn: &SqliteConn, virtual_path: &str) -> Result<(), SuperfuseError> {
    debug!(virtual_path = %virtual_path, "removing mapping");

    let rows_affected = conn.execute(
        "DELETE FROM mappings WHERE virtual_path = ?1",
        params![virtual_path],
    )?;

    if rows_affected == 0 {
        return Err(SuperfuseError::MappingNotFound(virtual_path.to_string()));
    }

    info!(virtual_path = %virtual_path, "mapping removed");
    Ok(())
}

/// Update an existing file mapping's template path.
pub fn update_mapping(
    conn: &SqliteConn,
    virtual_path: &str,
    new_template_path: &str,
) -> Result<FileMapping, SuperfuseError> {
    debug!(virtual_path = %virtual_path, new_template_path = %new_template_path, "updating mapping");

    let rows_affected = conn.execute(
        "UPDATE mappings SET template_path = ?1, last_modified_at = CURRENT_TIMESTAMP WHERE virtual_path = ?2",
        params![new_template_path, virtual_path],
    )?;

    if rows_affected == 0 {
        return Err(SuperfuseError::MappingNotFound(virtual_path.to_string()));
    }

    let mapping = conn.query_row(
        "SELECT * FROM mappings WHERE virtual_path = ?1",
        params![virtual_path],
        |row| {
            Ok(FileMapping {
                id: row.get(0)?,
                virtual_path: row.get(1)?,
                template_path: row.get(2)?,
                created_at: row.get(3)?,
                last_modified_at: row.get(4)?,
                last_accessed_at: row.get(5)?,
            })
        },
    )?;

    info!(virtual_path = %mapping.virtual_path, "mapping updated");
    Ok(mapping)
}

/// List all file mappings.
pub fn list_mappings(conn: &SqliteConn) -> Result<Vec<FileMapping>, SuperfuseError> {
    trace!("listing all mappings");

    let mut stmt = conn.prepare("SELECT * FROM mappings ORDER BY virtual_path")?;

    let mappings = stmt
        .query_map([], |row| {
            Ok(FileMapping {
                id: row.get(0)?,
                virtual_path: row.get(1)?,
                template_path: row.get(2)?,
                created_at: row.get(3)?,
                last_modified_at: row.get(4)?,
                last_accessed_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    debug!(count = mappings.len(), "retrieved mappings");
    Ok(mappings)
}

/// Look up a mapping by virtual path.
pub fn get_mapping_by_virtual_path(
    conn: &SqliteConn,
    virtual_path: &str,
) -> Result<FileMapping, SuperfuseError> {
    trace!(virtual_path = %virtual_path, "looking up mapping");

    let result = conn.query_row(
        "SELECT * FROM mappings WHERE virtual_path = ?1",
        params![virtual_path],
        |row| {
            Ok(FileMapping {
                id: row.get(0)?,
                virtual_path: row.get(1)?,
                template_path: row.get(2)?,
                created_at: row.get(3)?,
                last_modified_at: row.get(4)?,
                last_accessed_at: row.get(5)?,
            })
        },
    );

    match result {
        Ok(mapping) => Ok(mapping),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            Err(SuperfuseError::MappingNotFound(virtual_path.to_string()))
        }
        Err(e) => Err(SuperfuseError::Database(e)),
    }
}

/// Look up a mapping by its row id.
pub fn get_mapping_by_id(conn: &SqliteConn, id: i64) -> Result<FileMapping, SuperfuseError> {
    trace!(id = id, "looking up mapping by id");

    let result = conn.query_row("SELECT * FROM mappings WHERE id = ?1", params![id], |row| {
        Ok(FileMapping {
            id: row.get(0)?,
            virtual_path: row.get(1)?,
            template_path: row.get(2)?,
            created_at: row.get(3)?,
            last_modified_at: row.get(4)?,
            last_accessed_at: row.get(5)?,
        })
    });

    match result {
        Ok(mapping) => Ok(mapping),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            Err(SuperfuseError::MappingNotFound(id.to_string()))
        }
        Err(e) => Err(SuperfuseError::Database(e)),
    }
}

/// FUSE filesystem implementation.
#[derive(Debug)]
pub struct SuperfuseFileSystem {
    pool: Arc<SqlitePool>,
    provider: Arc<SuperpositionProvider>,
    runtime: Handle,
}

impl SuperfuseFileSystem {
    pub fn new(pool: SqlitePool, _paths: DataPaths, provider: SuperpositionProvider) -> Self {
        Self {
            pool: Arc::new(pool),
            // paths: Arc::new(paths),
            provider: Arc::new(provider),
            runtime: Handle::current(),
        }
    }
}

const FILE_SIZE_TTL: Duration = Duration::from_secs(1);
const SQLITE_TIMESTAMP_FMT: &str = "%Y-%m-%d %H:%M:%S";

fn parse_sqlite_timestamp(s: &str) -> std::time::SystemTime {
    let dt = NaiveDateTime::parse_from_str(s, SQLITE_TIMESTAMP_FMT)
        .unwrap_or_else(|e| panic!("Could not parse timestamp '{}': {}", s, e));
    std::time::UNIX_EPOCH + std::time::Duration::from_secs(dt.and_utc().timestamp() as u64)
}

fn current_uid() -> u32 {
    unsafe { getuid() }
}

fn current_gid() -> u32 {
    unsafe { getgid() }
}

fn superfuse_dir_attr() -> FileAttr {
    FileAttr {
        ino: INodeNo::ROOT,
        size: 0,
        blocks: 0,
        atime: std::time::UNIX_EPOCH,
        mtime: std::time::UNIX_EPOCH,
        ctime: std::time::UNIX_EPOCH,
        crtime: std::time::UNIX_EPOCH,
        kind: FileType::Directory,
        perm: 0o744,
        nlink: 2,
        uid: current_uid(),
        gid: current_gid(),
        rdev: 0,
        flags: 0,
        blksize: 512,
    }
}

impl Filesystem for SuperfuseFileSystem {
    fn lookup(
        &self,
        _req: &fuser::Request,
        parent: fuser::INodeNo,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEntry,
    ) {
        if u64::from(parent) != u64::from(INodeNo::ROOT) {
            return reply.error(fuser::Errno::ENOENT);
        }

        let Some(file_name) = name.to_str() else {
            return reply.error(fuser::Errno::ENOENT);
        };

        let Ok(conn) = self
            .pool
            .get()
            .map_err(|e| error!(error = %e, "failed to get connection from pool"))
        else {
            return reply.error(fuser::Errno::EIO);
        };

        let Ok(mapping) = get_mapping_by_virtual_path(&conn, file_name) else {
            return reply.error(fuser::Errno::EIO);
        };
        let ino = mapping.id as u64 + 1;
        let last_modified = parse_sqlite_timestamp(&mapping.last_modified_at);
        let attr = FileAttr {
            ino: INodeNo(ino),
            size: 512, // Placeholder size, could be dynamic based on template file
            blocks: 1,
            atime: parse_sqlite_timestamp(&mapping.last_accessed_at),
            mtime: last_modified,
            ctime: last_modified,
            crtime: parse_sqlite_timestamp(&mapping.created_at),
            kind: FileType::RegularFile,
            perm: 0o444,
            nlink: 1,
            uid: current_uid(),
            gid: current_gid(),
            rdev: 0,
            blksize: 4096,
            flags: 0,
        };
        reply.entry(&FILE_SIZE_TTL, &attr, Generation(0));
    }

    fn getattr(
        &self,
        _req: &fuser::Request,
        ino: fuser::INodeNo,
        _fh: Option<fuser::FileHandle>,
        reply: fuser::ReplyAttr,
    ) {
        match ino {
            INodeNo::ROOT => reply.attr(&FILE_SIZE_TTL, &superfuse_dir_attr()),
            id => {
                let Ok(conn) = self
                    .pool
                    .get()
                    .map_err(|e| error!(error = %e, "failed to get connection from pool"))
                else {
                    return reply.error(fuser::Errno::EIO);
                };
                let Ok(mapping) = get_mapping_by_id(&conn, u64::from(id) as i64 - 1) else {
                    return reply.error(fuser::Errno::EIO);
                };
                let last_modified = parse_sqlite_timestamp(&mapping.last_modified_at);
                reply.attr(
                    &FILE_SIZE_TTL,
                    &FileAttr {
                        ino: id,
                        size: 512, // Placeholder size, could be dynamic based on template file
                        blocks: 1,
                        atime: parse_sqlite_timestamp(&mapping.last_accessed_at),
                        mtime: last_modified,
                        ctime: last_modified,
                        crtime: parse_sqlite_timestamp(&mapping.created_at),
                        kind: FileType::RegularFile,
                        perm: 0o444,
                        nlink: 1,
                        uid: current_uid(),
                        gid: current_gid(),
                        rdev: 0,
                        blksize: 4096,
                        flags: 0,
                    },
                );
            }
        }
    }

    fn read(
        &self,
        _req: &fuser::Request,
        ino: fuser::INodeNo,
        _fh: fuser::FileHandle,
        offset: u64,
        _size: u32,
        _flags: fuser::OpenFlags,
        _lock_owner: Option<fuser::LockOwner>,
        reply: fuser::ReplyData,
    ) {
        let Ok(conn) = self
            .pool
            .get()
            .map_err(|e| error!(error = %e, "failed to get connection from pool"))
        else {
            return reply.error(fuser::Errno::EIO);
        };
        let id = u64::from(ino) as i64 - 1;
        if id <= 0 {
            return reply.error(fuser::Errno::ENOENT);
        }
        let Ok(mappings) = get_mapping_by_id(&conn, id) else {
            return reply.error(fuser::Errno::EIO);
        };
        let Ok(config) = self
            .runtime
            .block_on(
                self.provider
                    .resolve_full_config(&EvaluationContext::default()),
            )
            .map_err(|e| error!("Could not read from superposition provider: {}", e))
        else {
            return reply.error(fuser::Errno::EIO);
        };
        let Ok(template) = std::fs::read_to_string(&mappings.template_path).map_err(|e| {
            error!(
                "Could not read file template for: {} due to {}",
                mappings.virtual_path, e
            )
        }) else {
            return reply.error(fuser::Errno::EIO);
        };
        let handlebars = Handlebars::new();
        let Ok(data) = handlebars.render_template(&template, &config).map_err(|e| {
            error!(
                "Could not render template for: {} due to {}",
                mappings.virtual_path, e
            )
        }) else {
            return reply.error(fuser::Errno::EIO);
        };
        let data_bytes = data.into_bytes();
        if offset as usize >= data_bytes.len() {
            reply.data(&[]);
        } else {
            reply.data(&data_bytes[offset as usize..]);
        }
    }

    fn readdir(
        &self,
        _req: &fuser::Request,
        ino: fuser::INodeNo,
        _fh: fuser::FileHandle,
        offset: u64,
        mut reply: fuser::ReplyDirectory,
    ) {
        if u64::from(ino) != 1 {
            return reply.error(fuser::Errno::ENOENT);
        }
        let Ok(conn) = self
            .pool
            .get()
            .map_err(|e| error!(error = %e, "failed to get connection from pool"))
        else {
            return reply.error(fuser::Errno::EIO);
        };
        let Ok(entries) = list_mappings(&conn) else {
            return reply.error(fuser::Errno::EIO);
        };

        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
            // i + 1 means the index of the next entry
            if reply.add(
                INodeNo(entry.id as u64),
                (i + 1) as u64,
                FileType::RegularFile,
                entry.virtual_path,
            ) {
                break;
            }
        }
        reply.ok();
    }
}
