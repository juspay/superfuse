use std::{
    collections::HashMap,
    fmt::Display,
    path::{Path, PathBuf},
    str::FromStr,
};

use directories::ProjectDirs;
use fuser::SessionACL;
use superposition_provider::{SuperpositionProvider, SuperpositionProviderOptions};
use tracing::{debug, info, warn};

use crate::error::SuperfuseError;

/// Superposition connection configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct SuperpositionConfig {
    pub endpoint: String,
    pub token: String,
    pub org_id: String,
    pub workspace_id: String,
    pub additional_headers: HashMap<String, String>,
}

impl SuperpositionConfig {
    pub fn init() -> Result<Self, SuperfuseError> {
        let cfg = SuperpositionConfig {
            endpoint: require_env(
                "SUPERPOSITION_ENDPOINT",
                "http://localhost:8080".to_string(),
            ),
            token: require_env("SUPERPOSITION_TOKEN", "123456".to_string()),
            org_id: require_env("SUPERPOSITION_ORG_ID", "localorg".into()),
            workspace_id: require_env("SUPERPOSITION_WORKSPACE_ID", "localworkspace".into()),
            additional_headers: HashMap::new(),
        };
        debug!(
            endpoint = %cfg.endpoint,
            org_id = %cfg.org_id,
            workspace_id = %cfg.workspace_id,
            additional_headers = ?cfg.additional_headers,
            "loaded superposition config"
        );
        Ok(cfg)
    }
}

/// Resolved paths for the superfuse data directory.
#[derive(Debug, Clone)]
pub struct DataPaths {
    // pub root: PathBuf,
    pub db: PathBuf,
    pub logs: PathBuf,
}

impl DataPaths {
    /// Resolve and create the data directory tree.
    pub fn init() -> Result<Self, SuperfuseError> {
        let proj =
            ProjectDirs::from("com", "juspay", "superfuse").ok_or(SuperfuseError::NoDataDir)?;

        let root = proj.data_dir().to_path_buf();
        let db = root.join("superfuse.db");
        let logs = root.join("logs");

        for dir in [&root, &logs] {
            ensure_dir(dir)?;
        }

        info!(path = %root.display(), "data directory ready");
        debug!(db = %db.display(), logs = %logs.display(), "resolved data paths");

        Ok(Self { db, logs })
    }
}

fn ensure_dir(path: &Path) -> Result<(), SuperfuseError> {
    if !path.exists() {
        std::fs::create_dir_all(path).map_err(|source| SuperfuseError::CreateDir {
            path: path.to_path_buf(),
            source,
        })?;
        debug!(path = %path.display(), "created directory");
    }
    Ok(())
}

pub fn require_env<T>(key: &'static str, default: T) -> T
where
    T: FromStr + Display + std::fmt::Debug,
    <T as FromStr>::Err: std::fmt::Debug + std::error::Error,
{
    std::env::var(key)
        .map_err(|_| {
            warn!("No environment variable found for {key}, using default value: {default}")
        })
        .ok()
        .and_then(|val| val.parse::<T>().ok())
        .unwrap_or(default)
}

pub async fn init_superposition_provider(
    config: &SuperpositionConfig,
) -> Result<SuperpositionProvider, SuperfuseError> {
    let options = SuperpositionProviderOptions {
        endpoint: config.endpoint.clone(),
        token: config.token.clone(),
        org_id: config.org_id.clone(),
        workspace_id: config.workspace_id.clone(),
        fallback_config: None,
        evaluation_cache: Some(superposition_provider::EvaluationCacheOptions {
            size: Some(require_env("SUPERPOSITION_CACHE_SIZE", 500)),
            ttl: Some(require_env("SUPERPOSITION_CACHE_TTL", 3600)),
        }),
        // evaluation_cache: None,
        refresh_strategy: superposition_provider::RefreshStrategy::Polling(
            superposition_provider::PollingStrategy {
                interval: require_env("SUPERPOSITION_POLL_FREQUENCY", 60),
                timeout: Some(require_env("SUPERPOSITION_POLL_TIMEOUT", 30)),
            },
        ),
        experimentation_options: None,
    };
    let provider = SuperpositionProvider::new(options);
    provider
        .init()
        .await
        .map_err(|e| SuperfuseError::Provider(e.to_string()))?;
    Ok(provider)
}

pub fn fs_config(
    mount_point: &str,
    auto_unmount: bool,
    allow_root: bool,
    n_threads: usize,
    clone_fd: bool,
) -> fuser::Config {
    let mut config = fuser::Config::default();
    if auto_unmount {
        config.mount_options.push(fuser::MountOption::AutoUnmount);
    }
    if allow_root {
        config.acl = SessionACL::RootAndOwner;
    }
    if config
        .mount_options
        .contains(&fuser::MountOption::AutoUnmount)
        && config.acl != SessionACL::RootAndOwner
    {
        config.acl = SessionACL::All;
    }
    config.n_threads = Some(n_threads);
    config.clone_fd = clone_fd;
    config.mount_options.extend([
        fuser::MountOption::RO,
        fuser::MountOption::FSName(mount_point.to_string()),
    ]);
    config
}
