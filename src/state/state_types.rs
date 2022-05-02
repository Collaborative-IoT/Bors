use std::collections::HashMap;

use futures_channel::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite;

use crate::communication::types::HouseOfIoTCredentials;

pub struct MainState {
    /// Holds the sender portion of the server mpsc channel
    /// so when we need to send messages to the server we
    /// relay the message to this channel which has an async
    /// task that does the rest by forwarding directly over
    /// the real connection.
    pub server_connections: HashMap<String, UnboundedSender<tungstenite::protocol::Message>>,
    pub server_credentials: HashMap<String, HouseOfIoTCredentials>,
}
