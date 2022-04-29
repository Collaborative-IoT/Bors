//! A simple example of hooking up stdin/stdout to a WebSocket stream.
//!
//! This example will connect to a server specified in the argument list and
//! then forward all data read on stdin to the server, printing out all data
//! received on stdout.
//!
//! Note that this is not currently optimized for performance, especially around
//! buffer management. Rather it's intended to show an example of working with a
//! client.
//!
//! You can use this example together with the `server` example.

use crate::{communication::types::HouseOfIoTCredentials, state::state_types::MainState};
use futures_util::{future, pin_mut, stream::SplitStream, StreamExt};
use std::{env, sync::Arc};
use tokio::sync::RwLock;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use uuid::Uuid;

async fn connect_and_begin_listening(
    credentials: HouseOfIoTCredentials,
    server_state: &Arc<RwLock<MainState>>,
) {
    let url_res = url::Url::parse(&credentials.connection_str);
    if let Ok(url) = url_res {
        let (mut stdin_tx, stdin_rx) = futures_channel::mpsc::unbounded();

        let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
        let (write, mut read) = ws_stream.split();

        let stdin_to_ws = stdin_rx.map(Ok).forward(write);
        let is_authed = authenticate(&mut stdin_tx, &credentials, &mut read).await;
        if is_authed {
            let mut write_state = server_state.write().await;
            write_state
                .server_connections
                .insert(Uuid::new_v4().to_string(), stdin_tx);
            //TODO add
            let ws_to_stdout = { read.for_each(|message| async {}) };
            pin_mut!(stdin_to_ws, ws_to_stdout);
            future::select(stdin_to_ws, ws_to_stdout).await;
        } else {
            //push issue message to rabbitmq
        }
    }
}

async fn authenticate(
    tx: &mut futures_channel::mpsc::UnboundedSender<Message>,
    credentials: &HouseOfIoTCredentials,
    read: &mut SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
) -> bool {
    let password_send = tx.unbounded_send(Message::Text(credentials.password.clone()));
    let name_and_type_send = tx.unbounded_send(Message::Text(credentials.name_and_type.clone()));
    let outside_name_send = tx.unbounded_send(Message::Text(credentials.outside_name.clone()));
    if password_send.is_ok() && name_and_type_send.is_ok() && outside_name_send.is_ok() {
        if let Some(msg) = read.next().await {
            if let Ok(msg) = msg {
                if msg.is_text() && msg.to_string() == "success" {
                    return true;
                }
            }
        };
    }
    false
}
