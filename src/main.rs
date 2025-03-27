use std::pin::Pin;

use futures::{stream::SelectAll, SinkExt, Stream, StreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    select,
};

use tokio_util::codec::{Framed, LinesCodec, LinesCodecError};

fn build_login_msg(port: u16) -> String {
    format!("LOGIN:{port}")
}

fn build_message_msg(port: u16, content: &str) -> String {
    format!("MESSAGE:{port} {content}")
}

enum Event {
    NewConnection((u16, TcpStream)),
    NewMessage((u16, String)),
    // ClientDisconnected(u16),
}

struct FramedStreamWrapped {
    inner: FramedStream,
    port: u16,
}

impl Stream for FramedStreamWrapped {
    type Item = Result<(u16, String), LinesCodecError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner)
            .poll_next(cx)
            .map_ok(|msg| (self.port, msg))
    }
}

type FramedStream = Framed<TcpStream, LinesCodec>;

async fn handle_event(event: Event, conns: &mut SelectAll<FramedStreamWrapped>) {
    const MAX_CODEC_LENGTH: usize = 8192;

    match event {
        Event::NewConnection((port, c)) => {
            let codec = LinesCodec::new_with_max_length(MAX_CODEC_LENGTH);
            let mut framed = Framed::new(c, codec);
            framed.send(build_login_msg(port)).await.unwrap();

            let framed = FramedStreamWrapped {
                inner: framed,
                port,
            };

            conns.push(framed);
        }
        Event::NewMessage((port, m)) => {
            let msg = build_message_msg(port, &m);

            for connection in conns.iter_mut() {
                if connection.port == port {
                    continue;
                }

                if connection.inner.send(&msg).await.is_err() {
                    eprintln!("client {} disconnected", connection.port);
                }
            }
        }
    };
}

async fn select_next_event(
    listener: &TcpListener,
    conns: &mut SelectAll<FramedStreamWrapped>,
) -> Result<Event, std::io::Error> {
    // select_all will panic if the underlying iterable is empty
    if conns.is_empty() {
        let (sock, addr) = listener.accept().await?;

        return Ok(Event::NewConnection((addr.port(), sock)));
    }

    let event = select! {
        Ok((sock, addr)) = listener.accept() => {
            Event::NewConnection((addr.port(), sock))
        }

        Some(res) = conns.next() => {
            if let Ok((port, msg)) = res {
                Event::NewMessage((port, msg))
            } else {
                todo!()
            }

        }
    };

    Ok(event)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    const LISTEN_ADDR: &str = "localhost:8888";

    let mut conns = SelectAll::new();

    let listener = TcpListener::bind(LISTEN_ADDR).await.unwrap();

    eprintln!("started listening on {LISTEN_ADDR}");

    loop {
        if let Ok(event) = select_next_event(&listener, &mut conns).await {
            handle_event(event, &mut conns).await;
        }
    }
}
