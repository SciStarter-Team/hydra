//! Admin management of training-page text content (Engine Room).
//!
//! Each training's structure and gating live in code; this page edits only the
//! copy -- titles, prose, Articulate URLs, the sidebar -- held in that
//! training's copy record ([`TrainingCopy`]). Only the fields staff change are
//! stored ([`crate::training::content`]); every other field follows the
//! training's built-in copy, so improvements to it reach the page with the
//! next deploy. The editor marks each changed field, shows the built-in text
//! beside it, and reverts a field, or the whole record, to the built-in copy.
//!
//! Every training's form is rendered and read through the same [`FieldSpec`]
//! descriptors, working on the copy's fields as JSON; the
//! `descriptors_cover_every_field_of_every_copy` test keeps each training's
//! descriptors and its copy struct in step.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Error;
use poem::{
    Endpoint, Route, get, handler,
    http::StatusCode,
    web::{Form, Path},
};
use sailfish::TemplateOnce;
use serde_json::{Map, Value};

use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::training::champion::ChampionContent;
use crate::training::content::{TrainingCopy, TrainingOverrides, copy_fields};
use crate::training::dataethics::{DataEthicsContent, DataEthicsLanguage};
use crate::training::dataliteracy::DataLiteracyContent;
use crate::training::foundations::FoundationsContent;
use crate::training::highered::HigherEdContent;
use crate::training::k12::K12Content;
use crate::training::libraries::LibrariesContent;
use common::message::user::BasicInfo;

// ==================== Field descriptors ====================

/// How a field is edited, and how its text maps onto the copy's value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Input {
    /// A single line of text.
    Line,
    /// Plain prose over a few lines.
    Prose { rows: u32 },
    /// HTML source.
    Html { rows: u32 },
    /// A list, edited one item per line; items may contain HTML.
    List { rows: u32 },
}

