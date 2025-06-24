use anyhow::Error;
use ulid::Ulid;

pub trait AsUlid {
    fn as_ulid(&self) -> Result<Ulid, Error>;
}

impl AsUlid for String {
    fn as_ulid(&self) -> Result<Ulid, Error> {
        Ok(self.parse()?)
    }
}

impl AsUlid for &String {
    fn as_ulid(&self) -> Result<Ulid, Error> {
        Ok(self.parse()?)
    }
}

impl AsUlid for &str {
    fn as_ulid(&self) -> Result<Ulid, Error> {
        Ok(self.parse()?)
    }
}

impl AsUlid for Ulid {
    fn as_ulid(&self) -> Result<Ulid, Error> {
        Ok(*self)
    }
}
