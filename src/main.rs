use std::collections::HashMap;

use futures::{future::select_all, FutureExt, SinkExt, TryStreamExt};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    select,
};
use tokio_util::codec::{Framed, LinesCodec};

fn build_login_msg(port: u16) -> String {
    format!("LOGIN:{port}")
}

fn build_message_msg(port: u16, content: String) -> String {
    format!("MESSAGE:{port} {content}")
}

enum SelectResult {
    NewConnection((u16, TcpStream)),
    NewMessage((u16, String)),
    ClientDisconnected(u16),
}

#[tokio::main]
async fn main() {
    const LISTEN_ADDR: &str = "localhost:8888";

    let mut connections: HashMap<u16, Framed<TcpStream, LinesCodec>> = HashMap::new();

    let listener = TcpListener::bind(LISTEN_ADDR).await.unwrap();

    eprintln!("started listening on {LISTEN_ADDR}");

    loop {
        let res = if connections.is_empty() {
            if let Ok((sock, addr)) = listener.accept().await {
                SelectResult::NewConnection((addr.port(), sock))
            } else {
                continue;
            }
        } else {
            let new_msg_task = select_all(
                connections
                    .iter_mut()
                    .map(|(port, c)| async move { (port, c.try_next().await) }.boxed()),
            );

            select! {
                Ok((mut sock, addr)) = listener.accept() => {
                    sock.write_all(build_login_msg(addr.port()).as_bytes()).await.unwrap();
                    SelectResult::NewConnection((addr.port(), sock))
                }

                ((port, msg), _, _) = new_msg_task => {
                    if let Ok(Some(msg)) = msg {
                        SelectResult::NewMessage((*port, msg))
                    } else {
                        SelectResult::ClientDisconnected(*port)
                    }

                }
            }
        };

        match res {
            SelectResult::NewConnection((port, c)) => {
                let codec = LinesCodec::new_with_max_length(8192);
                let mut framed = Framed::new(c, codec);
                framed.send(build_login_msg(port)).await.unwrap();
                let _ = connections.insert(port, framed);
            }
            SelectResult::NewMessage((port, m)) => {
                for (p, c) in connections.iter_mut() {
                    if *p == port {
                        continue;
                    }

                    c.send(build_message_msg(port, m.clone())).await.unwrap();
                }
            }
            SelectResult::ClientDisconnected(port) => {
                connections.remove(&port);
            }
        };
    }
}
