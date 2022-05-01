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
use crate::communication::types::{
    AuthResponse, GeneralMessage, HOIBasicPassiveSingle, HOIBasicResponse,
};
use crate::{communication::types::HouseOfIoTCredentials, state::state_types::MainState};
use futures::lock::Mutex;
use futures_util::{future, pin_mut, stream::SplitStream, StreamExt};
use lapin::Channel;
use serde_json::Value;
use std::time::Duration;
use std::{env, sync::Arc};
use tokio::sync::RwLock;
use tokio::time::sleep;
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

            //Spawn our new basic task to route all
            //messages from the IoT server to relay abstracted
            //information to the main server
            let route_all_incoming_messages = {
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
            tokio::task::spawn(request_passive_data_on_interval(
                server_state.clone(),
                new_server_id.clone(),
            ));
            tokio::task::spawn(stdin_to_ws);
            route_all_incoming_messages.await;
        } else {
            send_auth_response(credentials.outside_name, false, None, &publish_channel_mut).await;
        }
    }
}

async fn request_passive_data_on_interval(server_state: Arc<RwLock<MainState>>, server_id: String) {
    loop {
        sleep(Duration::from_secs(5)).await;
        let mut write_state = server_state.write().await;
        if let Some(tx) = write_state.server_connections.get_mut(&server_id) {
            let send_res = tx.unbounded_send(Message::Text("passive_data".to_owned()));
            if send_res.is_err() {
                println!("issue sending for passive data");
            }
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
    let publish_channel_mut = publish_channel.lock().await;
    if msg == "success" || msg == "issue" {
        let response = GeneralMessage {
            category: "action_response".to_owned(),
            data: msg,
            server_id,
        };

        rabbit::publish_message(
            &publish_channel_mut,
            serde_json::to_string(&response).unwrap(),
        )
        .await
        .unwrap_or_default();
        return;
    }
    if let Ok(basic_response_from_server) = serde_json::from_str(&msg) {
        let actual_response: Value = basic_response_from_server;
        // If this is a passive data response
        if actual_response["bots"] != Value::Null {
            // we convert here to confirm we are getting the correct data from the iot server
            // before passing it along to the main general server
            if let Ok(data) = serde_json::from_value(actual_response) {
                let data: Vec<HOIBasicPassiveSingle> = data;
                let response = GeneralMessage {
                    category: "passive_data".to_owned(),
                    data: serde_json::to_string(&data).unwrap(),
                    server_id,
                };
                rabbit::publish_message(
                    &publish_channel_mut,
                    serde_json::to_string(&response).unwrap(),
                )
                .await
                .unwrap_or_default();
            }
        }
    }
}
