use std::io::{IsTerminal, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use rustget_core::EngineConfig;
use rustget_ipc::{ApiConfig, browser_url, load_or_create_token};
use rustget_queue::{
    Category, Database, DownloadManager, DownloadRecord, DownloadStatus, ManagerConfig,
    ManagerEvent, NewDownload, PowerAction, Schedule,
};
use serde_json::json;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context as LayerContext;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, Layer};
use url::Url;
use uuid::Uuid;

struct CaptureLayer {
    buffer: Arc<rustget_ipc::LogBuffer>,
}

struct MessageVisitor {
    message: String,
    extra: Vec<String>,
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.extra.push(format!("{}={value}", field.name()));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            self.extra.push(format!("{}={value:?}", field.name()));
        }
    }
}

impl<S: Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &Event<'_>, _context: LayerContext<'_, S>) {
        let mut visitor = MessageVisitor {
            message: String::new(),
            extra: Vec::new(),
        };
        event.record(&mut visitor);
        if !visitor.extra.is_empty() {
            if !visitor.message.is_empty() {
                visitor.message.push(' ');
            }
            visitor.message.push_str(&visitor.extra.join(" "));
        }
        let metadata = event.metadata();
        self.buffer.push(
            metadata.level().as_str(),
            metadata.target(),
            visitor.message,
        );
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "rustget",
    version,
    about = "A fast, open-source, cross-platform download manager",
    long_about = None
)]
struct Cli {
    /// Application data directory (SQLite, token, logs).
    #[arg(long, global = true, env = "RUSTGET_DATA_DIR")]
    data_dir: Option<PathBuf>,
    /// Default download directory.
    #[arg(long, global = true, env = "RUSTGET_DOWNLOAD_DIR")]
    download_dir: Option<PathBuf>,
    /// Local API base URL.
    #[arg(long, global = true, default_value = "http://127.0.0.1:8765")]
    api: String,
    /// Force local execution instead of using a running daemon.
    #[arg(long, global = true)]
    local: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum AutostartAction {
    Enable,
    Disable,
    Status,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Add a download and wait for it (or queue it with --no-start).
    Add(AddArgs),
    /// List downloads.
    List(ListArgs),
    /// Show one download.
    Info {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Pause a download through the running daemon.
    Pause { id: String },
    /// Pause every active and queued download.
    PauseAll,
    /// Resume every paused/failed download.
    ResumeAll,
    /// Resume a paused download through the running daemon.
    Resume { id: String },
    /// Retry a failed download through the running daemon.
    Retry { id: String },
    /// Remove a download (optionally deleting files).
    Remove {
        id: String,
        #[arg(long)]
        delete_files: bool,
    },
    /// Run the local API, event stream and web UI.
    Serve(ServeArgs),
    /// Inspect effective paths and defaults.
    Config,
    /// Add every URL (or numeric range pattern) listed in a file.
    Batch {
        file: PathBuf,
        /// Do not start; only queue entries.
        #[arg(long)]
        no_start: bool,
        #[arg(long, default_value_t = 4)]
        connections: u8,
    },
    /// Expand a numeric range pattern for inspection.
    Expand { pattern: String },
    /// Export the download list as portable JSON.
    Export {
        /// File path; omit for stdout.
        path: Option<PathBuf>,
    },
    /// Import a previously exported download list (queued, not started).
    Import { path: PathBuf },
    /// Replace the tags on a download.
    Tag { id: String, tags: Vec<String> },
    /// Verify that a completed file exists and is readable (scanner hook helper).
    Scan { path: PathBuf },
    /// Install, remove or inspect the per-user autostart entry.
    Autostart {
        #[command(subcommand)]
        action: AutostartAction,
        /// Print the platform action without changing the system.
        #[arg(long)]
        dry_run: bool,
    },
    /// Manage weekly download windows.
    Schedule {
        #[command(subcommand)]
        action: ScheduleCommand,
    },
    /// Discover files on a site (same-origin, depth-bounded).
    Grab {
        url: String,
        #[arg(long, default_value_t = 2)]
        depth: usize,
        #[arg(long, default_value_t = 100)]
        max_pages: usize,
        /// Comma-separated extensions; empty means every link.
        #[arg(long, default_value = "")]
        ext: String,
        /// Add every discovered file to the queue.
        #[arg(long)]
        download: bool,
        #[arg(short = 'n', long, default_value_t = 8)]
        connections: u8,
    },
}

#[derive(Debug, Subcommand)]
enum ScheduleCommand {
    List,
    Add(ScheduleAdd),
    Remove { id: String },
}

#[derive(Debug, Args)]
struct ScheduleAdd {
    #[arg(long)]
    name: String,
    /// HH:MM local time.
    #[arg(long)]
    start: Option<String>,
    #[arg(long)]
    stop: Option<String>,
    /// Comma-separated weekdays (mon,tue,...).
    #[arg(long, default_value = "mon,tue,wed,thu,fri,sat,sun")]
    days: String,
    #[arg(long, default_value = "none")]
    action: String,
}

#[derive(Debug, Args)]
struct AddArgs {
    url: String,
    /// Alternate URL expected to serve the same bytes; repeatable.
    #[arg(long = "mirror")]
    mirrors: Vec<String>,
    /// Allow another row with the same URL.
    #[arg(long)]
    allow_duplicate: bool,
    /// Zero-based media variant index (HLS/DASH); default is best quality.
    #[arg(long)]
    variant: Option<usize>,
    /// Tag to attach; repeatable.
    #[arg(long = "tag")]
    tags: Vec<String>,
    /// HTTP proxy URL for this download.
    #[arg(long)]
    proxy: Option<String>,
    /// Exact output path or directory.
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(short = 'n', long, default_value_t = 8)]
    connections: u8,
    /// Speed limit in MiB/s (0 = unlimited).
    #[arg(short, long, default_value_t = 0.0)]
    limit: f64,
    #[arg(short = 'H', long = "header", value_parser = parse_header)]
    headers: Vec<(String, String)>,
    #[arg(long, default_value = "General")]
    category: String,
    /// Format algorithm:hex, e.g. sha256:abc...
    #[arg(long)]
    checksum: Option<String>,
    #[arg(long)]
    priority: Option<i32>,
    /// Queue without starting.
    #[arg(long)]
    no_start: bool,
    /// Return immediately after adding to the daemon.
    #[arg(long)]
    detach: bool,
    #[arg(long)]
    json: bool,
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Debug, Args)]
struct ListArgs {
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:8765")]
    listen: SocketAddr,
    /// Open the UI in the default browser.
    #[arg(long)]
    open: bool,
    #[arg(long, default_value_t = 4)]
    concurrency: usize,
    /// Allow scheduler actions to shut down, sleep, hibernate or exit the host.
    #[arg(long)]
    allow_power_actions: bool,
    /// Antivirus scanner program; the completed file is appended as its last argument.
    #[arg(long)]
    scanner: Option<PathBuf>,
    /// Extra argument for `--scanner`; repeatable.
    #[arg(long = "scanner-arg")]
    scanner_args: Vec<String>,
}

