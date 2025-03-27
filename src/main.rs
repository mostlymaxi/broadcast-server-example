use std::pin::Pin;

use futures::{stream::SelectAll, SinkExt, Stream, StreamExt};
use thiserror::Error;
use tokio::{
    net::{TcpListener, TcpStream},
    select,
};

use tokio_util::codec::{Framed, LinesCodec, LinesCodecError};
use tracing::{debug, info, instrument, trace, Level};

fn build_login_msg(port: u16) -> String {
    format!("LOGIN:{port}")
}

fn build_message_msg(port: u16, content: &str) -> String {
    format!("MESSAGE:{port} {content}")
}

#[derive(Debug)]
struct Event {
    kind: EventKind,
    port: u16,
}

#[derive(Debug)]
enum EventKind {
    NewConnection(TcpStream),
    NewMessage(String),
    // ClientDisconnected(u16),
}

#[derive(Error, Debug)]
pub enum EventError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    CodecError(#[from] tokio_util::codec::LinesCodecError),
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

#[instrument(level = Level::DEBUG, skip(conns), ret, err(level = Level::ERROR))]
async fn handle_event(
    event: Event,
    conns: &mut SelectAll<FramedStreamWrapped>,
) -> Result<(), EventError> {
    const MAX_CODEC_LENGTH: usize = 8192;

    match event.kind {
        EventKind::NewConnection(sock) => {
            let codec = LinesCodec::new_with_max_length(MAX_CODEC_LENGTH);
            let mut framed = Framed::new(sock, codec);
            framed.send(build_login_msg(event.port)).await?;

            let framed = FramedStreamWrapped {
                inner: framed,
                port: event.port,
            };

            conns.push(framed);
        }
        EventKind::NewMessage(msg) => {
            let msg = build_message_msg(event.port, &msg);

            for connection in conns.iter_mut() {
                if connection.port == event.port {
                    continue;
                }

                connection.inner.send(&msg).await?;

                trace!("sent message to {}", connection.port);
            }
        }
    };

    Ok(())
}

#[instrument(level = Level::DEBUG, skip(conns), ret, err(level = Level::ERROR))]
async fn select_next_event(
    listener: &TcpListener,
    conns: &mut SelectAll<FramedStreamWrapped>,
) -> Result<Event, std::io::Error> {
    // select_all will panic if the underlying iterable is empty
    if conns.is_empty() {
        debug!("no open connections");

        let (sock, addr) = listener.accept().await?;

        let event = Event {
            kind: EventKind::NewConnection(sock),
            port: addr.port(),
        };

        return Ok(event);
    }

    let event = select! {
        Ok((sock, addr)) = listener.accept() => {
            Event {
                kind: EventKind::NewConnection(sock),
                port: addr.port(),
            }
        }

        Some(res) = conns.next() => {
            if let Ok((port, msg)) = res {
                Event {
                    kind: EventKind::NewMessage(msg),
                    port,
                }
            } else {
                // connection closed
                todo!()
            }

        }
    };

    Ok(event)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), std::io::Error> {
    tracing_subscriber::fmt::init();

    const LISTEN_ADDR: &str = "localhost:8888";

    let mut conns = SelectAll::new();

    let listener = TcpListener::bind(LISTEN_ADDR).await?;

    info!("started listening on {LISTEN_ADDR}");

    loop {
        if let Ok(event) = select_next_event(&listener, &mut conns).await {
            let _ = handle_event(event, &mut conns).await;
        }
    }
}
