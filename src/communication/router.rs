use std::sync::Arc;

use futures::lock::{Mutex, MutexGuard};
use lapin::Channel;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;

use crate::{
    communication::{rabbit, types::HOIRelationReq},
    integration,
    state::state_types::MainState,
};

use super::types::GeneralMessage;

pub async fn route_rabbit_message(
    msg: GeneralMessage,
    server_state: &Arc<RwLock<MainState>>,
    publish_channel: &Arc<Mutex<lapin::Channel>>,
) {
    println!("routing message...");
    println!("{:?}", msg);
    match msg.category.as_str() {
        "connect_hoi" => {
            // tries to connect to the IoT server and sends the response
            // to the main server via rabbitmq
            if let Ok(credentials) = serde_json::from_str(&msg.data) {
                tokio::task::spawn(integration::house_of_iot::connect_and_begin_listening(
                    credentials,
                    server_state.clone(),
                    publish_channel.clone(),
                ));
            }
        }
        "disconnect_hoi" => {
            let mut write_state = server_state.write().await;
            let mut channel = publish_channel.lock().await;
            // clean up iot server from state
            // which will automatically stop each
            // task associated with the iot server
            write_state.action_in_progress.remove(&msg.server_id);
            write_state.passive_data_skips.remove(&msg.server_id);
            write_state.passive_in_progress.remove(&msg.server_id);
            write_state.server_connections.remove(&msg.server_id);
            write_state.server_credentials.remove(&msg.server_id);
            write_state.action_execution_queue.remove(&msg.server_id);
            post_mq_msg(
                &mut channel,
                msg.server_id.clone(),
                String::new(),
                "disconnected".to_owned(),
            )
            .await;
        }
        "action_hoi" => {
            if let Ok(action_data) = serde_json::from_str(&msg.data) {
                tokio::task::spawn(integration::house_of_iot::queue_up_action_execution(
                    server_state.clone(),
                    action_data,
                    msg.server_id,
                ));
            }
        }
        "add_relation" | "remove_relation" => {
            let write_state = server_state.write().await;
            let mut channel = publish_channel.lock().await;
            if let Some(tx) = write_state.server_connections.get(&msg.server_id) {
                tx.unbounded_send(Message::Text(
                    serde_json::to_string(&HOIRelationReq {
                        category: msg.category.clone(),
                        data: msg.data.clone(),
                    })
                    .unwrap(),
                ))
                .unwrap_or_default();
            }
            post_mq_msg(
                &mut channel,
                msg.server_id.clone(),
                msg.category.clone(),
                "relation-request-made".to_owned(),
            )
            .await;
        }
        _ => {}
    }
}

/// Sends message to the queue
/// so the general server can pick it up
/// and send it to the room/user that owned. this
/// request
async fn post_mq_msg(
    channel: &mut MutexGuard<'_, Channel>,
    server_id: String,
    data: String,
    category: String,
) {
    let msg = GeneralMessage {
        category,
        data,
        server_id,
    };

    rabbit::publish_message(channel, serde_json::to_string(&msg).unwrap())
        .await
        .unwrap_or_default();
}