struct PlatformPower;

impl rustget_queue::manager::PowerHook for PlatformPower {
    fn execute(&self, action: rustget_queue::PowerAction) {
        use rustget_platform::PowerAction as PlatformAction;
        if action == rustget_queue::PowerAction::Exit {
            tracing::warn!("scheduler requested daemon exit");
            std::process::exit(0);
        }
        let mapped = match action {
            rustget_queue::PowerAction::Shutdown => PlatformAction::Shutdown,
            rustget_queue::PowerAction::Sleep => PlatformAction::Sleep,
            rustget_queue::PowerAction::Hibernate => PlatformAction::Hibernate,
            rustget_queue::PowerAction::Exit | rustget_queue::PowerAction::None => return,
        };
        match rustget_platform::execute_power(mapped) {
            Ok(command) => tracing::warn!(?command, "scheduler power command completed"),
            Err(error) => tracing::error!(%error, "scheduler power action failed"),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let logs = std::sync::Arc::new(rustget_ipc::LogBuffer::new(500));
    let console_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(console_filter),
        )
        .with(
            CaptureLayer {
                buffer: std::sync::Arc::clone(&logs),
            }
            .with_filter(tracing_subscriber::filter::LevelFilter::INFO),
        )
        .init();
    let cli = Cli::parse();
    if let Command::Scan { path } = &cli.command {
        return scan_file(path);
    }
    let paths = Paths::resolve(&cli)?;
    match &cli.command {
        Command::Serve(args) => serve(&cli, &paths, args, logs).await,
        Command::Add(args) => add(&cli, &paths, args).await,
        Command::List(args) => list(&cli, &paths, args).await,
        Command::Info { id, json } => info(&cli, &paths, id, *json).await,
        Command::Pause { id } => simple_action(&cli, &paths, id, "pause").await,
        Command::PauseAll => queue_action(&cli, &paths, "pause-all", "paused").await,
        Command::ResumeAll => queue_action(&cli, &paths, "resume-all", "resumed").await,
        Command::Resume { id } => simple_action(&cli, &paths, id, "resume").await,
        Command::Retry { id } => simple_action(&cli, &paths, id, "retry").await,
        Command::Remove { id, delete_files } => remove(&cli, &paths, id, *delete_files).await,
        Command::Config => {
            println!("data directory     {}", paths.data_dir.display());
            println!("download directory {}", paths.download_dir.display());
            println!(
                "database           {}",
                paths.data_dir.join("rustget.db").display()
            );
            println!(
                "api token          {}",
                paths.data_dir.join("api-token").display()
            );
            println!("api                {}", cli.api);
            let engine = EngineConfig::default();
            println!(
                "defaults           {} connections (max {}), {} per host, {} KiB min segment",
                engine.connections,
                engine.max_connections,
                engine.max_connections_per_host,
                engine.min_segment_size / 1024
            );
            Ok(())
        }
        Command::Batch {
            file,
            no_start,
            connections,
        } => batch(&cli, &paths, file, *no_start, *connections).await,
        Command::Expand { pattern } => {
            for url in expand_pattern(pattern)? {
                println!("{url}");
            }
            Ok(())
        }
        Command::Export { path } => export_downloads(&paths, path.as_deref()).await,
        Command::Import { path } => import_downloads(&cli, &paths, path).await,
        Command::Tag { id, tags } => tag_download(&cli, &paths, id, tags).await,
        Command::Autostart { action, dry_run } => autostart(&paths, action, *dry_run),
        Command::Scan { .. } => unreachable!("scan handled before path resolution"),
        Command::Schedule { action } => schedule_command(&cli, &paths, action).await,
        Command::Grab {
            url,
            depth,
            max_pages,
            ext,
            download,
            connections,
        } => {
            grab(
                &cli,
                &paths,
                url,
                *depth,
                *max_pages,
                ext,
                *download,
                *connections,
            )
            .await
        }
    }
}

struct Paths {
    data_dir: PathBuf,
    download_dir: PathBuf,
}

impl Paths {
    fn resolve(cli: &Cli) -> Result<Self> {
        let data_dir = cli
            .data_dir
            .clone()
            .or_else(|| dirs::data_dir().map(|dir| dir.join("RustGet")))
            .context("cannot determine a data directory; pass --data-dir")?;
        let download_dir = cli
            .download_dir
            .clone()
            .or_else(|| dirs::download_dir().map(|dir| dir.join("RustGet")))
            .unwrap_or_else(|| data_dir.join("downloads"));
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("cannot create {}", data_dir.display()))?;
        std::fs::create_dir_all(&download_dir)
            .with_context(|| format!("cannot create {}", download_dir.display()))?;
        Ok(Self {
            data_dir,
            download_dir,
        })
    }

