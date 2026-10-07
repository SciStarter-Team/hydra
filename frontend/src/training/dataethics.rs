//! Data Ethics for Practitioners training — a clean three-step training
//! (Account → Tutorial → Claim) on the shared [`crate::training`] framework.
//! Ported from orrery's `/training-dataethics`, now at `/training/data-ethics`.
//! State comes from the user's `data_ethics_progress` timestamps.
//!
//! Three languages, three pages. The tutorial has Spanish and Portuguese
//! builds of its own, each reporting its own xAPI activity id, so which
//! language a learner worked in is known only at the moment they finish — and
//! it decides which badge-earned email they are sent. Rather than translating
//! one page at render time, each language is a page at its own URL, mirroring
//! orrery's `training-dataethics{,-es,-pt}`, whose links are in circulation
//! and are redirected here.

use std::sync::Arc;

use anyhow::Error;
use common::message::user::{BasicInfo, WorkflowStatusResponse, WorkflowType};
use poem::{Response, handler};
use sailfish::TemplateOnce;

use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::training::{
    self, ArticulateCard, ClaimBody, SidebarBody, SigninBody, StepView, StepsView, TrainingChrome,
    TrainingContent, TutorialBody, linear_states_ungating, module_url, open_flags, render_page,
    render_progress_fragment, tutorial_launch_url,
};

/// Which build of the tutorial a page offers, and everything that follows from
/// that: where the page lives, which content record it reads, which module it
/// launches, and which completion step it reports.
///
/// One badge for all three — only the page, the module and the email differ.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DataEthicsLanguage {
    #[default]
    English,
    Spanish,
    Portuguese,
}

impl DataEthicsLanguage {
    /// Key of this language's row in `training_content`, and the last path
    /// segment of its admin editor.
    pub fn content_slug(self) -> &'static str {
        match self {
            DataEthicsLanguage::English => "data-ethics",
            DataEthicsLanguage::Spanish => "data-ethics-es",
            DataEthicsLanguage::Portuguese => "data-ethics-pt",
        }
    }

    pub fn base_path(self) -> &'static str {
        match self {
            DataEthicsLanguage::English => "/training/data-ethics",
            DataEthicsLanguage::Spanish => "/training/data-ethics-es",
            DataEthicsLanguage::Portuguese => "/training/data-ethics-pt",
        }
    }

    pub fn progress_path(self) -> &'static str {
        match self {
            DataEthicsLanguage::English => "/training/data-ethics/-progress",
            DataEthicsLanguage::Spanish => "/training/data-ethics-es/-progress",
            DataEthicsLanguage::Portuguese => "/training/data-ethics-pt/-progress",
        }
    }

    /// The completion step reported to the user service. Spelled as orrery
    /// spells the same three signals, so an account's progress reads alike
    /// whichever side recorded it.
    pub fn step(self) -> &'static str {
        match self {
            DataEthicsLanguage::English => "tutorial",
            DataEthicsLanguage::Spanish => "tutorial-es",
            DataEthicsLanguage::Portuguese => "tutorial-pt",
        }
    }

    /// Where the module is launched. All three are SCORM Driver packages,
    /// whose entry point is `scormdriver/indexAPI.html` — `index.html` 404s.
    /// English moved to one in Sep 2026; `dataethics/` still holds the Rise
    /// export it replaced.
    fn module_url(self) -> &'static str {
        match self {
            DataEthicsLanguage::English => {
                "https://media.scistarter.org/dataethics-v2/scormdriver/indexAPI.html"
            }
            DataEthicsLanguage::Spanish => {
                "https://media.scistarter.org/dataethics-es/scormdriver/indexAPI.html"
            }
            DataEthicsLanguage::Portuguese => {
                "https://media.scistarter.org/dataethics-pt/scormdriver/indexAPI.html"
            }
        }
    }

    /// The activity id to launch with, spelled as the module's own `tincan.xml`
    /// spells it.
    ///
    /// All three builds are now SCORM Driver packages, which report their
    /// assessment result only when given the full xAPI launch context, so
    /// every one of them needs an id -- hence a plain `&str` rather than an
    /// `Option`. English kept its 2022 activity id across the rebuild, so the
    /// arm in `api/v2.rs` did not have to change with it.
    fn activity_id(self) -> &'static str {
        match self {
            DataEthicsLanguage::English => "http://DataEthicsxAPISept72022",
            DataEthicsLanguage::Spanish => "http://4xI0y6kwB69QeGbWDC8I-KkmgEYmLbuL_rise",
            DataEthicsLanguage::Portuguese => "http://K632u-OqBl5kbcd5JFNucaO1cr2sekOh_rise",
        }
    }

    fn chrome(self) -> TrainingChrome {
        match self {
            DataEthicsLanguage::English => TrainingChrome::default(),
            DataEthicsLanguage::Spanish => TrainingChrome::spanish(),
            DataEthicsLanguage::Portuguese => TrainingChrome::portuguese(),
        }
    }
}

