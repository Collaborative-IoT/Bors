use std::sync::Arc;

use futures::lock::Mutex;
use tokio::sync::RwLock;

use crate::{communication::types::DisconnectMsg, integration, state::state_types::MainState};

use super::types::GeneralMessage;

pub async fn route_rabbit_message(
    msg: GeneralMessage,
    server_state: &Arc<RwLock<MainState>>,
    publish_channel: &Arc<Mutex<lapin::Channel>>,
) {
    println!("routing message...");
    match msg.category.as_str() {
        "connect_hoi" => {
            // tries to connect to the IoT server and sends the response
            // to the main server via rabbitmq
            if let Ok(credentials) = serde_json::from_str(&msg.data) {
                integration::house_of_iot::connect_and_begin_listening(
                    credentials,
                    server_state,
                    publish_channel,
                )
                .await;
            }
        }
        "disconnect_hoi" => {
            if let Ok(disconnect_msg) = serde_json::from_str(&msg.data) {
                let disconnect_msg: DisconnectMsg = disconnect_msg;
                let mut write_state = server_state.write().await;
                // clean up iot server from state
                // which will automatically stop each
                // task associated with the iot server
                write_state
                    .action_in_progress
                    .remove(&disconnect_msg.server_id);
                write_state
                    .passive_data_skips
                    .remove(&disconnect_msg.server_id);
                write_state
                    .passive_in_progress
                    .remove(&disconnect_msg.server_id);
                write_state
                    .server_connections
                    .remove(&disconnect_msg.server_id);
                write_state
                    .server_credentials
                    .remove(&disconnect_msg.server_id);
                write_state
                    .action_execution_queue
                    .remove(&disconnect_msg.server_id);
            }
        }
        "action_hoi" => {
            if let Ok(action_data) = serde_json::from_str(&msg.data) {
                integration::house_of_iot::queue_up_action_execution(server_state, action_data)
                    .await;
            }
        }
        _ => {}
    }
}