    fn manager_config(&self) -> ManagerConfig {
        ManagerConfig {
            data_dir: self.data_dir.clone(),
            download_dir: self.download_dir.clone(),
            engine: EngineConfig::default(),
            max_concurrent: 4,
        }
    }

    fn token(&self) -> Option<String> {
        std::fs::read_to_string(self.data_dir.join("api-token"))
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }
}

async fn serve(
    cli: &Cli,
    paths: &Paths,
    args: &ServeArgs,
    logs: std::sync::Arc<rustget_ipc::LogBuffer>,
) -> Result<()> {
    if !args.listen.ip().is_loopback() {
        bail!("refusing to expose the unauthenticated static UI on a non-loopback address");
    }
    let _instance = rustget_platform::InstanceLock::acquire(paths.data_dir.join("rustget.lock"))?;
    let token = load_or_create_token(&paths.data_dir)?;
    let mut config = paths.manager_config();
    config.max_concurrent = args.concurrency.max(1);
    let manager = DownloadManager::new(config)?;
    if let Some(scanner) = &args.scanner {
        let mut command = vec![scanner.to_string_lossy().to_string()];
        command.extend(args.scanner_args.iter().cloned());
        manager.set_setting("antivirus.scan_command", &serde_json::to_string(&command)?)?;
        println!("scanner:    {}", scanner.display());
    }
    if args.allow_power_actions {
        manager.set_power_hook(std::sync::Arc::new(PlatformPower));
        println!("power actions: ENABLED (shutdown/sleep/hibernate/exit)");
    }
    manager.start_scheduler();
    tracing::info!(api = %args.listen, "RustGet daemon started");
    println!("RustGet {}", env!("CARGO_PKG_VERSION"));
    println!("data:       {}", paths.data_dir.display());
    println!("downloads:  {}", paths.download_dir.display());
    println!("API:        http://{}", args.listen);
    println!("token:      {token}");
    println!("open:       {}", browser_url(args.listen, &token));
    if args.open {
        let _ = open::that_detached(browser_url(args.listen, &token));
    }
    let api = ApiConfig {
        listen: args.listen,
        token,
        allowed_origins: Vec::new(),
        logs: Some(logs),
    };
    let manager_for_signal = Arc::clone(&manager);
    tokio::select! {
        result = rustget_ipc::serve(Arc::clone(&manager), api) => {
            result.context("API server failed")?;
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\nshutting down; active downloads are paused");
            manager_for_signal.shutdown();
        }
    }
    let _ = cli;
    Ok(())
}

