use chrono::Local;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Clone)]
pub enum ChatCommand {
    Send { body: String, message_id: String },
}

#[derive(Debug, Clone)]
pub enum ChatEvent {
    Connected,
    Disconnected,
    Message(ChatWireMessage),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatWireMessage {
    pub id: String,
    pub room: String,
    pub author: String,
    pub body: String,
    pub timestamp: String,
}

#[derive(Debug)]
pub struct ChatWorker {
    pub url: String,
    pub room: String,
    pub username: String,
    pub commands: mpsc::Receiver<ChatCommand>,
    pub events: mpsc::Sender<ChatEvent>,
}

impl ChatWorker {
    pub async fn run(mut self) {
        let mut pending: Vec<ChatCommand> = Vec::new();
        loop {
            match connect_async(self.url.as_str()).await {
                Ok((stream, _)) => {
                    let _ = self.events.send(ChatEvent::Connected).await;
                    let (mut write, mut read) = stream.split();

                    for cmd in pending.drain(..) {
                        if let Err(e) = send_command(&mut write, &self.room, &self.username, cmd).await {
                            let _ = self.events.send(ChatEvent::Error(format!("chat send failed: {e}"))).await;
                        }
                    }

                    loop {
                        tokio::select! {
                            maybe_cmd = self.commands.recv() => {
                                let Some(cmd) = maybe_cmd else {
                                    return;
                                };
                                if let Err(e) = send_command(&mut write, &self.room, &self.username, cmd.clone()).await {
                                    pending.push(cmd);
                                    let _ = self.events.send(ChatEvent::Error(format!("chat send failed: {e}"))).await;
                                    let _ = self.events.send(ChatEvent::Disconnected).await;
                                    break;
                                }
                            }
                            inbound = read.next() => {
                                match inbound {
                                    Some(Ok(Message::Text(text))) => {
                                        match serde_json::from_str::<ChatWireMessage>(&text) {
                                            Ok(msg) if msg.room == self.room => {
                                                let _ = self.events.send(ChatEvent::Message(msg)).await;
                                            }
                                            Ok(_) => {}
                                            Err(e) => {
                                                let _ = self.events.send(ChatEvent::Error(format!("chat parse error: {e}"))).await;
                                            }
                                        }
                                    }
                                    Some(Ok(Message::Close(_))) | None => {
                                        let _ = self.events.send(ChatEvent::Disconnected).await;
                                        break;
                                    }
                                    Some(Ok(_)) => {}
                                    Some(Err(e)) => {
                                        let _ = self.events.send(ChatEvent::Error(format!("chat read failed: {e}"))).await;
                                        let _ = self.events.send(ChatEvent::Disconnected).await;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = self
                        .events
                        .send(ChatEvent::Error(format!("chat connect failed: {e}")))
                        .await;
                    let _ = self.events.send(ChatEvent::Disconnected).await;
                }
            }

            loop {
                match self.commands.try_recv() {
                    Ok(cmd) => pending.push(cmd),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => return,
                }
            }
            sleep(Duration::from_secs(2)).await;
        }
    }
}

async fn send_command<S>(
    write: &mut S,
    room: &str,
    username: &str,
    cmd: ChatCommand,
) -> Result<(), String>
where
    S: futures_util::sink::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    match cmd {
        ChatCommand::Send { body, message_id } => {
            let payload = ChatWireMessage {
                id: message_id,
                room: room.to_string(),
                author: username.to_string(),
                body,
                timestamp: Local::now().format("%H:%M").to_string(),
            };
            let text = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
            write
                .send(Message::Text(text.into()))
                .await
                .map_err(|e| e.to_string())
        }
    }
}
