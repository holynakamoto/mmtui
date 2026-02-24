use futures_util::{SinkExt, StreamExt};
use std::env;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr = env::var("MMTUI_CHAT_BIND").unwrap_or_else(|_| "0.0.0.0:8787".to_string());
    let listener = TcpListener::bind(&addr).await?;
    let (tx, _rx) = broadcast::channel::<String>(512);

    eprintln!("chat relay listening on {addr}");

    loop {
        let (stream, peer) = listener.accept().await?;
        let tx = tx.clone();
        let rx = tx.subscribe();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, tx, rx).await {
                eprintln!("client {peer} disconnected: {e}");
            }
        });
    }
}

async fn handle_client(
    stream: TcpStream,
    tx: broadcast::Sender<String>,
    mut rx: broadcast::Receiver<String>,
) -> anyhow::Result<()> {
    let ws = accept_async(stream).await?;
    let (mut write, mut read) = ws.split();

    loop {
        tokio::select! {
            inbound = read.next() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        let _ = tx.send(text.to_string());
                    }
                    Some(Ok(Message::Binary(_))) => {}
                    Some(Ok(Message::Ping(_))) => {}
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Frame(_))) => {}
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => return Err(e.into()),
                }
            }
            outbound = rx.recv() => {
                match outbound {
                    Ok(text) => {
                        write.send(Message::Text(text.into())).await?;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    Ok(())
}