async fn add(cli: &Cli, paths: &Paths, args: &AddArgs) -> Result<()> {
    let (algorithm, value) = match &args.checksum {
        Some(spec) => {
            let (algorithm, value) = spec
                .split_once(':')
                .context("checksum must be ALGORITHM:HEX")?;
            (Some(algorithm.to_string()), Some(value.to_string()))
        }
        None => (None, None),
    };
    let request = NewDownload {
        url: args.url.clone(),
        mirrors: args.mirrors.clone(),
        media_variant: args.variant,
        tags: args.tags.clone(),
        proxy: args.proxy.clone(),
        allow_duplicate: args.allow_duplicate,
        destination: args.output.clone(),
        directory: Some(paths.download_dir.clone()),
        category: Some(args.category.clone()),
        connections: Some(args.connections),
        speed_limit: (args.limit > 0.0).then_some((args.limit * 1024.0 * 1024.0) as u64),
        headers: args.headers.clone(),
        checksum_algorithm: algorithm,
        checksum_value: value,
        priority: args.priority,
        start: !args.no_start,
    };
    if !cli.local && daemon_available(&cli.api).await {
        let body = api_json(
            &cli.api,
            paths,
            reqwest::Method::POST,
            "/api/v1/downloads",
            Some(&request),
        )
        .await?;
        let record: DownloadRecord = serde_json::from_value(body)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&record)?);
        } else {
            println!("{}  {}", short_id(record.id), record.filename);
            println!("{}", record.destination.display());
        }
        if args.no_start || args.detach {
            return Ok(());
        }
        return wait_api(&cli.api, paths, record.id, args.quiet).await;
    }
    if args.detach {
        bail!("--detach requires a running `rustget serve` daemon");
    }
    let manager = DownloadManager::new(paths.manager_config())?;
    let mut events = manager.subscribe();
    let record = manager.add(request).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&record)?);
    } else if !args.quiet {
        eprintln!("added {}  {}", short_id(record.id), record.filename);
    }
    if args.no_start {
        return Ok(());
    }
    let tty = std::io::stderr().is_terminal() && !args.quiet;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                manager.shutdown();
                eprintln!("\npaused; resume with `rustget resume {}` after starting the daemon", record.id);
                return Ok(());
            }
            event = events.recv() => {
                match event {
                    Ok(ManagerEvent::Progress { id, progress }) if id == record.id && tty => {
                        let percent = progress.total_size.map_or(0.0, |total| {
                            if total == 0 { 100.0 } else { progress.downloaded as f64 / total as f64 * 100.0 }
                        });
                        let line = format!(
                            "{:5.1}%  {:>10}/s  {}",
                            percent,
                            human_bytes(progress.speed_bps as u64),
                            progress.eta_secs.map_or("--".to_string(), human_eta)
                        );
                        eprint!("\r{}", pad(&line, 70));
                        let _ = std::io::stderr().flush();
                    }
                    Ok(ManagerEvent::Finished { id, status }) if id == record.id => {
                        if tty { eprintln!(); }
                        let stored = manager.get(record.id)?;
                        if args.json {
                            println!("{}", serde_json::to_string_pretty(&stored)?);
                        }
                        match status {
                            DownloadStatus::Completed => {
                                println!("complete  {}", stored.destination.display());
                                return Ok(());
                            }
                            DownloadStatus::Paused => {
                                println!("paused    {}", stored.destination.display());
                                return Ok(());
                            }
                            DownloadStatus::Failed => {
                                bail!("download failed: {}", stored.error.unwrap_or_default());
                            }
                            _ => {}
                        }
                        return Ok(());
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        }
    }
    Ok(())
}

async fn list(cli: &Cli, paths: &Paths, args: &ListArgs) -> Result<()> {
    if !cli.local && daemon_available(&cli.api).await {
        let path = match &args.status {
            Some(status) => format!("/api/v1/downloads?status={status}"),
            None => "/api/v1/downloads".into(),
        };
        let body = api_json(&cli.api, paths, reqwest::Method::GET, &path, None::<&()>).await?;
        let records: Vec<DownloadRecord> =
            serde_json::from_value(body.get("downloads").cloned().unwrap_or(json!([])))?;
        return print_list(&records, args.json);
    }
    let db = Database::open(paths.data_dir.join("rustget.db"))?;
    let status = args.status.as_deref().map(DownloadStatus::parse);
    let records = db.list(status)?;
    print_list(&records, args.json)
}

fn print_list(records: &[DownloadRecord], as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(records)?);
        return Ok(());
    }
    if records.is_empty() {
        println!("no downloads");
        return Ok(());
    }
    println!(
        "{:<10} {:<12} {:>10} {:>7}  NAME",
        "ID", "STATUS", "SIZE", "DONE"
    );
    for record in records {
        let percent = if let Some(total) = record.total_size {
            if total == 0 {
                100.0
            } else {
                record.downloaded as f64 / total as f64 * 100.0
            }
        } else {
            0.0
        };
        println!(
            "{:<10} {:<12} {:>10} {:>6.1}%  {}",
            short_id(record.id),
            record.status.as_str(),
            record.total_size.map_or_else(|| "?".into(), human_bytes),
            percent,
            record.filename
        );
    }
    Ok(())
}

