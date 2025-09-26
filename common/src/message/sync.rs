#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct NextSequenceResponse {
    pub next_id: i64,
}
