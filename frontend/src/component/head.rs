use sailfish::TemplateOnce;

#[derive(Debug, TemplateOnce)]
#[template(path = "site/head.stpl.html")]
pub struct Head {
    enable_webawesome: bool,
    enable_sse: bool,
    enable_alpine: bool,
}

impl Default for Head {
    fn default() -> Self {
        Head {
            enable_webawesome: false,
            enable_sse: false,
            enable_alpine: false,
        }
    }
}

impl Head {
    #[tracing::instrument]
    pub fn with_webawesome(mut self) -> Self {
        self.enable_webawesome = true;
        self
    }

    #[tracing::instrument]
    pub fn with_sse(mut self) -> Self {
        self.enable_sse = true;
        self
    }

    #[tracing::instrument]
    pub fn with_alpine(mut self) -> Self {
        self.enable_alpine = true;
        self
    }

    #[tracing::instrument]
    pub fn without_webawesome(mut self) -> Self {
        self.enable_webawesome = false;
        self
    }

    #[tracing::instrument]
    pub fn without_sse(mut self) -> Self {
        self.enable_sse = false;
        self
    }

    #[tracing::instrument]
    pub fn without_alpine(mut self) -> Self {
        self.enable_alpine = false;
        self
    }
}
