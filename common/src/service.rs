#[macro_export]
macro_rules! handler {
    ($group:ident, $name:literal, $fut:expr) => {
        $group
            .endpoint($name)
            .await
            .expect("Unable to create endpoint")
            .for_each_concurrent(None, |req| async move {
                if let Err(err) = $fut(req).await {
                    eprintln!("{}: {:?}", err.to_string(), err);
                }
            })
    };
}

pub mod errors {
    use async_nats::service::error::Error;
    use num_enum::{IntoPrimitive, TryFromPrimitive};

    #[derive(IntoPrimitive, TryFromPrimitive)]
    #[repr(usize)]
    pub enum ErrorCode {
        Placeholder, // Replace with the first error code needed
    }

    pub fn service_error(status: impl Into<String>, code: ErrorCode) -> Error {
        Error {
            status: status.into(),
            code: code.into(),
        }
    }
}