impl Input {
    /// The text a field's value is edited as.
    fn text(self, value: &Value) -> String {
        match (self, value) {
            (Input::List { .. }, Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n"),
            (_, Value::String(text)) => text.clone(),
            _ => String::new(),
        }
    }

    /// The value a field's submitted text stands for. A browser sends a
    /// textarea's line breaks as CRLF; they are stored as LF, as the
    /// built-in copy has them, so an untouched field still reads as built-in.
    fn value(self, text: &str) -> Value {
        let text = text.replace("\r\n", "\n");

        match self {
            Input::List { .. } => Value::Array(
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(|line| Value::String(line.to_string()))
                    .collect(),
            ),
            _ => Value::String(text),
        }
    }

    /// Rows of the textarea it is edited in; 0 for a single-line input.
    fn rows(self) -> u32 {
        match self {
            Input::Line => 0,
            Input::Prose { rows } | Input::Html { rows } | Input::List { rows } => rows,
        }
    }

    /// Whether it is edited in a monospaced font, as source.
    fn mono(self) -> bool {
        matches!(self, Input::Html { .. } | Input::List { .. })
    }
}

/// One editable field of a training's copy. `name` is the copy struct's field
/// name, which is also its key in the stored changes.
#[derive(PartialEq, Eq, Debug)]
struct FieldSpec {
    name: &'static str,
    label: &'static str,
    input: Input,
}

const fn field(name: &'static str, label: &'static str, input: Input) -> FieldSpec {
    FieldSpec { name, label, input }
}

/// A titled group of fields, as the page lays them out.
struct SectionSpec {
    heading: &'static str,
    fields: &'static [FieldSpec],
}

const HEADER: SectionSpec = SectionSpec {
    heading: "Page header",
    fields: &[
        field("title", "Title", Input::Line),
        field("sponsor", "Sponsor line (blank to hide)", Input::Line),
        field("intro", "Intro (HTML)", Input::Html { rows: 5 }),
        field("estimated_time", "Estimated time", Input::Line),
        field("badge_image", "Badge image URL", Input::Line),
    ],
};

const SKILLS: SectionSpec = SectionSpec {
    heading: "Skills & competencies",
    fields: &[
        field("skills_intro", "Intro sentence", Input::Prose { rows: 2 }),
        field(
            "skills",
            "Skills (one per line, HTML allowed)",
            Input::List { rows: 6 },
        ),
        field(
            "skills_footnote",
            "Footnote (HTML)",
            Input::Html { rows: 3 },
        ),
    ],
};

const ACCOUNT: SectionSpec = SectionSpec {
    heading: "Step 1 — SciStarter Account",
    fields: &[field("account_heading", "Step heading", Input::Line)],
};

const PREREQUISITE: SectionSpec = SectionSpec {
    heading: "Step 2 — Foundations prerequisite",
    fields: &[
        field("prereq_heading", "Step heading", Input::Line),
        field("prereq_html", "Body (HTML)", Input::Html { rows: 3 }),
    ],
};

/// The tutorial step of a training with one Articulate module.
const TUTORIAL_FIELDS: &[FieldSpec] = &[
    field("tutorial_heading", "Step heading", Input::Line),
    field("tutorial_intro", "Intro (HTML)", Input::Html { rows: 3 }),
    field("articulate_url", "Articulate URL", Input::Line),
    field("articulate_image", "Tutorial card image URL", Input::Line),
];

const TUTORIAL_SECOND: SectionSpec = SectionSpec {
    heading: "Step 2 — Tutorial & quiz",
    fields: TUTORIAL_FIELDS,
};

const TUTORIAL_THIRD: SectionSpec = SectionSpec {
    heading: "Step 3 — Tutorial & quiz",
    fields: TUTORIAL_FIELDS,
};

const SIDEBAR: SectionSpec = SectionSpec {
    heading: "Sidebar",
    fields: &[field(
        "sidebar_html",
        "Sidebar (HTML, badge rendered separately)",
        Input::Html { rows: 10 },
    )],
};

const FOUNDATIONS: &[&SectionSpec] = &[
    &HEADER,
    &SKILLS,
    &SectionSpec {
        heading: "Step 1 — SciStarter Account",
        fields: &[
            field("account_heading", "Step heading", Input::Line),
            field(
                "account_signed_out_html",
                "Signed out (HTML; {next} is the return path)",
                Input::Html { rows: 3 },
            ),
        ],
    },
    &SectionSpec {
        heading: "Step 2 — Tutorial & quiz",
        fields: &[
            field("tutorial_heading", "Step heading", Input::Line),
            field("tutorial_intro", "Intro (HTML)", Input::Html { rows: 3 }),
            field(
                "tutorial_done_html",
                "Finished, signed out (HTML)",
                Input::Html { rows: 2 },
            ),
            field("articulate_en", "Articulate URL (English)", Input::Line),
            field("articulate_es", "Articulate URL (Spanish)", Input::Line),
        ],
    },
    &SectionSpec {
        heading: "Step 3 — Your badge",
        fields: &[
            field("claim_heading", "Step heading", Input::Line),
            field(
                "claim_button",
                "Header button once finished, signed out",
                Input::Line,
            ),
            field(
                "claim_html",
                "Congratulations (HTML)",
                Input::Html { rows: 2 },
            ),
            field(
                "next_steps_html",
                "Below the badge links (HTML)",
                Input::Html { rows: 6 },
            ),
            field(
                "last_chance_html",
                "Finished, signed out: last chance (HTML)",
                Input::Html { rows: 10 },
            ),
        ],
    },
    &SIDEBAR,
];

const DATA_ETHICS: &[&SectionSpec] = &[
    &HEADER,
    &SKILLS,
    &ACCOUNT,
    &TUTORIAL_SECOND,
    &SectionSpec {
        heading: "Step 3 — Claim your badge",
        fields: &[
            field("claim_heading", "Step heading", Input::Line),
            field("claim_html", "Body (HTML)", Input::Html { rows: 4 }),
        ],
    },
    &SIDEBAR,
];

const DATA_LITERACY: &[&SectionSpec] = &[
    &HEADER,
    &SKILLS,
    &ACCOUNT,
    &SectionSpec {
        heading: "Step 2 — Tutorial & quiz",
        fields: &[
            field("tutorial_heading", "Step heading", Input::Line),
            field("tutorial_intro", "Intro (HTML)", Input::Html { rows: 3 }),
            field("articulate_en", "Articulate URL (English)", Input::Line),
            field(
                "articulate_en_image",
                "Card image URL (English)",
                Input::Line,
            ),
            field("articulate_es", "Articulate URL (Spanish)", Input::Line),
            field(
                "articulate_es_image",
                "Card image URL (Spanish)",
                Input::Line,
            ),
        ],
    },
    &SectionSpec {
        heading: "Step 3 — Claim your badge",
        fields: &[
            field("claim_heading", "Step heading", Input::Line),
            field("claim_html", "Body (HTML)", Input::Html { rows: 5 }),
        ],
    },
    &SIDEBAR,
];

const HIGHER_ED: &[&SectionSpec] = &[
    &HEADER,
    &SKILLS,
    &ACCOUNT,
    &TUTORIAL_SECOND,
    &SectionSpec {
        heading: "Step 3 — Create a list (tracked)",
        fields: &[
            field("list_heading", "Step heading", Input::Line),
            field("list_html", "Body (HTML)", Input::Html { rows: 4 }),
        ],
    },
    &SectionSpec {
        heading: "Step 4 — Explore AAPS (tracked, grants badge)",
        fields: &[
            field("explore_heading", "Step heading", Input::Line),
            field(
                "explore_html",
                "Body (HTML) — link to /training/higher-ed/explore",
                Input::Html { rows: 4 },
            ),
            field(
                "explore_url",
                "Explore destination URL (where the link redirects)",
                Input::Line,
            ),
        ],
    },
    &SIDEBAR,
];

const LIBRARIES: &[&SectionSpec] = &[
    &HEADER,
    &SKILLS,
    &ACCOUNT,
    &PREREQUISITE,
    &TUTORIAL_THIRD,
    &SectionSpec {
        heading: "Step 4 — Join the Library Network",
        fields: &[
            field("supplemental_heading", "Step heading", Input::Line),
            field("supplemental_html", "Body (HTML)", Input::Html { rows: 3 }),
        ],
    },
    &SectionSpec {
        heading: "Step 5 — Claim your badge",
        fields: &[
            field("claim_heading", "Step heading", Input::Line),
            field("claim_html", "Body (HTML)", Input::Html { rows: 10 }),
        ],
    },
    &SIDEBAR,
];

const CHAMPION: &[&SectionSpec] = &[
    &HEADER,
    &SKILLS,
    &ACCOUNT,
    &PREREQUISITE,
    &TUTORIAL_THIRD,
    &SectionSpec {
        heading: "Step 4 — Champion claim",
        fields: &[
            field("claim_heading", "Step heading", Input::Line),
            field("claim_html", "Body (HTML)", Input::Html { rows: 10 }),
        ],
    },
    &SectionSpec {
        heading: "Step 5 — Apply (Certified Ambassador)",
        fields: &[
            field("apply_heading", "Step heading", Input::Line),
            field("apply_html", "Body (HTML)", Input::Html { rows: 8 }),
        ],
    },
    &SectionSpec {
        heading: "Step 6 — Application received (shown after submitting)",
        fields: &[
            field("received_heading", "Step heading", Input::Line),
            field("received_html", "Body (HTML)", Input::Html { rows: 4 }),
        ],
    },
    &SIDEBAR,
];

const K12: &[&SectionSpec] = &[
    &HEADER,
    &SKILLS,
    &ACCOUNT,
    &PREREQUISITE,
    &TUTORIAL_THIRD,
    &SectionSpec {
        heading: "Step 4 — Congratulations",
        fields: &[
            field("claim_heading", "Step heading", Input::Line),
            field("claim_html", "Body (HTML)", Input::Html { rows: 5 }),
        ],
    },
    &SIDEBAR,
];

// ==================== Trainings ====================

/// One editable copy record: a training page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Training {
    Foundations,
    DataEthics(DataEthicsLanguage),
    DataLiteracy,
    HigherEd,
    Libraries,
    Champion,
    K12,
}

