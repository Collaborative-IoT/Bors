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

use crate::communication::rabbit;
use crate::communication::types::AuthResponse;
use crate::{communication::types::HouseOfIoTCredentials, state::state_types::MainState};
use futures::lock::Mutex;
use futures_util::{future, pin_mut, stream::SplitStream, StreamExt};
use lapin::Channel;
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
    publish_channel: &Arc<Mutex<lapin::Channel>>,
) {
    let url_res = url::Url::parse(&credentials.connection_str);
    if let Ok(url) = url_res {
        let (mut stdin_tx, stdin_rx) = futures_channel::mpsc::unbounded();

        let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
        let (write, mut read) = ws_stream.split();
        let stdin_to_ws = stdin_rx.map(Ok).forward(write);
        let is_authed = authenticate(&mut stdin_tx, &credentials, &mut read).await;
        let mut write_state = server_state.write().await;
        let publish_channel_mut = publish_channel.lock().await;

        // If authentication is successfull we should
        // relay that information directly to the message
        // broker channel
        if is_authed {
            let new_server_id = Uuid::new_v4().to_string();
            //insert our new server
            write_state
                .server_connections
                .insert(new_server_id.clone(), stdin_tx);

            //let the consumer know, that this request
            //was successful and we are awaiting commands
            //for the newly added server
            send_auth_response(
                credentials.outside_name,
                true,
                Some(new_server_id.clone()),
                &publish_channel_mut,
            )
            .await;

            //Spawn our new basic tasks
            let ws_to_stdout = {
                read.for_each(|message| async {
                    if let Ok(msg) = message {
                        let str_msg = msg.to_string();
                        route_message(str_msg, publish_channel.clone(), new_server_id.clone())
                            .await;
                    }
                })
            };
            drop(write_state);
            drop(publish_channel_mut);
            pin_mut!(stdin_to_ws, ws_to_stdout);
            future::select(stdin_to_ws, ws_to_stdout).await;
        } else {
            send_auth_response(credentials.outside_name, false, None, &publish_channel_mut).await;
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

async fn send_auth_response(
    outside_name: String,
    passed: bool,
    server_id: Option<String>,
    channel: &Channel,
) {
    let auth_response = AuthResponse {
        outside_name: outside_name,
        passed_auth: passed,
        server_id: server_id,
    };
    rabbit::publish_message(channel, serde_json::to_string(&auth_response).unwrap())
        .await
        .unwrap_or_default();
}

async fn route_message(
    msg: String,
    publish_channel: Arc<Mutex<lapin::Channel>>,
    server_id: String,
) {
    
}
