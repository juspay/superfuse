use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum SuperfuseError {

    #[error("could not determine data directory for superfuse")]
    NoDataDir,

    #[error("failed to create directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("mapping not found for path: {0}")]
    MappingNotFound(String),

    #[error("mapping already exists for path: {0}")]
    MappingAlreadyExists(String),

    #[error("template file not found: {0}")]
    TemplateNotFound(PathBuf),

    #[error("template error: {0}")]
    Template(#[from] handlebars::RenderError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    // #[error("superposition provider error: {0}")]
    // Provider(String),
}
