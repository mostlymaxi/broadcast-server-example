use std::pin::Pin;

use futures::{stream::SelectAll, SinkExt, Stream, StreamExt};
use tokio::{
    net::{TcpListener, TcpStream, ToSocketAddrs},
    select,
};

use tokio_util::codec::{Framed, LinesCodec, LinesCodecError};
use tracing::{debug, info, instrument, trace, Level};

use thiserror::Error;

mod util {
    pub const MAX_CODEC_LENGTH: usize = 8192;

    pub fn build_login_msg(port: u16) -> String {
        format!("LOGIN:{port}")
    }

    pub fn build_message_msg(port: u16, content: &str) -> String {
        format!("MESSAGE:{port} {content}")
    }
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
}

#[derive(Error, Debug)]
pub enum EventError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    CodecError(#[from] tokio_util::codec::LinesCodecError),
}

struct FramedStream {
    inner: Framed<TcpStream, LinesCodec>,
    port: u16,
}

impl Stream for FramedStream {
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

#[instrument(level = Level::DEBUG, skip(conns), ret, err(level = Level::ERROR))]
async fn handle_event(event: Event, conns: &mut SelectAll<FramedStream>) -> Result<(), EventError> {
    match event.kind {
        EventKind::NewConnection(sock) => {
            let codec = LinesCodec::new_with_max_length(util::MAX_CODEC_LENGTH);
            let mut framed = Framed::new(sock, codec);
            framed.send(util::build_login_msg(event.port)).await?;

            let framed = FramedStream {
                inner: framed,
                port: event.port,
            };

            conns.push(framed);
        }
        EventKind::NewMessage(msg) => {
            let msg = util::build_message_msg(event.port, &msg);

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
    conns: &mut SelectAll<FramedStream>,
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
                // pretty sure this branch will never be reached
                // leaving a panic in case i'm wrong
                unreachable!()
            }

        }
    };

    Ok(event)
}

#[instrument(level = Level::DEBUG, skip_all, ret, err(level = Level::ERROR))]
pub async fn serve<A: ToSocketAddrs>(bind: A) -> Result<(), std::io::Error> {
    let mut conns = SelectAll::new();

    let listener = TcpListener::bind(bind).await?;

    info!("started listening on {}", listener.local_addr()?);

    let event_loop = async {
        loop {
            if let Ok(event) = select_next_event(&listener, &mut conns).await {
                let _ = handle_event(event, &mut conns).await;
            }
        }
    };

    let shutdown = async {
        tokio::signal::ctrl_c()
            .await
            .expect("signal registration to work");
    };

    select! {
        _ = event_loop => {},
        _ = shutdown => {
            info!("shutting down");
        },
    }

    Ok(())
}