impl Training {
    const ALL: [Training; 9] = [
        Training::Foundations,
        Training::DataEthics(DataEthicsLanguage::English),
        Training::DataEthics(DataEthicsLanguage::Spanish),
        Training::DataEthics(DataEthicsLanguage::Portuguese),
        Training::DataLiteracy,
        Training::HigherEd,
        Training::Libraries,
        Training::Champion,
        Training::K12,
    ];

    /// The last path segment of the record's editor, and the target logged
    /// with each edit.
    fn slug(self) -> &'static str {
        match self {
            Training::Foundations => "foundations",
            Training::DataEthics(language) => language.content_slug(),
            Training::DataLiteracy => "data-literacy",
            Training::HigherEd => "higher-ed",
            Training::Libraries => "libraries-hubs",
            Training::Champion => "champion",
            Training::K12 => "k12",
        }
    }

    fn from_slug(slug: &str) -> Option<Training> {
        Training::ALL
            .into_iter()
            .find(|training| training.slug() == slug)
    }

    /// The page's name, as the listing and the breadcrumb show it.
    fn name(self) -> &'static str {
        match self {
            Training::Foundations => "Foundations of Citizen Science",
            Training::DataEthics(DataEthicsLanguage::English) => "Data Ethics for Practitioners",
            Training::DataEthics(DataEthicsLanguage::Spanish) => {
                "Data Ethics for Practitioners (Spanish)"
            }
            Training::DataEthics(DataEthicsLanguage::Portuguese) => {
                "Data Ethics for Practitioners (Portuguese)"
            }
            Training::DataLiteracy => "Data Literacy",
            Training::HigherEd => "Teaching in Higher Education",
            Training::Libraries => "Libraries as Community Hubs",
            Training::Champion => "Citizen Science Champion",
            Training::K12 => "Citizen Science for K-12 Educators",
        }
    }

    /// The editor's page title.
    fn title(self) -> &'static str {
        match self {
            Training::Foundations => "Foundations Training",
            Training::DataEthics(DataEthicsLanguage::English) => "Data Ethics Training",
            Training::DataEthics(DataEthicsLanguage::Spanish) => "Data Ethics Training (Spanish)",
            Training::DataEthics(DataEthicsLanguage::Portuguese) => {
                "Data Ethics Training (Portuguese)"
            }
            Training::DataLiteracy => "Data Literacy Training",
            Training::HigherEd => "Higher Education Training",
            Training::Libraries => "Libraries & Hubs Training",
            Training::Champion => "Champion Training",
            Training::K12 => "K-12 Educators Training",
        }
    }

    fn sections(self) -> &'static [&'static SectionSpec] {
        match self {
            Training::Foundations => FOUNDATIONS,
            Training::DataEthics(_) => DATA_ETHICS,
            Training::DataLiteracy => DATA_LITERACY,
            Training::HigherEd => HIGHER_ED,
            Training::Libraries => LIBRARIES,
            Training::Champion => CHAMPION,
            Training::K12 => K12,
        }
    }

    fn fields(self) -> impl Iterator<Item = &'static FieldSpec> {
        self.sections()
            .iter()
            .flat_map(|section| section.fields.iter())
    }

    /// The `training_content` row holding the record's changes.
    fn row(self) -> &'static str {
        match self {
            Training::Foundations => FoundationsContent::row(()),
            Training::DataEthics(language) => DataEthicsContent::row(language),
            Training::DataLiteracy => DataLiteracyContent::row(()),
            Training::HigherEd => HigherEdContent::row(()),
            Training::Libraries => LibrariesContent::row(()),
            Training::Champion => ChampionContent::row(()),
            Training::K12 => K12Content::row(()),
        }
    }

    /// The record's built-in copy, field by field.
    fn built_in(self) -> Result<Map<String, Value>, Error> {
        match self {
            Training::Foundations => copy_fields(&FoundationsContent::built_in(())),
            Training::DataEthics(language) => copy_fields(&DataEthicsContent::built_in(language)),
            Training::DataLiteracy => copy_fields(&DataLiteracyContent::built_in(())),
            Training::HigherEd => copy_fields(&HigherEdContent::built_in(())),
            Training::Libraries => copy_fields(&LibrariesContent::built_in(())),
            Training::Champion => copy_fields(&ChampionContent::built_in(())),
            Training::K12 => copy_fields(&K12Content::built_in(())),
        }
    }

    /// The record's built-in copy and its current copy, field by field.
    async fn copy(self) -> Result<(Map<String, Value>, Map<String, Value>), Error> {
        let built_in = self.built_in()?;
        let overrides = TrainingOverrides::load(self.row().to_string()).await?;
        let current = overrides.apply(built_in.clone());
        Ok((built_in, current))
    }
}

