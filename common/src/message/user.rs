use std::collections::BTreeMap;

use anyhow::Error;
use serde_json::Value as JsonValue;
use ulid::Ulid;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Signup {
    pub handle: String,
    pub email: String,
    pub password: String,
    pub newsletter: bool,
    pub zipcode: String,
    pub protected: bool,
    pub point: Option<String>,
    pub tags: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Login {
    pub email: String,
    pub password: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Session {
    pub session_id: String,
    pub expires: Option<String>,
    pub data: Box<[u8]>,
}

impl Session {
    pub fn new(
        session_id: impl Into<String>,
        expires: impl Into<Option<String>>,
        data: &BTreeMap<String, JsonValue>,
    ) -> Result<Session, Error> {
        Ok(Session {
            session_id: session_id.into(),
            expires: expires.into(),
            data: flexbuffers::to_vec(data)?.into_boxed_slice(),
        })
    }

    pub fn decode(&self) -> Result<BTreeMap<String, JsonValue>, Error> {
        Ok(flexbuffers::from_slice(&self.data)?)
    }

    pub fn encode(&mut self, data: BTreeMap<String, JsonValue>) -> Result<(), Error> {
        self.data = flexbuffers::to_vec(data)?.into_boxed_slice();
        Ok(())
    }
}

pub trait UserInfo {
    fn user_id(&self) -> Option<Ulid>;
    fn handle(&self) -> &str;
    fn is_staff(&self) -> bool;
    fn is_superuser(&self) -> bool;

    fn can_access_admin(&self) -> bool {
        self.is_staff() | self.is_superuser()
    }
}

pub struct Unauthenticated;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BasicInfo {
    pub user_id: Ulid,
    pub handle: String,
    pub is_staff: bool,
    pub is_superuser: bool,
}

impl UserInfo for Unauthenticated {
    fn user_id(&self) -> Option<Ulid> {
        None
    }

    fn handle(&self) -> &str {
        "Unknown"
    }

    fn is_staff(&self) -> bool {
        false
    }

    fn is_superuser(&self) -> bool {
        false
    }
}

impl UserInfo for BasicInfo {
    fn user_id(&self) -> Option<Ulid> {
        Some(self.user_id)
    }

    fn handle(&self) -> &str {
        self.handle.as_str()
    }

    fn is_staff(&self) -> bool {
        self.is_staff || self.is_superuser
    }

    fn is_superuser(&self) -> bool {
        self.is_superuser
    }
}

impl UserInfo for Option<BasicInfo> {
    fn user_id(&self) -> Option<Ulid> {
        match self {
            Some(info) => info.user_id(),
            None => Unauthenticated.user_id(),
        }
    }

    fn handle(&self) -> &str {
        match self {
            Some(info) => info.handle(),
            None => Unauthenticated.handle(),
        }
    }

    fn is_staff(&self) -> bool {
        match self {
            Some(info) => info.is_staff(),
            None => Unauthenticated.is_staff(),
        }
    }

    fn is_superuser(&self) -> bool {
        match self {
            Some(info) => info.is_superuser(),
            None => Unauthenticated.is_superuser(),
        }
    }
}

impl UserInfo for Option<&BasicInfo> {
    fn user_id(&self) -> Option<Ulid> {
        match self {
            Some(info) => info.user_id(),
            None => Unauthenticated.user_id(),
        }
    }

    fn handle(&self) -> &str {
        match self {
            Some(info) => info.handle(),
            None => Unauthenticated.handle(),
        }
    }

    fn is_staff(&self) -> bool {
        match self {
            Some(info) => info.is_staff(),
            None => Unauthenticated.is_staff(),
        }
    }

    fn is_superuser(&self) -> bool {
        match self {
            Some(info) => info.is_superuser(),
            None => Unauthenticated.is_superuser(),
        }
    }
}
