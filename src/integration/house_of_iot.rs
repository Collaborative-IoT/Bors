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
    AuthResponse, GeneralMessage, HOIActionData, HOIBasicPassiveSingle,
};
use crate::{communication::types::HouseOfIoTCredentials, state::state_types::MainState};
use futures::lock::Mutex;
use futures_channel::mpsc::UnboundedSender;
use futures_util::{stream::SplitStream, StreamExt};
use lapin::Channel;
use queues::*;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
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

pub async fn connect_and_begin_listening(
    credentials: HouseOfIoTCredentials,
    server_state: &Arc<RwLock<MainState>>,
    publish_channel: &Arc<Mutex<lapin::Channel>>,
) {
    let url_res = url::Url::parse(&credentials.connection_str);
    println!("Connecting...");
    if let Ok(url) = url_res {
        let (mut stdin_tx, stdin_rx) = futures_channel::mpsc::unbounded();

        let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
        let (write, mut read) = ws_stream.split();
        let stdin_to_ws = stdin_rx.map(Ok).forward(write);
        tokio::task::spawn(stdin_to_ws);
        let is_authed = authenticate(&mut stdin_tx, &credentials, &mut read).await;
        let mut write_state = server_state.write().await;
        let publish_channel_mut = publish_channel.lock().await;
        // If authentication is successfull we should
        // relay that information directly to the message
        // broker channel
        if is_authed {
            println!("Authenticated...");
            let new_server_id = Uuid::new_v4().to_string();
            //insert our new server
            write_state
                .server_connections
                .insert(new_server_id.clone(), stdin_tx);
            write_state
                .server_credentials
                .insert(new_server_id.clone(), credentials.clone());
            write_state
                .action_execution_queue
                .insert(new_server_id.clone(), queue![]);
            write_state
                .action_in_progress
                .insert(new_server_id.clone(), false);
            write_state
                .passive_in_progress
                .insert(new_server_id.clone(), false);
            //let the consumer know, that this request
            //was successful and we are awaiting commands
            //for the newly added server
            send_auth_response(
                credentials.user_id,
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
                        route_message(
                            str_msg,
                            publish_channel.clone(),
                            server_state.clone(),
                            new_server_id.clone(),
                        )
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
            tokio::task::spawn(execute_actions_on_interval(
                server_state.clone(),
                new_server_id.clone(),
            ));
            println!("Spawned and waiting...");
            route_all_incoming_messages.await;
        } else {
            send_auth_response(credentials.user_id, false, None, &publish_channel_mut).await;
        }
    }
}

/// We need to queue up every action instead
/// of executing it directly due to the s
pub async fn queue_up_action_execution(
    server_state: &Arc<RwLock<MainState>>,
    action_data: HOIActionData,
) {
    let mut write_state = server_state.write().await;
    if let Some(data) = write_state
        .action_execution_queue
        .get_mut(&action_data.server_id)
    {
        // Note: We need to account for circular removal
        // when we add an item to the queue, another item
        // could be removed "in the case of a circular buffer"
        data.add(action_data).unwrap_or_default();
    }
}

pub async fn execute_actions_on_interval(server_state: Arc<RwLock<MainState>>, server_id: String) {
    loop {
        sleep(Duration::from_millis(200)).await;
        let mut write_state = server_state.write().await;
        // Only request action if nothing is blocking us from requesting
        // Things that could block us from requesting:
        // 1. Being in the middle of a passive request
        // 2. Being in the middle of an action request
        if let Some(action_status) = write_state.action_in_progress.get_mut(&server_id) {
            if *action_status == true {
                continue;
            }
        }
        if let Some(passive_status) = write_state.passive_in_progress.get_mut(&server_id) {
            if *passive_status == true {
                continue;
            }
        }

        if let Some(server_action_queue) = write_state.action_execution_queue.get_mut(&server_id) {
            //get the most recent queued action and execute
            let action_data_res = server_action_queue.remove();
            if let Ok(action_data) = action_data_res {
                drop(server_action_queue);
                write_state
                    .action_in_progress
                    .insert(server_id.clone(), true);
                if let Some(tx) = write_state.server_connections.get_mut(&server_id) {
                    execute_action(tx, action_data).await;
                }
            }
        }
        //if we don't have a server queue, we don't have this server
        //running anymore so we can just stop this task.
        else {
            return;
        }
    }
}

pub async fn execute_action(tx: &mut UnboundedSender<Message>, action_data: HOIActionData) {
    // Normal HOI bot control protocol
    tx.unbounded_send(Message::text("bot_control".to_owned()))
        .unwrap_or_default();
    tx.unbounded_send(Message::text(action_data.action))
        .unwrap_or_default();
    tx.unbounded_send(Message::text(action_data.bot_name))
        .unwrap_or_default();
}

async fn request_passive_data_on_interval(server_state: Arc<RwLock<MainState>>, server_id: String) {
    loop {
        println!("requesting passive data");
        sleep(Duration::from_secs(1)).await;
        let mut write_state = server_state.write().await;

        // Only request passive data if nothing is blocking us from requesting
        // Things that could block us from requesting:
        // 1. Being in the middle of a passive request
        // 2. Being in the middle of an action request
        if let Some(action_status) = write_state.action_in_progress.get_mut(&server_id) {
            if *action_status == true {
                if let Some(skips) = write_state.passive_data_skips.get_mut(&server_id) {
                    *skips += 1;
                }
                continue;
            }
        }
        if let Some(passive_status) = write_state.passive_in_progress.get_mut(&server_id) {
            if *passive_status == true {
                continue;
            }
        }
        if let Some(tx) = write_state.server_connections.get_mut(&server_id) {
            let send_res = tx.unbounded_send(Message::Text("passive_data".to_owned()));
            //set the in progress flag
            write_state
                .passive_in_progress
                .insert(server_id.clone(), true);
            if send_res.is_err() {
                println!("issue sending for passive data");
            }
        }
        // if we don't have a sending channel, we don't have this server
        //running anymore so we can just stop this task.
        else {
            return;
        }

        if let Some(skips) = write_state.passive_data_skips.get_mut(&server_id) {
            *skips = 0;
        }
    }
}

pub async fn authenticate(
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
                println!("{}", msg);
                if msg.is_text() && msg.to_string() == "success" {
                    return true;
                }
            }
        };
    }
    false
}