/// How many of a record's fields read differently from its built-in copy.
fn changed_count(built_in: &Map<String, Value>, current: &Map<String, Value>) -> usize {
    TrainingOverrides::between("", built_in, current)
        .fields
        .len()
}

// ==================== Listing ====================

struct TrainingRow {
    name: &'static str,
    slug: &'static str,
    changed: usize,
}

#[derive(TemplateOnce)]
#[template(path = "admin/trainings.stpl.html")]
struct TrainingsPage {
    trainings: Vec<TrainingRow>,
}

#[handler]
async fn trainings_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let mut trainings = Vec::with_capacity(Training::ALL.len());

    for training in Training::ALL {
        let (built_in, current) = training.copy().await?;

        trainings.push(TrainingRow {
            name: training.name(),
            slug: training.slug(),
            changed: changed_count(&built_in, &current),
        });
    }

    let prepared = AdminLayout::new("Trainings", TrainingsPage { trainings })
        .with_sidebar_active("engine-room-trainings")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Trainings", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

// ==================== Editing ====================

/// A field as the editor shows it.
struct FieldView {
    name: &'static str,
    label: &'static str,
    value: String,
    built_in: String,
    rows: u32,
    mono: bool,
    changed: bool,
}

struct SectionView {
    heading: &'static str,
    fields: Vec<FieldView>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/training_edit.stpl.html")]
struct TrainingEditPage {
    action: String,
    notice: Option<String>,
    changed: usize,
    sections: Vec<SectionView>,
}

fn sections_view(
    training: Training,
    built_in: &Map<String, Value>,
    current: &Map<String, Value>,
) -> Vec<SectionView> {
    let text = |fields: &Map<String, Value>, spec: &FieldSpec| {
        fields
            .get(spec.name)
            .map(|value| spec.input.text(value))
            .unwrap_or_default()
    };

    training
        .sections()
        .iter()
        .map(|section| SectionView {
            heading: section.heading,
            fields: section
                .fields
                .iter()
                .map(|spec| FieldView {
                    name: spec.name,
                    label: spec.label,
                    value: text(current, spec),
                    built_in: text(built_in, spec),
                    rows: spec.input.rows(),
                    mono: spec.input.mono(),
                    changed: current.get(spec.name) != built_in.get(spec.name),
                })
                .collect(),
        })
        .collect()
}

async fn render_edit(
    user_info: Option<Arc<BasicInfo>>,
    training: Training,
    built_in: &Map<String, Value>,
    current: &Map<String, Value>,
    notice: Option<String>,
) -> Result<poem::Response, anyhow::Error> {
    let page = TrainingEditPage {
        action: format!("/-admin/trainings/{}", training.slug()),
        notice,
        changed: changed_count(built_in, current),
        sections: sections_view(training, built_in, current),
    };
    let prepared = AdminLayout::new(training.title(), page)
        .with_sidebar_active("engine-room-trainings")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Trainings", Some("/-admin/trainings"))
        .with_breadcrumb(training.name(), None)
        .prepare(user_info)
        .await?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

fn not_found() -> poem::Response {
    poem::Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body("No such training")
}

#[handler]
async fn training_edit(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let Some(training) = Training::from_slug(&slug) else {
        return Ok(not_found());
    };

    let (built_in, current) = training.copy().await?;
    render_edit(user_info, training, &built_in, &current, None).await
}

/// What a save puts back to the built-in copy, besides applying the form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Revert {
    Field(&'static FieldSpec),
    All,
}

impl Revert {
    /// The revert a submitted form asks for: the value of the button pressed,
    /// a field name or `*` for every field. A name the training does not
    /// have -- from an editor opened before a field was renamed -- asks for
    /// nothing, and the rest of the form is saved as usual.
    fn requested(training: Training, form: &HashMap<String, String>) -> Option<Revert> {
        match form.get("revert").map(String::as_str) {
            None => None,
            Some("*") => Some(Revert::All),
            Some(name) => training
                .fields()
                .find(|spec| spec.name == name)
                .map(Revert::Field),
        }
    }

    fn notice(self) -> String {
        match self {
            Revert::Field(spec) => format!(
                "Saved, with “{}” put back to the built-in copy. Changes are live across all frontends.",
                spec.label
            ),
            Revert::All => {
                "Every field is back to the built-in copy. Changes are live across all frontends."
                    .to_string()
            }
        }
    }
}

/// A record's fields once a submitted form is applied to `current`: each
/// field the form carries takes the submitted text, and then `revert` puts
/// one field, or every field, back to the built-in copy. Reverting one field
/// keeps the form's other edits; reverting all of them discards them too.
fn apply_form(
    training: Training,
    mut current: Map<String, Value>,
    built_in: &Map<String, Value>,
    form: &HashMap<String, String>,
    revert: Option<Revert>,
) -> Map<String, Value> {
    if revert == Some(Revert::All) {
        return built_in.clone();
    }

    for spec in training.fields() {
        if let Some(text) = form.get(spec.name) {
            current.insert(spec.name.to_string(), spec.input.value(text));
        }
    }

    if let Some(Revert::Field(spec)) = revert {
        match built_in.get(spec.name) {
            Some(value) => current.insert(spec.name.to_string(), value.clone()),
            None => current.remove(spec.name),
        };
    }

    current
}

#[handler]
async fn training_save(
    Path(slug): Path<String>,
    Form(form): Form<HashMap<String, String>>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let Some(training) = Training::from_slug(&slug) else {
        return Ok(not_found());
    };

    let (built_in, current) = training.copy().await?;
    let revert = Revert::requested(training, &form);
    let edited = apply_form(training, current, &built_in, &form, revert);

    TrainingOverrides::between(training.row(), &built_in, &edited)
        .store()
        .await?;
    log_edit(&user_info, training.slug()).await;

    let notice = match revert {
        Some(revert) => revert.notice(),
        None => "Saved. Changes are live across all frontends.".to_string(),
    };

    render_edit(user_info, training, &built_in, &edited, Some(notice)).await
}

async fn log_edit(user_info: &Option<Arc<BasicInfo>>, slug: &str) {
    if let Some(user) = user_info {
        let _ = common::service::log_activity(
            None,
            "training-content-edit",
            user.user_id,
            None,
            Some(slug.to_string()),
        )
        .await;
    }
}

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(trainings_list))
        .at("/:slug", get(training_edit).post(training_save))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The editor reads and writes the copy by field name, so a descriptor
    /// naming a field the copy lacks would edit nothing, and a copy field
    /// with no descriptor could never be edited. Each descriptor also has to
    /// edit its field as the kind of value the copy holds there.
    #[test]
    fn descriptors_cover_every_field_of_every_copy() {
        for training in Training::ALL {
            let built_in = training.built_in().unwrap();
            let names: Vec<&str> = training.fields().map(|spec| spec.name).collect();

            let mut sorted = names.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(
                sorted.len(),
                names.len(),
                "{training:?} edits a field twice"
            );

            for name in built_in.keys() {
                assert!(
                    names.contains(&name.as_str()),
                    "{training:?} has no descriptor for {name}"
                );
            }

            for spec in training.fields() {
                let value = built_in
                    .get(spec.name)
                    .unwrap_or_else(|| panic!("{training:?} has no field {}", spec.name));
                let is_list = matches!(spec.input, Input::List { .. });
                assert_eq!(
                    value.is_array(),
                    is_list,
                    "{training:?} edits {} as the wrong kind of value",
                    spec.name
                );
            }
        }
    }