async fn info(cli: &Cli, paths: &Paths, id: &str, as_json: bool) -> Result<()> {
    let uuid = parse_id(id)?;
    let record = if !cli.local && daemon_available(&cli.api).await {
        let body = api_json(
            &cli.api,
            paths,
            reqwest::Method::GET,
            &format!("/api/v1/downloads/{uuid}"),
            None::<&()>,
        )
        .await?;
        serde_json::from_value(body.get("download").cloned().unwrap_or(json!({})))?
    } else {
        Database::open(paths.data_dir.join("rustget.db"))?
            .get(uuid)?
            .context("download not found")?
    };
    if as_json {
        println!("{}", serde_json::to_string_pretty(&record)?);
        return Ok(());
    }
    println!("ID          {}", record.id);
    println!("Name        {}", record.filename);
    println!("Status      {}", record.status.as_str());
    println!(
        "Progress    {} / {} ({:.1}%)",
        human_bytes(record.downloaded),
        record.total_size.map_or_else(|| "?".into(), human_bytes),
        record.progress_ratio() * 100.0
    );
    println!("Destination {}", record.destination.display());
    println!("URL         {}", record.url);
    if let Some(final_url) = &record.final_url {
        println!("Final URL   {final_url}");
    }
    println!("Connections {}", record.connections);
    if let Some(error) = &record.error {
        println!("Error       {error}");
    }
    Ok(())
}

async fn simple_action(cli: &Cli, paths: &Paths, id: &str, action: &str) -> Result<()> {
    let uuid = parse_id(id)?;
    if cli.local || !daemon_available(&cli.api).await {
        bail!("this command requires a running `rustget serve` daemon (or use the web UI)");
    }
    let body = api_json(
        &cli.api,
        paths,
        reqwest::Method::POST,
        &format!("/api/v1/downloads/{uuid}/{action}"),
        None::<&()>,
    )
    .await?;
    let record: DownloadRecord = serde_json::from_value(body)?;
    println!("{}  {}", record.status.as_str(), record.filename);
    Ok(())
}

async fn queue_action(cli: &Cli, paths: &Paths, action: &str, field: &str) -> Result<()> {
    if cli.local || !daemon_available(&cli.api).await {
        bail!("this command requires a running `rustget serve` daemon");
    }
    let body = api_json(
        &cli.api,
        paths,
        reqwest::Method::POST,
        &format!("/api/v1/downloads/{action}"),
        None::<&()>,
    )
    .await?;
    println!(
        "{field} {} download(s)",
        body.get(field)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    );
    Ok(())
}

async fn export_downloads(paths: &Paths, path: Option<&std::path::Path>) -> Result<()> {
    let db = Database::open(paths.data_dir.join("rustget.db"))?;
    let records = db.list(None)?;
    let downloads: Vec<NewDownload> = records.iter().map(NewDownload::from_record).collect();
    let settings: std::collections::BTreeMap<String, String> =
        db.list_settings()?.into_iter().collect();
    let categories = db.list_categories()?;
    let schedules = db.list_schedules()?;
    let bundle = serde_json::json!({
        "version": 1,
        "downloads": downloads,
        "settings": settings,
        "categories": categories,
        "schedules": schedules,
    });
    let text = serde_json::to_string_pretty(&bundle)?;
    if let Some(path) = path {
        std::fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))?;
        println!(
            "exported {} download(s) to {}",
            records.len(),
            path.display()
        );
    } else {
        println!("{text}");
    }
    Ok(())
}

async fn import_downloads(cli: &Cli, paths: &Paths, path: &std::path::Path) -> Result<()> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    let entries: Vec<NewDownload> = serde_json::from_value(
        value
            .get("downloads")
            .cloned()
            .unwrap_or_else(|| value.clone()),
    )?;
    let settings: std::collections::BTreeMap<String, String> = value
        .get("settings")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    let categories: Vec<Category> = value
        .get("categories")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    let schedules: Vec<Schedule> = value
        .get("schedules")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    if entries.is_empty() && settings.is_empty() && categories.is_empty() && schedules.is_empty() {
        bail!("import file contains no settings or downloads");
    }
    if !cli.local && daemon_available(&cli.api).await {
        let body = serde_json::json!({
            "downloads": entries,
            "settings": settings,
            "categories": categories,
            "schedules": schedules,
        });
        let result = api_json(
            &cli.api,
            paths,
            reqwest::Method::POST,
            "/api/v1/import",
            Some(&body),
        )
        .await?;
        println!(
            "imported {} download(s) (queued)",
            result
                .get("imported")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        );
        return Ok(());
    }
    let manager = DownloadManager::new(paths.manager_config())?;
    for (key, value) in &settings {
        manager.set_setting(key, value)?;
    }
    for category in &categories {
        manager.upsert_category(category)?;
    }
    for schedule in &schedules {
        manager.replace_schedule(schedule)?;
    }
    let mut imported = 0_usize;
    let mut failures = Vec::new();
    for mut entry in entries {
        entry.start = false;
        match manager.add(entry).await {
            Ok(_) => imported += 1,
            Err(error) => failures.push(error.to_string()),
        }
    }
    println!(
        "imported {imported} download(s), {} setting(s), {} categor(ies), {} schedule(s)",
        settings.len(),
        categories.len(),
        schedules.len()
    );
    for failure in failures {
        eprintln!("failed: {failure}");
    }
    Ok(())
}

