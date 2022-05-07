use std::collections::HashMap;

use crate::communication::types::{HOIActionData, HouseOfIoTCredentials};
use futures_channel::mpsc::UnboundedSender;
use queues::*;
use tokio_tungstenite::tungstenite;

pub struct MainState {
    /// Holds the sender portion of the server mpsc channel
    /// so when we need to send messages to the server we
    /// relay the message to this channel which has an async
    /// task that does the rest by forwarding directly over
    /// the real connection.
    pub server_connections: HashMap<String, UnboundedSender<tungstenite::protocol::Message>>,
    pub server_credentials: HashMap<String, HouseOfIoTCredentials>,
    pub action_execution_queue: HashMap<String, Queue<HOIActionData>>,
    /// Keeping track of actions in progress to never
    /// have two actions running at once which won't work
    /// with some IoT servers especially HOI.
    pub action_in_progress: HashMap<String, bool>,
    /// Keeping track of what passive data requests
    /// are in progress, HOI can't have an action + passive
    /// data in progress.
    pub passive_in_progress: HashMap<String, bool>,
    /// Keeping track of how many times we skipped over
    /// passive data in order to do a mandatory force
    /// request after every 7 skips
    pub passive_data_skips: HashMap<String, u8>,
}

impl MainState {
    pub fn new() -> Self {
        Self {
            server_connections: HashMap::new(),
            server_credentials: HashMap::new(),
            action_execution_queue: HashMap::new(),
            action_in_progress: HashMap::new(),
            passive_in_progress: HashMap::new(),
            passive_data_skips: HashMap::new(),
        }
    }
}