/// Editable copy for the Data Ethics training. See
/// [`crate::training::foundations::FoundationsContent`] for the pattern;
/// `#[serde(default)]` is required so added fields fill from `Default` instead
/// of discarding saved edits.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DataEthicsContent {
    /// Which page this copy belongs to. Carried in the record rather than
    /// implied by where it was read from, because `save_to_database` is handed
    /// nothing but the value -- including on a peer applying somebody else's
    /// edit -- and has to know which row to write.
    pub language: DataEthicsLanguage,
    pub title: String,
    pub sponsor: String,
    pub intro: String,
    pub estimated_time: String,
    pub badge_image: String,
    pub skills_intro: String,
    pub skills: Vec<String>,
    pub skills_footnote: String,
    pub account_heading: String,
    pub tutorial_heading: String,
    pub tutorial_intro: String,
    pub articulate_url: String,
    pub articulate_image: String,
    pub claim_heading: String,
    pub claim_html: String,
    pub sidebar_html: String,
}

impl Default for DataEthicsContent {
    fn default() -> Self {
        DataEthicsContent {
            language: DataEthicsLanguage::English,
            title: "Data Ethics for Practitioners".to_string(),
            sponsor: String::new(),
            intro: r#"The "Data Ethics" tutorial and badge are designed for practitioners and leaders of participatory science projects, including citizen science and community science projects. The tutorial will help you “think like an ethicist” by identifying relevant ethical obligations, tensions, and issues that may arise in your project, and use them to make decisions. The tutorial is intended to help you understand the unique obligations for collecting, stewarding, and disseminating project data in ways that foster trust between practitioners, participants, and partners.
<br>To maximize your engagement with and implementation of this material, we encourage you to become a member of the <a href="https://participatorysciences.org/" target="_blank">Association for Advancing Participatory Sciences</a> for access to more experiences and expertise across this diverse community of practice. AAPS membership is not required to access this tutorial or receive the badge, but one goal of this resource is to support the CSA’s capacity to create and maintain a culture of ethical data practices in the participatory sciences."#.to_string(),
            estimated_time: "2 hours".to_string(),
            badge_image: "https://media.scistarter.org/curated/2023/Data_Ethics_with_border.png"
                .to_string(),
            skills_intro: "By the end of this tutorial, you will be able to:".to_string(),
            skills: vec![
                "<strong>Distinguish</strong> roles and ethical obligations of project leaders, participants, and partners in participatory science projects.".to_string(),
                "<strong>Identify</strong> how ethical tensions arise in participatory science projects.".to_string(),
                "<strong>Recognize</strong> the obligations associated with ethical principles in the context of participatory science projects.".to_string(),
                "<strong>Analyze</strong> ethical tensions related to data integrity, report-outs, recompense, and report-backs in participatory science projects.".to_string(),
                "<strong>Recognize</strong> how data governance structures can concentrate or diffuse decision-making power.".to_string(),
            ],
            skills_footnote: r#"You will practice or acquire the following skills, which are part of an Open Skills Taxonomy used by employers and schools: <a href="https://skills.lightcast.io/skills/KS122P86NZFH1GP38G15" target="_blank">data integrity</a>, <a href="https://skills.lightcast.io/skills/KS122P378DGNVX4NKQKN" target="_blank">data governance</a>."#.to_string(),
            account_heading: "SciStarter Account".to_string(),
            tutorial_heading: "Self-guided tutorial and quiz".to_string(),
            tutorial_intro: r#"<b>Complete this self-guided tutorial and quiz.</b> Score an 80% or higher to move onto the next steps towards claiming your badge!"#.to_string(),
            articulate_url: DataEthicsLanguage::English.module_url().to_string(),
            articulate_image: "https://orrery-media.s3.amazonaws.com/uploads/froala_editor/images/Data%20Ethics%20Website%20Image_a3PGAki.png".to_string(),
            claim_heading: "Claim your badge".to_string(),
            claim_html: r#"<h2>Congratulations!</h2>
<p>You'll find your badge on your <a href="/dashboard/profile">SciStarter profile</a>, and you'll receive an email with instructions for sharing the verified badge and metadata on LinkedIn.</p>"#.to_string(),
            sidebar_html: r#"<div>
    <p class="text-sm">This material is based upon input from over 100 individuals so that it reflects both areas of broad agreement and areas of divergent priorities and values. Resources were developed and field tested by a team with expertise in instructional design, education, libraries, inclusive practices, digital design, micro accreditation and, of course, citizen science to produce a series of tutorials, trainings, and accompanying modules to help people from all walks of life discover and engage in authentic science.</p>
    <hr class="my-8 w-[50px] text-primary-light" />
    <p class="text-sm">Learn more about <a class="text-sm" href="https://scistarter.org/go/badging-brochure-2025" target="_blank">SciStarter Badges</a>.</p>
    <a href="https://scistarter.org/go/badging-brochure-2025" rel="noopener noreferrer" target="_blank"><img class="border" src="https://media.scistarter.org/curated/2025/SciStarter_Badging_Program_Brochure_2025_-_C.jpg" ></a>
    <hr class="my-8 w-[50px] text-primary-light" />
    <p>Optional: download the <em>PDF version</em> of the <a href="https://scistarter.org/go/data-ethics-toolkit" target="_blank">Data Ethics for Practitioners Toolkit</a>.</p>
    <a href="https://scistarter.org/go/data-ethics-toolkit" rel="noopener noreferrer" target="_blank"><img class="border" src="https://orrery-media.s3.amazonaws.com/uploads/froala_editor/images/Data%20Ethics%20Toolkit%20thumbnail.png" ></a>
</div>"#.to_string(),
        }
    }
}

