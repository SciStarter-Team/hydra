#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Session {
    pub session_id: String,
    pub expires: Option<String>,
    pub data: Box<[u8]>,
}

pub trait UserInfo {
    fn user_id(&self) -> Option<&str>;
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
    pub user_id: String,
    pub handle: String,
    pub is_staff: bool,
    pub is_superuser: bool,
}

impl UserInfo for Unauthenticated {
    fn user_id(&self) -> Option<&str> {
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
    fn user_id(&self) -> Option<&str> {
        Some(self.user_id.as_str())
    }

    fn handle(&self) -> &str {
        self.handle.as_str()
    }

    fn is_staff(&self) -> bool {
        self.is_staff
    }

    fn is_superuser(&self) -> bool {
        self.is_superuser
    }
}

impl UserInfo for Option<&BasicInfo> {
    fn user_id(&self) -> Option<&str> {
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