    /// Every record has its own editor and its own row.
    #[test]
    fn every_training_has_its_own_slug_and_row() {
        for (i, a) in Training::ALL.iter().enumerate() {
            assert_eq!(Training::from_slug(a.slug()), Some(*a));
            for b in &Training::ALL[i + 1..] {
                assert_ne!(a.slug(), b.slug());
                assert_ne!(a.row(), b.row());
            }
        }
    }

    /// A form submitted untouched -- line endings as the browser sends them
    /// -- records no change, so the record goes on following the code.
    #[test]
    fn an_untouched_form_changes_nothing() {
        let training = Training::Foundations;
        let built_in = training.built_in().unwrap();

        let form: HashMap<String, String> = training
            .fields()
            .map(|spec| {
                let text = spec.input.text(&built_in[spec.name]);
                (spec.name.to_string(), text.replace('\n', "\r\n"))
            })
            .collect();

        let edited = apply_form(training, built_in.clone(), &built_in, &form, None);

        assert_eq!(changed_count(&built_in, &edited), 0);
    }

    /// An edited field is the only change; reverting it keeps the form's
    /// other edits, and reverting everything returns the built-in copy.
    #[test]
    fn reverting_puts_fields_back_to_the_built_in_copy() {
        let training = Training::K12;
        let built_in = training.built_in().unwrap();
        let form = HashMap::from([
            ("title".to_string(), "Edited title".to_string()),
            ("claim_heading".to_string(), "Edited heading".to_string()),
        ]);

        let edited = apply_form(training, built_in.clone(), &built_in, &form, None);
        assert_eq!(edited["title"], json!("Edited title"));
        assert_eq!(changed_count(&built_in, &edited), 2);

        let title = training.fields().find(|spec| spec.name == "title").unwrap();
        let reverted = apply_form(
            training,
            edited.clone(),
            &built_in,
            &form,
            Some(Revert::Field(title)),
        );
        assert_eq!(reverted["title"], built_in["title"]);
        assert_eq!(reverted["claim_heading"], json!("Edited heading"));

        let all = apply_form(training, edited, &built_in, &form, Some(Revert::All));
        assert_eq!(all, built_in);
    }

    /// The button pressed names what to revert; anything else asks for
    /// nothing.
    #[test]
    fn the_revert_requested_is_the_button_pressed() {
        let training = Training::Foundations;
        let ask = |value: &str| {
            Revert::requested(
                training,
                &HashMap::from([("revert".to_string(), value.to_string())]),
            )
        };

        assert_eq!(ask("*"), Some(Revert::All));
        assert!(
            matches!(ask("articulate_en"), Some(Revert::Field(spec)) if spec.name == "articulate_en")
        );
        assert_eq!(ask("no_such_field"), None);
        assert_eq!(Revert::requested(training, &HashMap::new()), None);
    }

    /// Skills are edited one per line; blank lines and stray indentation do
    /// not become items.
    #[test]
    fn a_list_is_edited_one_item_per_line() {
        let list = Input::List { rows: 6 };

        assert_eq!(
            list.value("  first \r\n\r\nsecond\n"),
            json!(["first", "second"])
        );
        assert_eq!(list.text(&json!(["first", "second"])), "first\nsecond");
    }
}
