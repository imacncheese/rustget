use clap::Parser;
use rustget_testkit::{TestFile, TestServer, TestkitConfig};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "rustget-testkit",
    about = "Instrumented adversarial HTTP origin"
)]
struct Args {
    /// Address to bind (port 0 chooses a free port).
    #[arg(long, default_value = "127.0.0.1:0")]
    listen: std::net::SocketAddr,
    /// Size of the generated file.
    #[arg(long, default_value_t = 16 * 1024 * 1024)]
    size: u64,
    #[arg(long, default_value = "file.bin")]
    name: String,
    /// Ignore Range and return 200.
    #[arg(long)]
    no_range: bool,
    /// Advertise ranges but ignore them.
    #[arg(long)]
    lie_range: bool,
    /// Reset this many responses mid-body.
    #[arg(long, default_value_t = 0)]
    flaky: u32,
    #[arg(long, default_value_t = 1024 * 1024)]
    fail_after: u64,
    #[arg(long)]
    throttle: Option<u64>,
    #[arg(long, default_value_t = 0)]
    redirects: u8,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    let mut file = TestFile::new(args.name, args.size);
    if args.no_range {
        file = file.no_range();
    }
    if args.lie_range {
        file = file.lie_about_range();
    }
    if args.flaky > 0 {
        file = file.flaky(args.flaky, args.fail_after);
    }
    if let Some(bps) = args.throttle {
        file = file.throttled(bps);
    }
    if args.redirects > 0 {
        file = file.redirects(args.redirects);
    }
    let range_mode = file.range_mode;
    let file_name = file.name.clone();
    let server =
        TestServer::start_at(args.listen, TestkitConfig::default().with_file(file)).await?;
    println!("testkit listening on http://{}", server.addr);
    println!("file:       {}", server.file_url(&file_name));
    println!("range mode: {range_mode:?}");
    println!("transcript: {}/__admin/requests", server.base_url);
    tokio::signal::ctrl_c().await?;
    server.shutdown().await;
    Ok(())
}