impl LoadStorePeered for DataEthicsContent {
    type DatabaseKey = DataEthicsLanguage;

    const GROUP: &str = "training_content";
    // One peering channel for all three languages: the value carries its own,
    // so a peer knows which row an arriving edit belongs to.
    const SPECIFIC: &str = "data-ethics";

    async fn save_to_database(&self) -> Result<(), Error> {
        training::save_content_json(self.language.content_slug(), self).await
    }

    async fn read_from_database(language: DataEthicsLanguage) -> Result<Self, Error> {
        training::read_content_json(language.content_slug()).await
    }
}

impl DataEthicsContent {
    /// Current editable content for a language, or its built-in copy if none
    /// has been saved yet.
    pub async fn current(language: DataEthicsLanguage) -> DataEthicsContent {
        DataEthicsContent::load(language)
            .await
            .unwrap_or_else(|_| DataEthicsContent::built_in(language))
    }

    /// The copy a language ships with: orrery's English page for
    /// [`DataEthicsLanguage::English`], and the wording of its translated
    /// pages for the other two.
    pub fn built_in(language: DataEthicsLanguage) -> DataEthicsContent {
        match language {
            DataEthicsLanguage::English => DataEthicsContent::default(),
            DataEthicsLanguage::Spanish => DataEthicsContent::spanish(),
            DataEthicsLanguage::Portuguese => DataEthicsContent::portuguese(),
        }
    }
}

