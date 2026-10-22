#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParticipantFile {
    pub name: String,
    pub url: String,
}
