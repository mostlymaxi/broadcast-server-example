use std::collections::HashMap;

use futures::{future::select_all, FutureExt, SinkExt, TryStreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    select,
};

use tokio_util::codec::{Framed, LinesCodec};

fn build_login_msg(port: u16) -> String {
    format!("LOGIN:{port}")
}

fn build_message_msg(port: u16, content: &str) -> String {
    format!("MESSAGE:{port} {content}")
}

enum Event {
    NewConnection((u16, TcpStream)),
    NewMessage((u16, String)),
    ClientDisconnected(u16),
}

type FramedStream = Framed<TcpStream, LinesCodec>;

async fn handle_event(event: Event, conns: &mut HashMap<u16, FramedStream>) {
    const MAX_CODEC_LENGTH: usize = 8192;

    match event {
        Event::NewConnection((port, c)) => {
            let codec = LinesCodec::new_with_max_length(MAX_CODEC_LENGTH);
            let mut framed = Framed::new(c, codec);
            framed.send(build_login_msg(port)).await.unwrap();
            let _ = conns.insert(port, framed);
        }
        Event::NewMessage((port, m)) => {
            for (p, c) in conns.iter_mut() {
                if *p == port {
                    continue;
                }

                // TODO: can factor this out to clone less
                c.send(build_message_msg(port, &m)).await.unwrap();
            }
        }
        Event::ClientDisconnected(port) => {
            conns.remove(&port);
        }
    };
}

async fn select_next_event(
    listener: &TcpListener,
    connections: &mut HashMap<u16, FramedStream>,
) -> Result<Event, std::io::Error> {
    // select_all will panic if the underlying iterable is empty
    if connections.is_empty() {
        let (sock, addr) = listener.accept().await?;

        return Ok(Event::NewConnection((addr.port(), sock)));
    }

    // TODO: potentially can improve this by not recreating all the tasks
    // but having issues with select_all holding a mutable reference to the tasks
    let new_msg_task = select_all(
        connections
            .iter_mut()
            .map(|(port, c)| async move { (port, c.try_next().await) }.boxed()),
    );

    let event = select! {
        Ok((sock, addr)) = listener.accept() => {
            Event::NewConnection((addr.port(), sock))
        }

        ((port, msg), _, _) = new_msg_task => {
            if let Ok(Some(msg)) = msg {
                Event::NewMessage((*port, msg))
            } else {
                Event::ClientDisconnected(*port)
            }

        }
    };

    return Ok(event);
}

#[tokio::main]
async fn main() {
    const LISTEN_ADDR: &str = "localhost:8888";

    let mut conns: HashMap<u16, FramedStream> = HashMap::new();

    let listener = TcpListener::bind(LISTEN_ADDR).await.unwrap();

    eprintln!("started listening on {LISTEN_ADDR}");

    loop {
        if let Ok(event) = select_next_event(&listener, &mut conns).await {
            handle_event(event, &mut conns).await;
        }
    }
}
