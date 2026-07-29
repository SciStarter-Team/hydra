//! PDF certificate generation and download.
//!
//! Each certificate `mode` is described by a single [`CertificateMode`] config
//! in [`config_for_mode`] — its drawing [`CertificateLayout`], display title,
//! how the `project` field is parsed, whether non-owners may view it, and (if
//! so) the watermarked background to use. Generation is config-driven, so a new
//! certificate type is one entry here plus its background asset — no per-mode
//! branching scattered across functions, and no `__watermarked` twin mode.
//!
//! Requires oxidize-pdf 4.x or newer: 2.0's `Image::from_png_data` stored the
//! PNG container itself as the XObject stream while declaring `/FlateDecode`,
//! so every background failed to decode and certificates came out blank.

use anyhow::Result;
use common::message::user::{BasicInfo, CertificateGetRequest, CertificateGetResponse};
use oxidize_pdf::{Color, Document, Font, Image, Page, measure_text};
use poem::Response;
use poem::handler;
use poem::http::StatusCode;
use poem::session::Session;
use poem::web::Path;
use ulid::Ulid;

use crate::Static;

/// Where and how the text is drawn on a certificate.
struct CertificateLayout {
    background: &'static str,
    shape: (f64, f64),
    name_x: f64,
    name_y: f64,
    name_size: f64,
    name_font: &'static str,
    project_x: f64,
    project_y: f64,
    project_size: f64,
    project_dl: f64,
    issued_x: f64,
    issued_y: f64,
    issued_size: f64,
    issued_font: &'static str,
    issued_align: Align,
    color: Color,
    show_issued: bool,
}

#[derive(Clone, Copy)]
enum Align {
    Center,
    Left,
}

impl Default for CertificateLayout {
    fn default() -> Self {
        Self {
            background: "certificates/certificate_bkg_generic.png",
            shape: (990.0, 768.0),
            name_x: 650.0,
            name_y: 445.0,
            name_size: 64.0,
            name_font: "Satisfy",
            project_x: 400.0,
            project_y: 225.0,
            project_size: 16.0,
            project_dl: 20.0,
            issued_x: 300.0,
            issued_y: 300.0,
            issued_size: 32.0,
            issued_font: "Satisfy",
            issued_align: Align::Center,
            color: Color::black(),
            show_issued: false,
        }
    }
}

/// How a certificate's `project` field maps to the printed project lines.
#[derive(Clone, Copy)]
enum Projects {
    /// No project list (e.g. champion, conference certificates).
    None,
    /// The whole field is a single project name (empty → none).
    Single,
    /// One project per line.
    Lines,
}

impl Projects {
    fn parse<'a>(self, project: &'a str) -> Vec<&'a str> {
        match self {
            Projects::None => vec![],
            Projects::Lines => project.lines().collect(),
            Projects::Single => {
                if project.is_empty() {
                    vec![]
                } else {
                    vec![project]
                }
            }
        }
    }
}

/// The complete description of a certificate mode: everything needed to render,
/// name, and access-control it, in one place.
struct CertificateMode {
    layout: CertificateLayout,
    /// Display name (also the download filename, with `.pdf`).
    title: &'static str,
    projects: Projects,
    /// Whether someone other than the owner may view a (watermarked) copy.
    shareable: bool,
    /// Background to substitute when rendering a watermarked (non-owner) copy.
    watermark_background: Option<&'static str>,
}

impl Default for CertificateMode {
    fn default() -> Self {
        Self {
            layout: CertificateLayout::default(),
            title: "Certificate",
            projects: Projects::Single,
            shareable: false,
            watermark_background: None,
        }
    }
}

impl CertificateMode {
    fn filename(&self) -> String {
        format!("{}.pdf", self.title)
    }
}

