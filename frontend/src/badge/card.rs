//! The picture a link to an earner's badge page carries on social media: a
//! 1200x630 card, the size Facebook and LinkedIn show full width, with the
//! badge's artwork, what was earned, and where to earn it.
//!
//! It is drawn here from the same words the page uses rather than kept as a
//! file per badge, so every badge and every translation has one, and a new
//! badge has one as soon as it exists. scistarter.org draws the same card.

use std::sync::{Arc, LazyLock};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use resvg::{tiny_skia, usvg};

use anyhow::{Error, anyhow};

pub const WIDTH: u32 = 1200;
pub const HEIGHT: u32 = 630;

/// The site's typeface, the same file the site serves. Variable: the
/// weights below are instances of it.
static NUNITO: &[u8] = include_bytes!("../../static/fonts/Nunito.ttf");

static LOGO: &[u8] = include_bytes!("../../static/img/logo/logo-rgb-coral-blue.svg");

static FONTS: LazyLock<Arc<usvg::fontdb::Database>> = LazyLock::new(|| {
    let mut fonts = usvg::fontdb::Database::new();
    fonts.load_font_data(NUNITO.to_vec());
    Arc::new(fonts)
});

/// Where the text column starts, right of the artwork's panel, and how wide
/// it may run.
const PANEL_WIDTH: f32 = 520.0;
const TEXT_X: f32 = 580.0;
const TEXT_WIDTH: f32 = 560.0;

/// The text column is centred in the space above the logo.
const LOGO_Y: f32 = 540.0;
const TEXT_BOTTOM: f32 = 520.0;

/// The badge's name is set as large as fits in this many lines, down to
/// [`NAME_MIN_SIZE`].
const NAME_MAX_LINES: usize = 3;
const NAME_MIN_SIZE: f32 = 40.0;

const CORAL: &str = "#D63511";
const NAVY: &str = "#24284E";
const ACTION_BLUE: &str = "#037F9B";
const LIGHT_BLUE: &str = "#E8F7FF";

/// What a card says, in the language it is drawn for.
pub struct CardText {
    /// The line above the name: what kind of thing was earned.
    pub kicker: String,
    /// The badge's name.
    pub name: String,
    /// Where a reader can earn it too, for a badge with a training behind
    /// it.
    pub invite: Option<String>,
}

impl CardText {
    /// The title of a link carrying this card: what was earned, and which
    /// badge.
    pub fn title(&self) -> String {
        format!("{}: {}", self.kicker, self.name)
    }
}

/// One run of text: its size in pixels, its weight on the font's `wght`
/// axis, and extra space after each character.
struct Style {
    size: f32,
    weight: f32,
    spacing: f32,
    color: &'static str,
}

const KICKER: Style = Style {
    size: 26.0,
    weight: 800.0,
    spacing: 2.5,
    color: CORAL,
};

const INVITE: Style = Style {
    size: 28.0,
    weight: 700.0,
    spacing: 0.0,
    color: ACTION_BLUE,
};

impl Style {
    fn name(size: f32) -> Self {
        Self {
            size,
            weight: 800.0,
            spacing: 0.0,
            color: NAVY,
        }
    }

