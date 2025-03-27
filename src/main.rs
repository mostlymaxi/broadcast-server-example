mod server;
use server::serve;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), std::io::Error> {
    tracing_subscriber::fmt::init();

    const LISTEN_ADDR: &str = "localhost:8888";

    serve(LISTEN_ADDR).await
}