async fn send_auth_response(
    user_id: i32,
    passed: bool,
    server_id: Option<String>,
    channel: &Channel,
) {
    let auth_response = AuthResponse {
        user_id,
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
    server_state: Arc<RwLock<MainState>>,
    server_id: String,
) {
    let publish_channel_mut = publish_channel.lock().await;

    if let Ok(response_from_server) = serde_json::from_str(&msg) {
        let actual_response: Value = response_from_server;
        let mut write_state = server_state.write().await;
        println!("{}", msg);
        // If an action that was requested requires admin authentication
        // we should provide such authentication.
        //
        //There are technically 2 different authentications one for
        //super admin and one for regular admin, but this integration
        //is only for doing things that requires regular admin auth.
        if actual_response["status"] != Value::Null
            && actual_response["status"].to_string() == "needs-admin-auth"
        {
            let mut creds = None;
            if let Some(cred) = write_state.server_credentials.get(&server_id) {
                creds = Some(cred.clone());
            }
            if let Some(tx) = write_state.server_connections.get_mut(&server_id) {
                if let Some(creds) = creds {
                    tx.unbounded_send(Message::Text(creds.admin_password.clone()))
                        .unwrap_or_default();
                }
            }
            return;
        }
        // If this is a passive data response
        if actual_response["bots"] != Value::Null {
            // we convert here to confirm we are getting the correct data from the iot server
            // before passing it along to the main general server
            clear_old_in_progress(&mut write_state, server_id.clone());
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
            return;
        }
        // If this is a response for an action execution
        if actual_response["bot_name"] != Value::Null
            && actual_response["action"] != Value::Null
            && actual_response["status"] != Value::Null
        {
            clear_old_in_progress(&mut write_state, server_id.clone());
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
    }
}
/// Used to set the in-progess flags to false, to allow the next action/passive data request
/// to be executed, since only one can happen at a time.
/// These must be false since neither can be true at the same time
/// and after every response(from the iot server) then each passive data/action cycle is over.
///
/// You can read more about how we manage action/passive data requests in the docs.
pub fn clear_old_in_progress(write_state: &mut MainState, server_id: String) {
    write_state
        .action_in_progress
        .insert(server_id.clone(), false);
    write_state.passive_in_progress.insert(server_id, false);
}
