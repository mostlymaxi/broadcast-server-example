use futures::{SinkExt, TryStreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    select,
    sync::broadcast::{self, Sender},
};

use tokio_util::codec::{Framed, LinesCodec};

fn build_login_msg(port: u16) -> String {
    format!("LOGIN:{port}")
}

fn build_message_msg(port: u16, content: String) -> String {
    format!("MESSAGE:{port} {content}")
}

async fn handle_new_connection(sock: TcpStream, port: u16, tx: Sender<(u16, String)>) {
    // including tokio_util as a dependency for better quality codecs.
    //
    // this gives greater flexbility for changing the message format if needed
    // and is always cancellation safe.
    let codec = LinesCodec::new_with_max_length(8192);
    let mut sock = Framed::new(sock, codec);
    let mut rx = tx.subscribe();

    sock.send(build_login_msg(port)).await.unwrap();

    loop {
        select! {
            Ok(Some(msg)) = sock.try_next() => {
                eprintln!("message {port} {msg}");
                tx.send((port, msg)).unwrap();
            }
            Ok((port_recv, msg)) = rx.recv() => {
                if port_recv != port {
                    sock.send(build_message_msg(port_recv, msg)).await.unwrap();
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    const LISTEN_ADDR: &str = "localhost:8888";
    const BROADCAST_CHANNEL_CAP: usize = 128;

    let listener = TcpListener::bind(LISTEN_ADDR).await.unwrap();
    let (tx, _rx) = broadcast::channel(BROADCAST_CHANNEL_CAP);

    eprintln!("started listening on {LISTEN_ADDR}");

    loop {
        let Ok((sock, addr)) = listener.accept().await else {
            continue;
        };

        eprintln!("connected {addr}");

        let tx_c = tx.clone();
        tokio::spawn(handle_new_connection(sock, addr.port(), tx_c));
    }
}
