use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone)]
pub struct HouseOfIoTCredentials {
    //the connection str is usually just the location
    //of the server
    pub connection_str: String,
    pub name_and_type: String,
    pub password: String,
    pub admin_password: String,
    pub outside_name: String,
    pub user_id: i32,
}

#[derive(Deserialize, Serialize)]
pub struct AuthResponse {
    pub user_id: i32,
    pub passed_auth: bool,
    pub server_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct GeneralMessage {
    pub category: String,
    pub data: String,
    pub server_id: String,
}

#[derive(Deserialize, Serialize)]
pub struct HOIBasicResponse {
    pub server_name: String,
    pub action: String,
    pub status: String,
    pub bot_name: String,
    pub target: String,
    pub target_value: String,
}

// The passive data for one HOI bot
#[derive(Deserialize, Serialize)]
pub struct HOIBasicPassiveSingle {
    pub active_status: bool,
    pub device_name: String,
    pub device_type: String,
}
#[derive(Deserialize, Serialize, Clone)]
pub struct HOIActionData {
    pub server_id: String,
    pub bot_name: String,
    pub action: String,
}