impl DataEthicsContent {
    /// The copy of orrery's `training-dataethics-es` page (entity 13061),
    /// published July 2026. The translation was funded in part by the Gordon
    /// and Betty Moore Foundation and produced with WCS, Alianza Aguas
    /// Amazónicas, RBCC and RICAP; it is their wording, not ours to reword.
    fn spanish() -> DataEthicsContent {
        DataEthicsContent {
            language: DataEthicsLanguage::Spanish,
            title: "Ética de datos para las ciencias participativas".to_string(),
            sponsor: String::new(),
            intro: r#"El tutorial y la insignia de “Ética de datos” están diseñados para participantes y líderes de proyectos de ciencia participativa, incluidos proyectos de ciencia ciudadana y ciencia comunitaria. El tutorial te ayudará a “pensar como una persona experta en ética”, identificando las obligaciones, tensiones y cuestiones relevantes en los aspectos éticos que pueden surgir en tu proyecto, y usándolas para orientar la toma de decisiones. El tutorial busca ayudarte a comprender las obligaciones específicas relacionadas con la recolección, gestión responsable y difusión de los datos del proyecto, de manera que fortalezcan la confianza entre profesionales, participantes y socios.
<br>Para ampliar tu participación y la implementación de este material, te invitamos a hacerte miembro de la <a href="https://participatorysciences.org/" target="_blank">Association for Advancing Participatory Sciences</a> u otras asociaciones regionales como la <a href="https://cienciaparticipativa.net/" target="_blank">Red Iberoamericana de Ciencia Participativa (RICAP)</a> o la <a href="https://www.rbcc.ong.br/" target="_blank">Rede Brasileira de Ciência Cidadã (RBCC)</a>, lo que te permitirá acceder a más experiencias y conocimientos dentro de esta diversa comunidad de práctica. No es necesario ser miembro de AAPS para acceder a este tutorial o recibir la insignia, pero uno de los objetivos de este recurso es apoyar la capacidad de la CSA para crear y mantener una cultura de prácticas éticas de datos en las ciencias participativas."#.to_string(),
            estimated_time: "2 horas".to_string(),
            badge_image: "https://media.scistarter.org/curated/2023/Data_Ethics_with_border.png"
                .to_string(),
            skills_intro: "Al finalizar este tutorial, podrás:".to_string(),
            skills: vec![
                "<strong>Distinguir</strong> los roles y las obligaciones éticas de líderes de proyecto, participantes y socios en proyectos de ciencia participativa.".to_string(),
                "<strong>Identificar</strong> cómo surgen las tensiones éticas en proyectos de ciencia participativa.".to_string(),
                "<strong>Reconocer</strong> las obligaciones asociadas con principios éticos en el contexto de proyectos de ciencia participativa.".to_string(),
                "<strong>Analizar</strong> tensiones éticas relacionadas con la integridad de los datos, la comunicación pública de resultados, la compensación y la devolución de resultados en proyectos de ciencia participativa.".to_string(),
                "<strong>Reconocer</strong> cómo las estructuras de gobernanza de datos pueden concentrar o distribuir el poder de decisión.".to_string(),
            ],
            skills_footnote: r#"Practicarás o adquirirás las siguientes habilidades, que forman parte de una Taxonomía Abierta de Competencias utilizada por empleadores e instituciones educativas: <a href="https://skills.lightcast.io/skills/KS122P86NZFH1GP38G15" target="_blank">integridad de datos</a> y <a href="https://skills.lightcast.io/skills/KS122P378DGNVX4NKQKN" target="_blank">gobernanza de datos</a>."#.to_string(),
            account_heading: "Cuenta de SciStarter".to_string(),
            tutorial_heading: "Tutorial autoguiado y cuestionario".to_string(),
            tutorial_intro: r#"<b>Completa este tutorial autoguiado y el cuestionario.</b> Obtén una puntuación del 80% o superior para avanzar a los siguientes pasos y solicitar tu insignia."#.to_string(),
            articulate_url: DataEthicsLanguage::Spanish.module_url().to_string(),
            articulate_image: "https://orrery-media.s3.amazonaws.com/attr/2026-08/Data_Ethics_Website_Image_SVG-2.png".to_string(),
            claim_heading: "Solicita tu insignia".to_string(),
            claim_html: r#"<h2>¡Felicitaciones!</h2>
<p>Encontrarás tu insignia en tu <a href="/dashboard/profile">perfil de SciStarter</a> y recibirás un correo electrónico con instrucciones para compartir la insignia verificada y sus metadatos en LinkedIn.</p>"#.to_string(),
            sidebar_html: r#"<div>
    <h3>Agradecimientos</h3>
    <p class="text-sm">La traducción al portugués de Brasil y al español de la Caja de Herramientas de Ética de Datos para las Ciencias Participativas es el resultado de una colaboración con Wildlife Conservation Society (WCS), la Alianza Aguas Amazónicas, la Rede Brasileira de Ciência Cidadã (RBCC) y la Red Iberoamericana de Ciencia Participativa (RICAP). Esta traducción fue financiada parcialmente por la Fundación Gordon y Betty Moore.</p>
    <hr class="my-8 w-[50px] text-primary-light" />
    <p class="text-sm">Este material se basa en los aportes de más de 100 personas, de modo que refleja tanto áreas de amplio acuerdo como áreas con prioridades y valores divergentes. Los recursos fueron desarrollados y probados en campo por un equipo con experiencia en diseño instruccional, educación, bibliotecas, prácticas inclusivas, diseño digital, microacreditación y, por supuesto, ciencia ciudadana, con el fin de producir una serie de tutoriales, formaciones y módulos complementarios que ayuden a personas de distintos contextos a descubrir y participar en actividades científicas.</p>
    <hr class="my-8 w-[50px] text-primary-light" />
    <p class="text-sm">Conoce más sobre las <a class="text-sm" href="https://scistarter.org/go/badging-brochure-2025" target="_blank">insignias de SciStarter</a>.</p>
    <a href="https://scistarter.org/go/badging-brochure-2025" rel="noopener noreferrer" target="_blank"><img class="border" src="https://media.scistarter.org/curated/2025/SciStarter_Badging_Program_Brochure_2025_-_C.jpg" ></a>
    <hr class="my-8 w-[50px] text-primary-light" />
    <p>Opcional: descarga la <em>versión en PDF</em> del <a href="https://scistarter.org/go/data-ethics-toolkit" target="_blank">kit de herramientas Ética de datos para profesionales</a>.</p>
    <a href="https://scistarter.org/go/data-ethics-toolkit" rel="noopener noreferrer" target="_blank"><img class="border" src="https://orrery-media.s3.amazonaws.com/uploads/froala_editor/images/Data%20Ethics%20Toolkit%20thumbnail.png" ></a>
</div>"#.to_string(),
        }
    }