    /// How wide `text` sets in this style. Kerning only ever narrows Nunito,
    /// so leaving it out errs toward wrapping early, never toward running
    /// off the card.
    fn width(&self, face: &mut ttf_parser::Face, text: &str) -> f32 {
        face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), self.weight);

        let scale = self.size / f32::from(face.units_per_em());

        text.chars()
            .map(|c| {
                let advance = face
                    .glyph_index(c)
                    .and_then(|glyph| face.glyph_hor_advance(glyph))
                    .map_or(self.size, |advance| f32::from(advance) * scale);

                advance + self.spacing
            })
            .sum()
    }

    /// `text` broken into lines no wider than `width`, at spaces. A single
    /// word wider than that has a line to itself.
    fn wrap(&self, face: &mut ttf_parser::Face, text: &str, width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();

        for word in text.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };

            if line.is_empty() || self.width(face, &candidate) <= width {
                line = candidate;
            } else {
                lines.push(std::mem::replace(&mut line, word.to_owned()));
            }
        }

        if !line.is_empty() {
            lines.push(line);
        }

        lines
    }

    fn text_element(&self, y: f32, text: &str) -> String {
        format!(
            r#"<text x="{TEXT_X}" y="{y}" font-family="Nunito" font-weight="{}" font-size="{}" letter-spacing="{}" fill="{}">{}</text>"#,
            self.weight,
            self.size,
            self.spacing,
            self.color,
            escape(text)
        )
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The card for `text`, drawn around `artwork`, the badge's PNG.
pub fn render(artwork: &[u8], text: &CardText) -> Result<Vec<u8>, Error> {
    let svg = svg(artwork, text)?;

    let options = usvg::Options {
        fontdb: FONTS.clone(),
        ..Default::default()
    };

    let tree = usvg::Tree::from_str(&svg, &options)
        .map_err(|e| anyhow!("Badge card did not parse: {e}"))?;

    let mut pixmap =
        tiny_skia::Pixmap::new(WIDTH, HEIGHT).ok_or_else(|| anyhow!("Badge card has no size"))?;

    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    pixmap
        .encode_png()
        .map_err(|e| anyhow!("Badge card did not encode: {e}"))
}

/// The card as SVG: the artwork on a pale panel, the text beside it, and
/// the logo beneath the text.
fn svg(artwork: &[u8], text: &CardText) -> Result<String, Error> {
    let mut face =
        ttf_parser::Face::parse(NUNITO, 0).map_err(|e| anyhow!("Nunito did not parse: {e}"))?;

    let mut name = Style::name(60.0);
    let mut name_lines = name.wrap(&mut face, &text.name, TEXT_WIDTH);

    while name_lines.len() > NAME_MAX_LINES && name.size > NAME_MIN_SIZE {
        name = Style::name(name.size - 4.0);
        name_lines = name.wrap(&mut face, &text.name, TEXT_WIDTH);
    }

    let invite_lines = text
        .invite
        .as_deref()
        .map(|invite| INVITE.wrap(&mut face, invite, TEXT_WIDTH))
        .unwrap_or_default();

    let name_leading = name.size * 1.12;
    let invite_leading = INVITE.size * 1.3;
    let kicker_gap = 24.0;
    let invite_gap = 28.0;

    let height = KICKER.size
        + kicker_gap
        + name_leading * name_lines.len() as f32
        + if invite_lines.is_empty() {
            0.0
        } else {
            invite_gap + invite_leading * invite_lines.len() as f32
        };

    let mut y = ((TEXT_BOTTOM - height) / 2.0).max(40.0) + KICKER.size;
    let mut elements = KICKER.text_element(y, &text.kicker.to_uppercase());

    y += kicker_gap;

    for line in &name_lines {
        y += name_leading;
        elements.push_str(&name.text_element(y, line));
    }

    if !invite_lines.is_empty() {
        y += invite_gap;

        for line in &invite_lines {
            y += invite_leading;
            elements.push_str(&INVITE.text_element(y, line));
        }
    }

    let artwork = BASE64.encode(artwork);
    let logo = BASE64.encode(LOGO);

    Ok(format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">
<rect width="{WIDTH}" height="{HEIGHT}" fill="#FFFFFF"/>
<rect width="{PANEL_WIDTH}" height="{HEIGHT}" fill="{LIGHT_BLUE}"/>
<image x="60" y="115" width="400" height="400" xlink:href="data:image/png;base64,{artwork}"/>
{elements}
<image x="{TEXT_X}" y="{LOGO_Y}" width="191" height="55" xlink:href="data:image/svg+xml;base64,{logo}"/>
</svg>"##
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::badge::Badge;

    /// Every character the card is asked to draw, in every language, has a
    /// glyph in the font: a missing one would be drawn as nothing at all.
    #[test]
    fn the_font_covers_the_translated_text() {
        let face = ttf_parser::Face::parse(NUNITO, 0).unwrap();

        for text in [
            "Certificação conquistada",
            "Ética de Datos",
            "Obtén la tuya",
            "ñ ç ã õ ú í",
        ] {
            for c in text.chars().filter(|c| !c.is_whitespace()) {
                assert!(face.glyph_index(c).is_some(), "Nunito has no {c:?}");
            }
        }
    }

    /// A long name wraps within the text column rather than running off the
    /// card.
    #[test]
    fn a_long_name_wraps_within_the_column() {
        let mut face = ttf_parser::Face::parse(NUNITO, 0).unwrap();
        let name = Style::name(60.0);
        let lines = name.wrap(
            &mut face,
            "Building Data Literacy Through Community and Citizen Science",
            TEXT_WIDTH,
        );

        assert!(lines.len() > 1);

        for line in &lines {
            assert!(
                name.width(&mut face, line) <= TEXT_WIDTH,
                "{line:?} overruns"
            );
        }
    }

    #[test]
    fn a_card_renders_at_the_size_previews_want() {
        let artwork = crate::Static::get(Badge::FoundationsOfCitizenScience.template_path())
            .unwrap()
            .data;
        let png = render(
            &artwork,
            &CardText {
                kicker: String::from("Certification earned"),
                name: String::from("Foundations of Citizen Science"),
                invite: Some(String::from(
                    "Earn yours at SciStarter.org/training-foundations",
                )),
            },
        )
        .unwrap();

        // The IHDR chunk's width and height, big-endian, right after the
        // PNG signature and the chunk's length and type.
        assert_eq!(&png[16..20], &WIDTH.to_be_bytes());
        assert_eq!(&png[20..24], &HEIGHT.to_be_bytes());
    }
}
