use sailfish::TemplateOnce;

#[derive(TemplateOnce)]
#[template(path = "site/head.stpl.html")]
pub struct Head {
    enable_shoelace: bool,
    enable_sse: bool,
    enable_alpine: bool,
}

impl Default for Head {
    fn default() -> Self {
        Head {
            enable_shoelace: true,
            enable_sse: false,
            enable_alpine: false,
        }
    }
}

impl Head {
    pub fn with_shoelace(mut self) -> Self {
        self.enable_shoelace = true;
        self
    }

    pub fn with_sse(mut self) -> Self {
        self.enable_sse = true;
        self
    }

    pub fn with_alpine(mut self) -> Self {
        self.enable_alpine = true;
        self
    }

    pub fn without_shoelace(mut self) -> Self {
        self.enable_shoelace = false;
        self
    }

    pub fn without_sse(mut self) -> Self {
        self.enable_sse = false;
        self
    }

    pub fn without_alpine(mut self) -> Self {
        self.enable_alpine = false;
        self
    }
}
