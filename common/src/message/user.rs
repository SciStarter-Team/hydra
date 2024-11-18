#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Session {
    pub session_id: String,
    pub expires: Option<String>,
    pub data: Box<[u8]>,
}