/// The single registry of certificate modes.
fn config_for_mode(mode: &str) -> CertificateMode {
    match mode {
        "broward" => CertificateMode {
            layout: CertificateLayout {
                background: "certificates/certificate_bkg_broward.png",
                ..Default::default()
            },
            ..Default::default()
        },
        "citsciday" => CertificateMode {
            layout: CertificateLayout {
                background: "certificates/certificate_bkg.png",
                name_y: 425.0,
                ..Default::default()
            },
            title: "Citizen Science Day Certificate",
            ..Default::default()
        },
        "citsciday2019" => CertificateMode {
            layout: CertificateLayout {
                background: "certificates/certificate_bkg_2019.jpg",
                name_y: 470.0,
                name_x: 490.0,
                project_y: 304.0,
                project_x: 380.0,
                project_dl: 27.7,
                ..Default::default()
            },
            title: "Citizen Science Day 2019 Certificate",
            projects: Projects::Lines,
            ..Default::default()
        },
        "inventing-tomorrow" => CertificateMode {
            layout: CertificateLayout {
                background: "certificates/certificate_bkg_inventing_tomorrow.png",
                name_y: 425.0,
                name_x: 495.0,
                ..Default::default()
            },
            title: "Inventing Tomorrow Certificate",
            projects: Projects::None,
            ..Default::default()
        },
        "citscicon2021" => CertificateMode {
            layout: CertificateLayout {
                background: "certificates/certificate_bkg_citscicon2021.png",
                name_y: 440.0,
                name_x: 960.0,
                shape: (1920.0, 1080.0),
                ..Default::default()
            },
            title: "CitSciCon2021 Certificate",
            projects: Projects::None,
            ..Default::default()
        },
        "explorer2021" => CertificateMode {
            layout: CertificateLayout {
                background: "certificates/certificate_bkg_explorer2021.png",
                name_y: 670.0,
                name_x: 960.0,
                name_size: 72.0,
                project_y: 280.0,
                project_size: 24.0,
                project_dl: 30.0,
                shape: (1920.0, 1080.0),
                ..Default::default()
            },
            title: "SciStarter Explorer 2021",
            projects: Projects::Lines,
            ..Default::default()
        },
        "csm2022" => CertificateMode {
            layout: CertificateLayout {
                background: "certificates/certificate_bkg_csm2022.png",
                name_y: 280.0,
                name_x: 985.0,
                shape: (1920.0, 1080.0),
                color: Color::white(),
                ..Default::default()
            },
            projects: Projects::None,
            ..Default::default()
        },
        "broward_coach" => CertificateMode {
            layout: CertificateLayout {
                background: "certificates/certificate_bkg_broward_coach.png",
                name_y: 720.0,
                name_x: 1000.0,
                name_size: 128.0,
                issued_y: 220.0,
                issued_x: 1540.0,
                shape: (2000.0, 1414.0),
                show_issued: true,
                ..Default::default()
            },
            title: "Broward Coach Certificate",
            projects: Projects::None,
            ..Default::default()
        },
        "champion" => CertificateMode {
            layout: CertificateLayout {
                background: "certificates/certificate_bkg_champion.png",
                name_y: 260.0,
                name_x: 396.0,
                name_size: 32.0,
                issued_y: 130.0,
                issued_x: 150.0,
                issued_size: 16.0,
                issued_font: "Nunito",
                issued_align: Align::Left,
                shape: (792.0, 612.0),
                show_issued: true,
                ..Default::default()
            },
            title: "Citizen Science Champion",
            projects: Projects::None,
            shareable: true,
            watermark_background: Some("certificates/certificate_bkg_champion_watermarked.png"),
        },
        _ => CertificateMode::default(),
    }
}

/// Display title for a mode (for listing certificates in the UI).
pub fn title_for_mode(mode: &str) -> String {
    config_for_mode(mode).title.to_string()
}

fn load_embedded(path: &str) -> Result<Vec<u8>> {
    Static::get(path)
        .map(|f| f.data.to_vec())
        .ok_or_else(|| anyhow::anyhow!("Certificate asset not found: {}", path))
}

fn font_for_name(name: &str) -> Font {
    match name {
        "Satisfy" => Font::Custom("Satisfy".to_string()),
        "Nunito" => Font::Custom("Nunito".to_string()),
        _ => Font::Helvetica,
    }
}

