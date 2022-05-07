use std::sync::Arc;

use communication::rabbit;
use futures::lock::Mutex;
use state::state_types::MainState;
use tokio::sync::RwLock;

pub mod integration {
    pub mod house_of_iot;
}
pub mod communication {
    pub mod rabbit;
    pub mod router;
    pub mod types;
}

pub mod state {
    pub mod state_types;
}

#[tokio::main]
async fn main() {
    let main_state = Arc::new(RwLock::new(MainState::new()));
    let connection = rabbit::setup_rabbit_connection().await;
    if let Ok(conn) = connection {
        let publish_channel = rabbit::setup_publish_channel(&conn).await;
        if let Ok(pub_channel) = publish_channel {
            let locked_channel = Arc::new(Mutex::new(pub_channel));
            // run forever by checking our message broker
            // and executing commands
            rabbit::setup_consume_task(&conn, main_state.clone(), locked_channel)
                .await
                .unwrap();
        }
    }
}
