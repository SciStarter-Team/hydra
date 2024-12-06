#[macro_export]
macro_rules! handler {
    ($group:ident, $name:literal, $fut:expr) => {
        $group
            .endpoint($name)
            .await
            .expect("Unable to create endpoint")
            .for_each_concurrent(None, |req| async move {
                match $fut(&req).await {
                    Ok(val) => {
                        let Ok(payload) = flexbuffers::to_vec(val) else {
                            if let Err(x) = req
                                .respond(Err(common::service::errors::service_error(
                                    "Error while encoding response payload",
                                    common::service::errors::ErrorCode::RequestError,
                                )))
                                .await
                            {
                                eprintln!("{}: {:?}", x.to_string(), x);
                            }
                            return;
                        };

                        if let Err(x) = req.respond(Ok(payload.into())).await {
                            eprintln!("{}: {:?}", x.to_string(), x);
                        }
                    }
                    Err(err) => {
                        if let Err(x) = req
                            .respond(Err(common::service::errors::service_error(
                                err.to_string(),
                                common::service::errors::ErrorCode::HandlerError,
                            )))
                            .await
                        {
                            eprintln!("{}: {:?}", x.to_string(), x);
                        }
                    }
                }
            })
    };
}

pub mod errors {
    use std::str::FromStr;

    use async_nats::service::error::Error;
    use num_enum::{IntoPrimitive, TryFromPrimitive};

    #[derive(IntoPrimitive, TryFromPrimitive)]
    #[repr(usize)]
    pub enum ErrorCode {
        Unknown,
        RequestError,
        HandlerError,
    }

    impl FromStr for ErrorCode {
        type Err = anyhow::Error;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            let code: usize = s.parse()?;
            Ok(ErrorCode::try_from(code)?)
        }
    }

    pub fn service_error(status: impl Into<String>, code: ErrorCode) -> Error {
        Error {
            status: status.into(),
            code: code.into(),
        }
    }

    pub trait DecodeError {
        fn decode_error(self) -> Result<async_nats::Message, async_nats::service::error::Error>;
    }

    impl DecodeError for async_nats::Message {
        fn decode_error(self) -> Result<async_nats::Message, async_nats::service::error::Error> {
            if let Some(headers) = self.headers.as_ref() {
                if let Some(val) = headers.get("Nats-Service-Error") {
                    Err(service_error(
                        val.as_str(),
                        headers
                            .get("Nats-Service-Error-Code")
                            .and_then(|v| v.as_str().parse().ok())
                            .unwrap_or(ErrorCode::Unknown),
                    ))
                } else {
                    Ok(self)
                }
            } else {
                Ok(self)
            }
        }
    }
}