    /// The copy of orrery's `training-dataethics-pt` page (entity 13062).
    fn portuguese() -> DataEthicsContent {
        DataEthicsContent {
            language: DataEthicsLanguage::Portuguese,
            title: "Ética de dados para ciência cidadã".to_string(),
            sponsor: String::new(),
            intro: r#"O tutorial e o selo de “Ética de Dados” foram desenvolvidos para profissionais e lideranças de projetos de ciência participativa, incluindo projetos de ciência cidadã e ciência comunitária. O tutorial ajudará você a “pensar como uma pessoa especialista em ética”, identificando obrigações éticas, tensões e questões relevantes que podem surgir em seu projeto, e usando-as para orientar a tomada de decisões. O tutorial busca ajudar você a compreender as obrigações específicas relacionadas à coleta, gestão responsável e disseminação dos dados do projeto, de forma a fortalecer a confiança entre profissionais, participantes e parceiros.
<br>Para ampliar sua participação e a implementação deste material, convidamos você a se tornar membro da <a href="https://participatorysciences.org/" target="_blank">Association for Advancing Participatory Sciences</a> ou de outras associações regionais, como a <a href="https://cienciaparticipativa.net/" target="_blank">Red Iberoamericana de Ciencia Participativa (RICAP)</a> ou a <a href="https://www.rbcc.ong.br/" target="_blank">Rede Brasileira de Ciência Cidadã (RBCC)</a>, o que permitirá acesso a mais experiências e conhecimentos dentro desta diversa comunidade de prática. Não é necessário ser membro da AAPS para acessar este tutorial ou receber o selo, mas um dos objetivos deste recurso é apoiar a capacidade da CSA de criar e manter uma cultura de práticas éticas de dados na ciência cidadã."#.to_string(),
            estimated_time: "2 horas".to_string(),
            badge_image: "https://media.scistarter.org/curated/2023/Data_Ethics_with_border.png"
                .to_string(),
            skills_intro: "Ao final deste tutorial, você será capaz de:".to_string(),
            skills: vec![
                "<strong>Distinguir</strong> os papéis e as obrigações éticas de lideranças de projeto, participantes e parceiros em projetos de ciência cidadã.".to_string(),
                "<strong>Identificar</strong> como surgem tensões éticas em projetos de ciência cidadã.".to_string(),
                "<strong>Reconhecer</strong> as obrigações associadas a princípios éticos no contexto de projetos de ciência cidadã.".to_string(),
                "<strong>Analisar</strong> tensões éticas relacionadas à integridade dos dados, à divulgação de resultados, à compensação e à devolutiva de resultados em projetos de ciência cidadã.".to_string(),
                "<strong>Reconhecer</strong> como as estruturas de governança de dados podem concentrar ou distribuir o poder de decisão.".to_string(),
            ],
            skills_footnote: r#"Você praticará ou adquirirá as seguintes habilidades, que fazem parte de uma Taxonomia Aberta de Competências utilizada por empregadores e instituições de ensino: <a href="https://skills.lightcast.io/skills/KS122P86NZFH1GP38G15" target="_blank">integridade de dados</a>, e <a href="https://skills.lightcast.io/skills/KS122P378DGNVX4NKQKN" target="_blank">governança de dados</a>."#.to_string(),
            account_heading: "Conta SciStarter".to_string(),
            tutorial_heading: "Tutorial autoguiado e questionário".to_string(),
            tutorial_intro: r#"<b>Conclua este tutorial autoguiado e o questionário.</b> Obtenha uma pontuação de 80% ou mais para avançar para as próximas etapas e solicitar o seu selo!"#.to_string(),
            articulate_url: DataEthicsLanguage::Portuguese.module_url().to_string(),
            articulate_image: "https://orrery-media.s3.amazonaws.com/attr/2026-08/Data_Ethics_Website_Image_SVG-2_25PyYGd.png".to_string(),
            claim_heading: "Solicite seu selo".to_string(),
            claim_html: r#"<h2>Parabéns!</h2>
<p>Você encontrará o seu selo no seu <a href="/dashboard/profile">perfil do SciStarter</a> e receberá um e-mail com instruções para compartilhar o selo verificado e seus metadados no LinkedIn.</p>"#.to_string(),
            sidebar_html: r#"<div>
    <h3>Agradecimentos</h3>
    <p class="text-sm">A tradução para o português do Brasil e para o espanhol da Caixa de Ferramentas de Ética de Dados para as Ciências Participativas é resultado de uma colaboração com a Wildlife Conservation Society (WCS), a Aliança Águas Amazônicas, a Rede Brasileira de Ciência Cidadã (RBCC) e a Red Iberoamericana de Ciencia Participativa (RICAP). Esta tradução foi parcialmente financiada pela Fundação Gordon e Betty Moore.</p>
    <hr class="my-8 w-[50px] text-primary-light" />
    <p class="text-sm">Este material se baseia nas contribuições de mais de 100 pessoas, de modo que reflete tanto áreas de amplo acordo quanto áreas com prioridades e valores divergentes. Os recursos foram desenvolvidos e testados em campo por uma equipe com experiência em projeto instrucional, educação, bibliotecas, práticas inclusivas, criação digital, microacreditação e, claro, ciência cidadã, com o objetivo de produzir uma série de tutoriais, formações e módulos complementares para ajudar pessoas de diferentes contextos a descobrir e participar da ciência autêntica.</p>
    <hr class="my-8 w-[50px] text-primary-light" />
    <p class="text-sm">Saiba mais sobre os <a class="text-sm" href="https://scistarter.org/go/badging-brochure-2025" target="_blank">selos da SciStarter</a>.</p>
    <a href="https://scistarter.org/go/badging-brochure-2025" rel="noopener noreferrer" target="_blank"><img class="border" src="https://media.scistarter.org/curated/2025/SciStarter_Badging_Program_Brochure_2025_-_C.jpg" ></a>
    <hr class="my-8 w-[50px] text-primary-light" />
    <p>Opcional: baixe a <em>versão em PDF</em> do <a href="https://scistarter.org/go/data-ethics-toolkit" target="_blank">kit de ferramentas Ética de Dados para Profissionais</a>.</p>
    <a href="https://scistarter.org/go/data-ethics-toolkit" rel="noopener noreferrer" target="_blank"><img class="border" src="https://orrery-media.s3.amazonaws.com/uploads/froala_editor/images/Data%20Ethics%20Toolkit%20thumbnail.png" ></a>
</div>"#.to_string(),
        }
    }
}