fn scan_file(path: &std::path::Path) -> Result<()> {
    let metadata =
        std::fs::metadata(path).with_context(|| format!("cannot stat {}", path.display()))?;
    if !metadata.is_file() {
        bail!("not a regular file: {}", path.display());
    }
    let mut file =
        std::fs::File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut probe = [0_u8; 1];
    let _ = std::io::Read::read(&mut file, &mut probe)?;
    println!("ok {} ({} bytes)", path.display(), metadata.len());
    Ok(())
}

fn autostart(paths: &Paths, action: &AutostartAction, dry_run: bool) -> Result<()> {
    let executable = std::env::current_exe().context("cannot locate the rustget executable")?;
    let preview = rustget_platform::autostart_preview(&executable, &paths.data_dir);
    match action {
        AutostartAction::Status => {
            let enabled = rustget_platform::autostart_status()?;
            println!(
                "autostart: {}",
                if enabled { "enabled" } else { "disabled" }
            );
            println!("entry:     {preview}");
        }
        AutostartAction::Enable => {
            println!("entry: {preview}");
            if dry_run {
                println!("dry run: no changes");
            } else {
                rustget_platform::autostart_enable(&executable, &paths.data_dir)?;
                println!("autostart enabled");
            }
        }
        AutostartAction::Disable => {
            if dry_run {
                println!("dry run: would remove the RustGet autostart entry");
            } else {
                rustget_platform::autostart_disable()?;
                println!("autostart disabled");
            }
        }
    }
    Ok(())
}

async fn tag_download(cli: &Cli, paths: &Paths, id: &str, tags: &[String]) -> Result<()> {
    let uuid = parse_id(id)?;
    if !cli.local && daemon_available(&cli.api).await {
        let body = serde_json::json!({"tags": tags});
        let record = api_json(
            &cli.api,
            paths,
            reqwest::Method::PATCH,
            &format!("/api/v1/downloads/{uuid}"),
            Some(&body),
        )
        .await?;
        println!("tags: {}", record["tags"]);
        return Ok(());
    }
    let manager = DownloadManager::new(paths.manager_config())?;
    let record = manager.set_tags(uuid, tags.to_vec())?;
    println!("tags: {}", record.tags.join(", "));
    Ok(())
}

