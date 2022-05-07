use std::sync::Arc;

use futures::lock::Mutex;
use tokio::sync::RwLock;

use crate::{integration, state::state_types::MainState};

use super::types::GeneralMessage;

pub async fn route_rabbit_message(
    msg: GeneralMessage,
    server_state: &Arc<RwLock<MainState>>,
    publish_channel: &Arc<Mutex<lapin::Channel>>,
) {
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
        "action_hoi" => {
            if let Ok(action_data) = serde_json::from_str(&msg.data) {
                integration::house_of_iot::queue_up_action_execution(server_state, action_data)
                    .await;
            }
        }
        _ => {}
    }
}