/// Full page at `/training/data-ethics`.
#[handler]
pub async fn page(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    render(DataEthicsLanguage::English, user_info).await
}

/// Full page at `/training/data-ethics-es`.
#[handler]
pub async fn page_es(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    render(DataEthicsLanguage::Spanish, user_info).await
}

/// Full page at `/training/data-ethics-pt`.
#[handler]
pub async fn page_pt(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    render(DataEthicsLanguage::Portuguese, user_info).await
}

async fn render(
    language: DataEthicsLanguage,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<Response, Error> {
    let content = DataEthicsContent::current(language).await;
    let rendered = build_content(&content, user_info.clone()).await?;
    render_page(
        language.base_path(),
        content.title.clone(),
        rendered,
        user_info,
    )
    .await
}

/// SSE-triggered fragment at `/training/data-ethics/-progress`.
#[handler]
pub async fn progress(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    render_progress(DataEthicsLanguage::English, user_info).await
}

/// SSE-triggered fragment at `/training/data-ethics-es/-progress`.
#[handler]
pub async fn progress_es(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    render_progress(DataEthicsLanguage::Spanish, user_info).await
}

/// SSE-triggered fragment at `/training/data-ethics-pt/-progress`.
#[handler]
pub async fn progress_pt(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    render_progress(DataEthicsLanguage::Portuguese, user_info).await
}

async fn render_progress(
    language: DataEthicsLanguage,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<Response, Error> {
    let content = DataEthicsContent::current(language).await;
    let steps = build_steps(&content, &user_info).await;
    let count = training::tutorials_completed(WorkflowType::DataEthics).await;
    render_progress_fragment(steps, count)
}

async fn build_content(
    content: &DataEthicsContent,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<TrainingContent, Error> {
    let live_updates = user_info.is_some();
    let steps = build_steps(content, &user_info).await;
    let tutorials_completed = training::tutorials_completed(WorkflowType::DataEthics).await;
    let sidebar_html = SidebarBody {
        badge_image: content.badge_image.clone(),
        html: content.sidebar_html.clone(),
    }
    .render_once()
    .unwrap_or_default();

    Ok(TrainingContent {
        chrome: content.language.chrome(),
        breadcrumb_label: content.title.clone(),
        badge_image: content.badge_image.clone(),
        badge_alt: "Data Ethics for Practitioners badge".to_string(),
        sponsor: content.sponsor.clone(),
        title: content.title.clone(),
        intro: content.intro.clone(),
        estimated_time: content.estimated_time.clone(),
        tutorials_completed,
        skills_intro: content.skills_intro.clone(),
        skills: content.skills.clone(),
        skills_footnote: content.skills_footnote.clone(),
        steps,
        sidebar_html,
        live_updates,
        sse_channel: "training".to_string(),
        sse_event: WorkflowType::DataEthics.slug().to_string(),
        progress_path: content.language.progress_path().to_string(),
    })
}

/// Three linear steps: account (signed in) → tutorial (recorded) → badge.
async fn build_steps(content: &DataEthicsContent, user_info: &Option<Arc<BasicInfo>>) -> StepsView {
    let logged_in = user_info.is_some();
    let status = match user_info {
        Some(user) => {
            training::fetch_status(WorkflowType::DataEthics, &user.user_id.to_string()).await
        }
        None => WorkflowStatusResponse {
            current_step: None,
            completed_steps: vec![],
            badge_granted: false,
        },
    };

    let tutorial_done = status
        .completed_steps
        .iter()
        .any(|step| step == content.language.step());
    // The tutorial is open to everyone: a visitor with no account can work
    // through it, they just cannot be awarded the badge at the end. Orrery's
    // three pages say the same thing with `data-unblocked`.
    let states = linear_states_ungating(
        &[logged_in, tutorial_done, status.badge_granted],
        &[false, true, false],
    );
    let opens = open_flags(&states);

    let account_body = SigninBody {
        logged_in,
        tutorial_pending: false,
        handle: user_info
            .as_ref()
            .map(|user| user.handle.clone())
            .unwrap_or_default(),
        next_path: content.language.base_path().to_string(),
        chrome: content.language.chrome(),
    }
    .render_once()
    .unwrap_or_default();

    let tutorial_href = tutorial_launch_url(
        module_url(&content.articulate_url, content.language.module_url()),
        Some(content.language.activity_id()),
        user_info,
    );

    let tutorial_body = TutorialBody {
        intro: content.tutorial_intro.clone(),
        cards: vec![ArticulateCard {
            href: tutorial_href,
            image: content.articulate_image.clone(),
        }],
        extra: String::new(),
    }
    .render_once()
    .unwrap_or_default();

    let claim_body = ClaimBody {
        html: content.claim_html.clone(),
    }
    .render_once()
    .unwrap_or_default();

    let steps = vec![
        StepView {
            heading: content.account_heading.clone(),
            state: states[0],
            open: opens[0],
            body: account_body,
        },
        StepView {
            heading: content.tutorial_heading.clone(),
            state: states[1],
            open: opens[1],
            body: tutorial_body,
        },
        StepView {
            heading: content.claim_heading.clone(),
            state: states[2],
            open: opens[2],
            body: claim_body,
        },
    ];

    StepsView { steps }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The launch passes the activity id with the casing the module's own
    /// `tincan.xml` uses; the callback in [`crate::api::v2`] compares against
    /// a lowercased copy of what the module reports. Two spellings of one
    /// string, in two files, and a mismatch is silent: the learner finishes
    /// and nothing is recorded.
    #[test]
    fn each_build_launches_the_activity_id_its_callback_matches() {
        assert_eq!(
            DataEthicsLanguage::English.activity_id().to_lowercase(),
            "http://dataethicsxapisept72022",
        );
        assert_eq!(
            DataEthicsLanguage::Spanish.activity_id().to_lowercase(),
            "http://4xi0y6kwb69qegbwdc8i-kkmgeymlbul_rise",
        );
        assert_eq!(
            DataEthicsLanguage::Portuguese.activity_id().to_lowercase(),
            "http://k632u-oqbl5kbcd5jfnucao1cr2sekoh_rise",
        );
    }

    /// A SCORM Driver package reports nothing without a launch context, so
    /// none of the three may be launched without its id. English was the last
    /// one that could: it was an older Rise export until Sep 2026, and the
    /// type stopped being an `Option` so that "no id" cannot be spelled again.
    #[test]
    fn no_build_launches_without_an_activity_id() {
        for language in [
            DataEthicsLanguage::English,
            DataEthicsLanguage::Spanish,
            DataEthicsLanguage::Portuguese,
        ] {
            assert!(
                !language.activity_id().is_empty(),
                "{language:?} has no activity id",
            );
        }
    }

    /// The steps are orrery's `DataEthicsState.TUTORIAL_COMPLETIONS` keys
    /// and the arms of user-service's `data_ethics_completion`. Neither can
    /// import this spelling, and a step nothing matches earns no badge at
    /// all.
    #[test]
    fn the_steps_are_spelled_as_both_sides_expect() {
        assert_eq!(DataEthicsLanguage::English.step(), "tutorial");
        assert_eq!(DataEthicsLanguage::Spanish.step(), "tutorial-es");
        assert_eq!(DataEthicsLanguage::Portuguese.step(), "tutorial-pt");
    }

    /// Every build is a SCORM Driver package, whose entry point is
    /// `scormdriver/indexAPI.html`; `index.html` 404s in all of them. English
    /// launches from `dataethics-v2/` — `dataethics/` still holds the 2022
    /// Rise export it replaced, which does 404 at that path.
    #[test]
    fn each_build_is_launched_at_its_own_entry_point() {
        assert!(
            DataEthicsLanguage::English
                .module_url()
                .ends_with("/dataethics-v2/scormdriver/indexAPI.html")
        );
        assert!(
            DataEthicsLanguage::Spanish
                .module_url()
                .ends_with("/dataethics-es/scormdriver/indexAPI.html")
        );
        assert!(
            DataEthicsLanguage::Portuguese
                .module_url()
                .ends_with("/dataethics-pt/scormdriver/indexAPI.html")
        );
    }

    /// Each language reads and writes its own row, and its built-in copy says
    /// so -- a record that named the wrong language would overwrite another
    /// language's page the first time an admin saved it.
    #[test]
    fn built_in_copy_names_the_language_it_belongs_to() {
        for language in [
            DataEthicsLanguage::English,
            DataEthicsLanguage::Spanish,
            DataEthicsLanguage::Portuguese,
        ] {
            assert_eq!(DataEthicsContent::built_in(language).language, language);
        }
    }

    /// A stored record written before the language field existed is the
    /// English one; `#[serde(default)]` has to fill it in as such rather than
    /// failing the read and silently reverting the page to built-in copy.
    #[test]
    fn content_saved_before_the_language_field_reads_back_as_english() {
        let stored = serde_json::json!({ "title": "Data Ethics for Practitioners" });
        let content: DataEthicsContent = serde_json::from_value(stored).expect("reads back");

        assert_eq!(content.language, DataEthicsLanguage::English);
        assert_eq!(content.title, "Data Ethics for Practitioners");
    }

    /// The three pages are distinct places, and the legacy orrery URLs point
    /// at them; a duplicated path would shadow a page in the router.
    #[test]
    fn every_language_has_its_own_paths() {
        let paths: Vec<&str> = [
            DataEthicsLanguage::English,
            DataEthicsLanguage::Spanish,
            DataEthicsLanguage::Portuguese,
        ]
        .iter()
        .flat_map(|language| {
            [
                language.base_path(),
                language.progress_path(),
                language.content_slug(),
            ]
        })
        .collect();

        let mut unique = paths.clone();
        unique.sort_unstable();
        unique.dedup();

        assert_eq!(paths.len(), unique.len(), "two languages share a path");
    }
}
