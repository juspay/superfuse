mod config;
mod error;
mod storage;

use std::{io::stdout, path::PathBuf};

use clap::{Parser, Subcommand};
use tracing::{debug, error, info, warn};
use tracing_appender::{non_blocking, rolling};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use config::DataPaths;
use error::SuperfuseError;
use storage::FileMapping;

use crate::config::{SuperpositionConfig, init_superposition_provider};

#[derive(Parser, Debug)]
#[command(name = "superfuse", about = "Virtual filesystem for templated configs")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the superfuse filesystem
    Start {
        mount_point: PathBuf,
        /// Automatically unmount on process exit
        #[clap(long)]
        auto_unmount: bool,
        /// Allow root user to access filesystem
        #[clap(long)]
        allow_root: bool,
        /// Number of threads to use
        #[clap(long, default_value_t = 1)]
        n_threads: usize,
        /// Use `FUSE_DEV_IOC_CLONE` to give each worker thread its own fd.
        /// This enables more efficient request processing
        /// when multiple threads are used. Requires Linux 4.5+.
        #[clap(long)]
        clone_fd: bool,
    },
    /// Add a new file mapping
    Add {
        /// Virtual path in the FUSE filesystem (e.g., /config/app.yaml)
        #[arg(short, long)]
        path: String,
        /// Path to the template file (handlebars format)
        #[arg(short, long)]
        template: String,
    },
    /// Remove an existing file mapping
    Remove {
        /// Virtual path to remove
        #[arg(short, long)]
        path: String,
    },
    /// Update an existing file mapping
    Update {
        /// Virtual path to update
        #[arg(short, long)]
        path: String,
        /// New template file path
        #[arg(short, long)]
        template: String,
    },
    /// List all file mappings
    List,
}

fn init_tracing(log_dir: &std::path::Path) -> Result<(), SuperfuseError> {
    let file_appender = rolling::daily(log_dir, "superfuse.log");
    let (non_blocking, _guard) = non_blocking(file_appender);

    let log_level = std::env::var("SUPERFUSE_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_level));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(stdout))
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
        .init();

    debug!(log_level = %log_level, "tracing initialized");
    Ok(())
}

fn validate_template_exists(template_path: &str) -> Result<std::path::PathBuf, SuperfuseError> {
    let path = std::path::PathBuf::from(template_path);
    if !path.exists() {
        return Err(SuperfuseError::TemplateNotFound(path));
    }
    Ok(path)
}

fn print_mappings(mappings: &[FileMapping]) {
    if mappings.is_empty() {
        println!("No mappings found.");
        return;
    }

    println!(
        "{:<6} {:<40} {:<40} {:<20}",
        "ID", "Virtual Path", "Template Path", "Updated"
    );
    println!("{}", "-".repeat(106));
    for m in mappings {
        println!(
            "{:<6} {:<40} {:<40} {:<20}",
            m.id,
            truncate(&m.virtual_path, 40),
            truncate(&m.template_path, 40),
            truncate(&m.updated_at, 20)
        );
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    } else {
        s.to_string()
    }
}

#[tokio::main]
async fn main() {
    // Load .env file (non-fatal if missing)
    if dotenvy::dotenv().is_err() {
        // Only warn if user is trying to use provider features
        warn!("no .env file found, using environment variables");
    }

    // Initialize data directory
    let Ok(data_paths) = DataPaths::init().map_err(|e| eprintln!("Data path Error: {e}")) else {
        std::process::exit(1);
    };

    // Initialize tracing (logs to data directory + stdout)
    if let Err(e) = init_tracing(&data_paths.logs) {
        eprintln!("Failed to initialize logging: {e}");
        std::process::exit(1);
    }

    // Initialize database
    let Ok(pool) = storage::init_db(&data_paths.db)
        .await
        .map_err(|e| error!(error = %e, "failed to initialize database"))
    else {
        std::process::exit(1);
    };

    // Parse CLI
    let cli = Cli::parse();

    // Dispatch commands
    if let Err(e) = handle_command(cli, pool, &data_paths).await {
        error!(error = %e, "command failed");
        std::process::exit(1);
    }
}

async fn handle_command(
    cli: Cli,
    pool: sqlx::SqlitePool,
    paths: &DataPaths,
) -> Result<(), SuperfuseError> {
    match cli.command {
        Commands::Start { .. } => {
            let config = SuperpositionConfig::init()?;
            let provider = init_superposition_provider(&config).await?;
        }
        Commands::Add { path, template } => {
            validate_template_exists(&template)?;
            let canonical = std::fs::canonicalize(&template).map_err(SuperfuseError::Io)?;
            let template_path = canonical.display().to_string();

            let mapping = storage::add_mapping(&pool, &path, &template_path).await?;
            info!(id = mapping.id, path = %mapping.virtual_path, "mapping added");
            println!(
                "Added mapping: {} -> {}",
                mapping.virtual_path, mapping.template_path
            );
        }
        Commands::Remove { ref path } => {
            storage::remove_mapping(&pool, path).await?;
            println!("Removed mapping: {}", path);
        }
        Commands::Update { ref path, template } => {
            validate_template_exists(&template)?;
            let canonical = std::fs::canonicalize(&template).map_err(SuperfuseError::Io)?;
            let template_path = canonical.display().to_string();

            let mapping = storage::update_mapping(&pool, path, &template_path).await?;
            println!(
                "Updated mapping: {} -> {}",
                mapping.virtual_path, mapping.template_path
            );
        }
        Commands::List => {
            let mappings = storage::list_mappings(&pool).await?;
            print_mappings(&mappings);
        }
    }
    Ok(())
}