async fn remove(cli: &Cli, paths: &Paths, id: &str, delete_files: bool) -> Result<()> {
    let uuid = parse_id(id)?;
    if !cli.local && daemon_available(&cli.api).await {
        let token = paths.token().context("missing API token")?;
        let response = reqwest::Client::new()
            .delete(format!(
                "{}/api/v1/downloads/{uuid}?delete_files={delete_files}",
                cli.api.trim_end_matches('/')
            ))
            .bearer_auth(token)
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("remove failed: HTTP {}", response.status());
        }
        println!("removed {uuid}");
        return Ok(());
    }
    let db = Database::open(paths.data_dir.join("rustget.db"))?;
    let record = db.get(uuid)?.context("download not found")?;
    if delete_files {
        let paths = rustget_storage::DownloadPaths::new(&record.destination);
        for path in [&record.destination, &paths.part, &paths.journal] {
            if path.exists() {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    db.delete(uuid)?;
    println!("removed {uuid}");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn grab(
    cli: &Cli,
    paths: &Paths,
    url: &str,
    depth: usize,
    max_pages: usize,
    ext: &str,
    download: bool,
    connections: u8,
) -> Result<()> {
    let start = Url::parse(url).context("invalid URL")?;
    if !matches!(start.scheme(), "http" | "https") {
        bail!("only http(s) URLs can be grabbed");
    }
    let crawler = rustget_crawler::Crawler::new(rustget_crawler::CrawlConfig {
        max_depth: depth,
        max_pages,
        extensions: ext
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase)
            .collect(),
        ..rustget_crawler::CrawlConfig::default()
    })?;
    println!("exploring {start} (depth {depth}, {max_pages} pages max)");
    let result = crawler.crawl(start).await?;
    println!(
        "visited {} page(s), found {} file(s)",
        result.pages_visited,
        result.files.len()
    );
    for file in &result.files {
        println!("{}", file.url);
    }
    for error in &result.errors {
        eprintln!("warning: {error}");
    }
    if download {
        for file in result.files {
            add(
                cli,
                paths,
                &AddArgs {
                    url: file.url,
                    mirrors: Vec::new(),
                    variant: None,
                    tags: Vec::new(),
                    proxy: None,
                    allow_duplicate: false,
                    output: None,
                    connections,
                    limit: 0.0,
                    headers: Vec::new(),
                    category: "Grabber".into(),
                    checksum: None,
                    priority: None,
                    no_start: false,
                    detach: false,
                    json: false,
                    quiet: true,
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn schedule_command(cli: &Cli, paths: &Paths, action: &ScheduleCommand) -> Result<()> {
    let db = Database::open(paths.data_dir.join("rustget.db"))?;
    match action {
        ScheduleCommand::List => {
            let schedules = db.list_schedules()?;
            if schedules.is_empty() {
                println!("no schedules");
                return Ok(());
            }
            for schedule in schedules {
                let window = match (schedule.start_min, schedule.stop_min) {
                    (Some(start), Some(stop)) => {
                        format!(
                            "{}–{}",
                            Schedule::format_time(start),
                            Schedule::format_time(stop)
                        )
                    }
                    (Some(start), None) => format!("from {}", Schedule::format_time(start)),
                    (None, Some(stop)) => format!("until {}", Schedule::format_time(stop)),
                    (None, None) => "always".into(),
                };
                println!(
                    "{}  {:<20} {:<20} days={:07b} action={}",
                    short_id(schedule.id),
                    schedule.name,
                    window,
                    schedule.days,
                    schedule.action.as_str()
                );
            }
            Ok(())
        }
        ScheduleCommand::Add(args) => {
            if args.start.is_none() && args.stop.is_none() {
                bail!("at least one of --start or --stop is required");
            }
            let start = args
                .start
                .as_deref()
                .map(|value| {
                    Schedule::parse_time(value)
                        .with_context(|| format!("invalid start time '{value}'"))
                })
                .transpose()?;
            let stop = args
                .stop
                .as_deref()
                .map(|value| {
                    Schedule::parse_time(value)
                        .with_context(|| format!("invalid stop time '{value}'"))
                })
                .transpose()?;
            let mut days = 0_u8;
            for part in args.days.split(',') {
                let bit = match part.trim().to_ascii_lowercase().as_str() {
                    "mon" | "monday" => 0,
                    "tue" | "tuesday" => 1,
                    "wed" | "wednesday" => 2,
                    "thu" | "thursday" => 3,
                    "fri" | "friday" => 4,
                    "sat" | "saturday" => 5,
                    "sun" | "sunday" => 6,
                    _ => bail!("unknown weekday '{}'", part.trim()),
                };
                days |= 1 << bit;
            }
            let schedule = Schedule {
                id: Uuid::new_v4(),
                name: args.name.clone(),
                enabled: true,
                days,
                start_min: start,
                stop_min: stop,
                action: PowerAction::parse(&args.action),
            };
            if !cli.local && daemon_available(&cli.api).await {
                let token = paths.token().context("missing API token")?;
                let response = reqwest::Client::new()
                    .post(format!(
                        "{}/api/v1/schedules",
                        cli.api.trim_end_matches('/')
                    ))
                    .bearer_auth(token)
                    .json(&serde_json::json!({
                        "name": schedule.name,
                        "days": days,
                        "start": start.map(Schedule::format_time),
                        "stop": stop.map(Schedule::format_time),
                        "action": schedule.action.as_str(),
                    }))
                    .send()
                    .await?;
                if !response.status().is_success() {
                    bail!("API error: {}", response.text().await?);
                }
                println!("schedule added");
            } else {
                db.insert_schedule(&schedule)?;
                println!(
                    "schedule {} added (daemon must run to enforce it)",
                    schedule.id
                );
            }
            Ok(())
        }
        ScheduleCommand::Remove { id } => {
            let id = parse_id(id)?;
            if !cli.local && daemon_available(&cli.api).await {
                let token = paths.token().context("missing API token")?;
                let response = reqwest::Client::new()
                    .delete(format!(
                        "{}/api/v1/schedules/{id}",
                        cli.api.trim_end_matches('/')
                    ))
                    .bearer_auth(token)
                    .send()
                    .await?;
                if !response.status().is_success() {
                    bail!("API error: HTTP {}", response.status());
                }
            } else {
                db.delete_schedule(id)?;
            }
            println!("removed {id}");
            Ok(())
        }
    }
}

async fn batch(
    cli: &Cli,
    paths: &Paths,
    file: &Path,
    no_start: bool,
    connections: u8,
) -> Result<()> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
    let mut urls = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        urls.extend(expand_pattern(line)?);
    }
    if urls.is_empty() {
        bail!("no URLs found in {}", file.display());
    }
    println!("adding {} URL(s)", urls.len());
    for url in urls {
        let args = AddArgs {
            url,
            mirrors: Vec::new(),
            variant: None,
            tags: Vec::new(),
            proxy: None,
            allow_duplicate: false,
            output: None,
            connections,
            limit: 0.0,
            headers: Vec::new(),
            category: "General".into(),
            checksum: None,
            priority: None,
            no_start,
            detach: false,
            json: false,
            quiet: true,
        };
        add(cli, paths, &args).await?;
    }
    Ok(())
}

/// Expand `name_[001-010].jpg` and multiple ranges recursively.
fn expand_pattern(pattern: &str) -> Result<Vec<String>> {
    const LIMIT: usize = 10_000;
    let Some((start_index, end_index)) = find_range(pattern) else {
        return Ok(vec![pattern.to_string()]);
    };
    let inside = &pattern[start_index + 1..end_index];
    let (first, last) = inside.split_once('-').context("range must be START-END")?;
    let from: u64 = first.trim().parse().context("range start is not numeric")?;
    let to: u64 = last.trim().parse().context("range end is not numeric")?;
    if to < from {
        bail!("range end precedes start");
    }
    let width = first.trim().len().max(1);
    let prefix = &pattern[..start_index];
    let suffix = &pattern[end_index + 1..];
    let mut out = Vec::new();
    for value in from..=to {
        let replacement = format!("{value:0width$}");
        let next = format!("{prefix}{replacement}{suffix}");
        for expanded in expand_pattern(&next)? {
            out.push(expanded);
        }
        if out.len() > LIMIT {
            bail!("pattern expands to more than {LIMIT} URLs");
        }
    }
    Ok(out)
}

fn find_range(value: &str) -> Option<(usize, usize)> {
    let start = value.find('[')?;
    let end = value[start..].find(']')? + start;
    let inside = &value[start + 1..end];
    let (from, to) = inside.split_once('-')?;
    if from.trim().parse::<u64>().is_ok() && to.trim().parse::<u64>().is_ok() {
        Some((start, end))
    } else {
        None
    }
}

async fn daemon_available(api: &str) -> bool {
    reqwest::Client::new()
        .get(format!("{}/api/v1/health", api.trim_end_matches('/')))
        .timeout(Duration::from_millis(700))
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

async fn api_json<T: serde::Serialize + ?Sized>(
    api: &str,
    paths: &Paths,
    method: reqwest::Method,
    path: &str,
    body: Option<&T>,
) -> Result<serde_json::Value> {
    let token = paths
        .token()
        .context("missing API token; is the daemon running?")?;
    let mut request = reqwest::Client::new()
        .request(method, format!("{}{}", api.trim_end_matches('/'), path))
        .bearer_auth(token);
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .unwrap_or(text);
        bail!("API error {status}: {message}");
    }
    if text.is_empty() {
        return Ok(json!({}));
    }
    Ok(serde_json::from_str(&text)?)
}

async fn wait_api(api: &str, paths: &Paths, id: Uuid, quiet: bool) -> Result<()> {
    let tty = std::io::stderr().is_terminal() && !quiet;
    let mut last = String::new();
    loop {
        let body = api_json(
            api,
            paths,
            reqwest::Method::GET,
            &format!("/api/v1/downloads/{id}"),
            None::<&()>,
        )
        .await?;
        let record: DownloadRecord =
            serde_json::from_value(body.get("download").cloned().unwrap_or(json!({})))?;
        if tty {
            let line = format!(
                "{:5.1}%  {:>9}  {}",
                record.progress_ratio() * 100.0,
                format!("{}/s", human_bytes(record.speed_bps as u64)),
                record.filename
            );
            if line != last {
                eprint!("\r{}", pad(&line, 78));
                let _ = std::io::stderr().flush();
                last = line;
            }
        }
        match record.status {
            DownloadStatus::Completed => {
                if tty {
                    eprintln!();
                }
                println!("complete  {}", record.destination.display());
                return Ok(());
            }
            DownloadStatus::Failed => {
                if tty {
                    eprintln!();
                }
                bail!("download failed: {}", record.error.unwrap_or_default());
            }
            DownloadStatus::Paused => {
                if tty {
                    eprintln!();
                }
                println!("paused");
                return Ok(());
            }
            _ => {}
        }
        tokio::time::sleep(Duration::from_millis(350)).await;
    }
}

fn parse_header(value: &str) -> Result<(String, String), String> {
    let (name, value) = value
        .split_once(':')
        .ok_or_else(|| "header must be NAME: VALUE".to_string())?;
    Ok((name.trim().to_string(), value.trim().to_string()))
}

fn parse_id(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value).context("invalid download ID (expected UUID)")
}

fn short_id(id: Uuid) -> String {
    id.to_string().chars().take(8).collect()
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn human_eta(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h{:02}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

fn pad(value: &str, width: usize) -> String {
    let len = value.chars().count();
    if len >= width {
        value.to_string()
    } else {
        format!("{value}{}", " ".repeat(width - len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_numeric_patterns_with_padding() {
        let urls = expand_pattern("https://x/img_[008-010].jpg").unwrap();
        assert_eq!(
            urls,
            vec![
                "https://x/img_008.jpg",
                "https://x/img_009.jpg",
                "https://x/img_010.jpg"
            ]
        );
    }

    #[test]
    fn expands_multiple_numeric_ranges() {
        let urls = expand_pattern("/[1-2]/[3-4].bin").unwrap();
        assert_eq!(urls, vec!["/1/3.bin", "/1/4.bin", "/2/3.bin", "/2/4.bin"]);
    }

    #[test]
    fn leaves_non_numeric_brackets_alone() {
        assert_eq!(
            expand_pattern("/[a-c].bin").unwrap(),
            vec!["/[a-c].bin".to_string()]
        );
    }

    #[test]
    fn rejects_reversed_range() {
        assert!(expand_pattern("[10-1].txt").is_err());
    }
}
