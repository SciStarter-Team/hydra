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
    show_projects: bool,
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
            show_projects: true,
            show_issued: false,
        }
    }
}

fn layout_for_mode(mode: &str) -> CertificateLayout {
    match mode {
        "broward" => CertificateLayout {
            background: "certificates/certificate_bkg_broward.png",
            name_y: 445.0,
            ..Default::default()
        },
        "citsciday" => CertificateLayout {
            background: "certificates/certificate_bkg.png",
            name_y: 425.0,
            ..Default::default()
        },
        "citsciday2019" => CertificateLayout {
            background: "certificates/certificate_bkg_2019.jpg",
            name_y: 470.0,
            name_x: 490.0,
            project_y: 304.0,
            project_x: 380.0,
            project_dl: 27.7,
            ..Default::default()
        },
        "inventing-tomorrow" => CertificateLayout {
            background: "certificates/certificate_bkg_inventing_tomorrow.png",
            name_y: 425.0,
            name_x: 495.0,
            show_projects: false,
            ..Default::default()
        },
        "citscicon2021" => CertificateLayout {
            background: "certificates/certificate_bkg_citscicon2021.png",
            name_y: 440.0,
            name_x: 960.0,
            shape: (1920.0, 1080.0),
            show_projects: false,
            ..Default::default()
        },
        "explorer2021" => CertificateLayout {
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
        "csm2022" => CertificateLayout {
            background: "certificates/certificate_bkg_csm2022.png",
            name_y: 280.0,
            name_x: 985.0,
            shape: (1920.0, 1080.0),
            color: Color::white(),
            show_projects: false,
            ..Default::default()
        },
        "broward_coach" => CertificateLayout {
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
        "champion" => CertificateLayout {
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
            show_projects: false,
            show_issued: true,
            ..Default::default()
        },
        "champion__watermarked" => CertificateLayout {
            background: "certificates/certificate_bkg_champion_watermarked.png",
            name_y: 260.0,
            name_x: 396.0,
            name_size: 32.0,
            issued_y: 130.0,
            issued_x: 150.0,
            issued_size: 16.0,
            issued_font: "Nunito",
            issued_align: Align::Left,
            shape: (792.0, 612.0),
            show_projects: false,
            show_issued: true,
            ..Default::default()
        },
        _ => CertificateLayout::default(),
    }
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

pub fn generate_certificate(
    name: &str,
    projects: &[&str],
    issued: Option<&str>,
    mode: &str,
) -> Result<Vec<u8>> {
    let layout = layout_for_mode(mode);

    let mut doc = Document::new();
    doc.set_title(name);

    let satisfy_data = load_embedded("fonts/Satisfy-Regular.ttf")?;
    doc.add_font_from_bytes("Satisfy", satisfy_data)?;

    let nunito_data = load_embedded("fonts/Nunito.ttf")?;
    doc.add_font_from_bytes("Nunito", nunito_data)?;

    let mut page = Page::new(layout.shape.0, layout.shape.1);

    // Load and draw background image
    let bg_data = load_embedded(layout.background)?;
    let image = if layout.background.ends_with(".jpg") || layout.background.ends_with(".jpeg") {
        Image::from_jpeg_data(bg_data)?
    } else {
        Image::from_png_data(bg_data)?
    };
    page.add_image("background", image);
    page.draw_image("background", 0.0, 0.0, layout.shape.0, layout.shape.1)?;

    // Draw name (centered at name_x)
    let name_font = font_for_name(layout.name_font);
    let name_width = measure_text(name, name_font.clone(), layout.name_size);
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
                    let issued_width =
                        measure_text(issued_str, issued_font.clone(), layout.issued_size);
                    page.text()
                        .set_font(issued_font, layout.issued_size)
                        .set_fill_color(layout.color)
                        .at(layout.issued_x - issued_width / 2.0, layout.issued_y)
                        .write(issued_str)?;
                }
            }
        }
    }

    // Draw project names
    if layout.show_projects {
        let mut y = layout.project_y;
        for proj in projects {
            page.text()
                .set_font(Font::Helvetica, layout.project_size)
                .set_fill_color(layout.color)
                .at(layout.project_x, y)
                .write(proj)?;
            y -= layout.project_dl;
        }
    }

    doc.add_page(page);
    let bytes = doc.to_bytes()?;
    Ok(bytes)
}

pub fn filename_for_mode(mode: &str) -> &'static str {
    match mode {
        "citsciday" => "Citizen Science Day Certificate.pdf",
        "citscicon2021" => "CitSciCon2021 Certificate.pdf",
        "broward_coach" => "Broward Coach Certificate.pdf",
        "citsciday2019" => "Citizen Science Day 2019 Certificate.pdf",
        "inventing-tomorrow" => "Inventing Tomorrow Certificate.pdf",
        "explorer2021" => "SciStarter Explorer 2021.pdf",
        "champion" | "champion__watermarked" => "SciStarter Champion.pdf",
        _ => "certificate.pdf",
    }
}

pub fn projects_for_mode<'a>(mode: &str, project: &'a str) -> Vec<&'a str> {
    match mode {
        "inventing-tomorrow"
        | "citscicon2021"
        | "csm2022"
        | "champion"
        | "champion__watermarked"
        | "broward_coach" => vec![],
        "citsciday2019" | "explorer2021" => project.lines().collect(),
        _ => {
            if project.is_empty() {
                vec![]
            } else {
                vec![project]
            }
        }
    }
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

    // Non-owners get watermarked champion variant or 404
    let mode = if cert.user_id != current_user.user_id {
        if cert.mode == "champion" {
            "champion__watermarked".to_string()
        } else {
            return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
        }
    } else {
        cert.mode.clone()
    };

    let projects = projects_for_mode(&mode, &cert.project);
    let project_refs: Vec<&str> = projects.iter().map(|s| *s).collect();
    let filename = filename_for_mode(&mode);

    let issued = Some(cert.created_at.as_str());

    let pdf_bytes = generate_certificate(&cert.display_name, &project_refs, issued, &mode)
        .map_err(|e| {
            eprintln!("Certificate generation error: {}", e);
            poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    Ok(Response::builder()
        .header("Content-Type", "application/pdf")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(pdf_bytes))
}