fn generate_certificate(
    config: &CertificateMode,
    name: &str,
    projects: &[&str],
    issued: Option<&str>,
    watermarked: bool,
) -> Result<Vec<u8>> {
    let layout = &config.layout;
    let background = if watermarked {
        config.watermark_background.unwrap_or(layout.background)
    } else {
        layout.background
    };

    let mut doc = Document::new();
    doc.set_title(name);

    let satisfy_data = load_embedded("fonts/Satisfy-Regular.ttf")?;
    doc.add_font_from_bytes("Satisfy", satisfy_data)?;

    let nunito_data = load_embedded("fonts/Nunito.ttf")?;
    doc.add_font_from_bytes("Nunito", nunito_data)?;

    let mut page = Page::new(layout.shape.0, layout.shape.1);

    // Load and draw background image
    let bg_data = load_embedded(background)?;
    let image = if background.ends_with(".jpg") || background.ends_with(".jpeg") {
        Image::from_jpeg_data(bg_data)?
    } else {
        Image::from_png_data(bg_data)?
    };
    page.add_image("background", image);
    page.draw_image("background", 0.0, 0.0, layout.shape.0, layout.shape.1)?;

    // Draw name (centered at name_x)
    let name_font = font_for_name(layout.name_font);
    let name_width = measure_text(name, &name_font, layout.name_size);
    page.text()
        .set_font(name_font, layout.name_size)
        .set_fill_color(layout.color)
        .at(layout.name_x - name_width / 2.0, layout.name_y)
        .write(name)?;

    // Draw issued date
    if layout.show_issued {
        if let Some(issued_str) = issued {
            let issued_font = font_for_name(layout.issued_font);
            match layout.issued_align {
                Align::Left => {
                    page.text()
                        .set_font(issued_font, layout.issued_size)
                        .set_fill_color(layout.color)
                        .at(layout.issued_x, layout.issued_y)
                        .write(issued_str)?;
                }
                Align::Center => {
                    let issued_width = measure_text(issued_str, &issued_font, layout.issued_size);
                    page.text()
                        .set_font(issued_font, layout.issued_size)
                        .set_fill_color(layout.color)
                        .at(layout.issued_x - issued_width / 2.0, layout.issued_y)
                        .write(issued_str)?;
                }
            }
        }
    }

    // Draw project names (the parsed list — empty for modes without projects)
    let mut y = layout.project_y;
    for proj in projects {
        page.text()
            .set_font(Font::Helvetica, layout.project_size)
            .set_fill_color(layout.color)
            .at(layout.project_x, y)
            .write(proj)?;
        y -= layout.project_dl;
    }

    doc.add_page(page);
    let bytes = doc.to_bytes()?;
    Ok(bytes)
}

#[handler]
pub async fn download_certificate(
    session: &Session,
    Path(certificate_id): Path<String>,
) -> poem::Result<Response> {
    let current_user: Option<BasicInfo> = session.get("basic_info");
    let current_user =
        current_user.ok_or_else(|| poem::Error::from_status(StatusCode::UNAUTHORIZED))?;

    let cert_ulid: Ulid = certificate_id
        .parse()
        .map_err(|_| poem::Error::from_status(StatusCode::BAD_REQUEST))?;

    let cert: CertificateGetResponse = common::service::request(
        "user.certificates.get",
        CertificateGetRequest {
            certificate_id: cert_ulid,
        },
    )
    .await
    .map_err(|_| poem::Error::from_status(StatusCode::NOT_FOUND))?;

    let config = config_for_mode(&cert.mode);

    // The owner gets the real certificate; anyone else gets a watermarked copy
    // only if the mode is shareable, otherwise nothing.
    let watermarked = cert.user_id != current_user.user_id;
    if watermarked && !config.shareable {
        return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
    }

    let projects = config.projects.parse(&cert.project);
    let pdf_bytes = generate_certificate(
        &config,
        &cert.display_name,
        &projects,
        Some(cert.created_at.as_str()),
        watermarked,
    )
    .map_err(|e| {
        eprintln!("Certificate generation error: {}", e);
        poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder()
        .header("Content-Type", "application/pdf")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", config.filename()),
        )
        .body(pdf_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn champion_is_shareable_with_watermark() {
        let c = config_for_mode("champion");
        assert_eq!(c.title, "Citizen Science Champion");
        assert!(c.shareable);
        assert!(c.watermark_background.is_some());
        assert!(matches!(c.projects, Projects::None));
    }

    #[test]
    fn unknown_mode_is_private_default() {
        let c = config_for_mode("totally-unknown-mode");
        assert_eq!(c.title, "Certificate");
        assert!(!c.shareable);
        assert!(c.watermark_background.is_none());
    }

    #[test]
    fn project_parsing() {
        assert!(Projects::None.parse("a\nb").is_empty());
        assert_eq!(Projects::Single.parse("Project X"), vec!["Project X"]);
        assert!(Projects::Single.parse("").is_empty());
        assert_eq!(Projects::Lines.parse("a\nb\nc"), vec!["a", "b", "c"]);
    }

    /// Exercises the full config-driven generation path for the live champion
    /// mode, including the watermarked (non-owner) variant.
    #[test]
    fn generates_champion_pdf_owner_and_watermarked() {
        let config = config_for_mode("champion");

        let owner =
            generate_certificate(&config, "Ada Lovelace", &[], Some("2026-06-24"), false).unwrap();
        assert!(owner.starts_with(b"%PDF"));

        let watermarked =
            generate_certificate(&config, "Ada Lovelace", &[], Some("2026-06-24"), true).unwrap();
        assert!(watermarked.starts_with(b"%PDF"));
    }
}
