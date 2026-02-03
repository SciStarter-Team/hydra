use sailfish::TemplateOnce;

#[derive(Debug, TemplateOnce)]
#[template(path = "site/head.stpl.html")]
pub struct Head {
    enable_webawesome: bool,
    enable_sse: bool,
    enable_alpine: bool,
    enable_quill: bool,
    enable_maplibre: bool,
    enable_slick: bool,
    enable_jquery: bool,
}

impl Default for Head {
    fn default() -> Self {
        Head {
            enable_webawesome: true,
            enable_sse: false,
            enable_alpine: false,
            enable_quill: false,
            enable_maplibre: false,
            enable_slick: false,
            enable_jquery: false,
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
    pub fn with_quill(mut self) -> Self {
        self.enable_quill = true;
        self
    }

    #[tracing::instrument]
    pub fn with_maplibre(mut self) -> Self {
        self.enable_maplibre = true;
        self
    }

    #[tracing::instrument]
    pub fn with_slick(mut self) -> Self {
        self.enable_slick = true;
        self.with_jquery()
    }

    #[tracing::instrument]
    pub fn with_jquery(mut self) -> Self {
        self.enable_jquery = true;
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

    #[tracing::instrument]
    pub fn without_quill(mut self) -> Self {
        self.enable_quill = false;
        self
    }

    #[tracing::instrument]
    pub fn without_maplibre(mut self) -> Self {
        self.enable_maplibre = false;
        self
    }

    #[tracing::instrument]
    pub fn without_slick(mut self) -> Self {
        self.enable_slick = false;
        self
    }

    #[tracing::instrument]
    pub fn without_jquery(mut self) -> Self {
        self.enable_jquery = false;
        self.without_slick()
    }
}
