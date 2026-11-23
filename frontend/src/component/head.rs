use sailfish::TemplateOnce;

#[derive(Debug, TemplateOnce)]
#[template(path = "site/head.stpl.html")]
pub struct Head {
    enable_webawesome: bool,
    enable_sse: bool,
    enable_quill: bool,
    enable_maplibre: bool,
    enable_region_editor: bool,
    enable_slick: bool,
    enable_jquery: bool,
    enable_geosearch: bool,
    enable_event_editors: bool,
}

impl Default for Head {
    fn default() -> Self {
        Head {
            enable_webawesome: true,
            enable_sse: false,
            enable_quill: false,
            enable_maplibre: false,
            enable_region_editor: false,
            enable_slick: false,
            enable_jquery: false,
            enable_geosearch: false,
            enable_event_editors: false,
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
    pub fn with_quill(mut self) -> Self {
        self.enable_quill = true;
        self
    }

    #[tracing::instrument]
    pub fn with_maplibre(mut self) -> Self {
        self.enable_maplibre = true;
        self
    }

    /// Enable the reusable map-based region editor. Implies (and builds on)
    /// `with_maplibre`, since the editor is driven by MapLibre.
    #[tracing::instrument]
    pub fn with_region_editor(mut self) -> Self {
        self.enable_region_editor = true;
        self.with_maplibre()
    }

    #[tracing::instrument]
    pub fn without_region_editor(mut self) -> Self {
        self.enable_region_editor = false;
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
    pub fn with_geosearch(mut self) -> Self {
        self.enable_geosearch = true;
        self
    }

    #[tracing::instrument]
    pub fn without_jquery(mut self) -> Self {
        self.enable_jquery = false;
        self.without_slick()
    }

    /// Enable the scripts that drive an event's editors, wherever they are:
    /// the add event form, and the event page's and the admin's editors of
    /// its dates, location, registration and admission.
    #[tracing::instrument]
    pub fn with_event_editors(mut self) -> Self {
        self.enable_event_editors = true;
        // Where an event is held is picked on a map, as an area or a place.
        self.with_region_editor()
    }

    #[tracing::instrument]
    pub fn without_geosearch(mut self) -> Self {
        self.enable_geosearch = false;
        self
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[test]
    fn the_event_editors_scripts_come_once_and_only_when_asked() {
        let scripts = ["date-rows.js", "location-picker.js", "event-answers.js"];

        let html = Head::default().with_event_editors().render_once().unwrap();
        for script in scripts {
            check!(html.matches(script).count() == 1, "{script}");
        }

        let html = Head::default().render_once().unwrap();
        for script in scripts {
            check!(!html.contains(script), "{script}");
        }
    }
}
