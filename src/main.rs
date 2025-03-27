mod server;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use server::serve;

use clap::Parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// socket address to serve requests from
    #[arg(default_value_t = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 8888)))]
    bind: SocketAddr,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), std::io::Error> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    serve(args.bind).await
}
