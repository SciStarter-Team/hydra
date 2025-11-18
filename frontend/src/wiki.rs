// Wiki functionality

use anyhow::Error;
use common::shared::db;
use serde::{Deserialize, Serialize};
use crate::peering::LoadStorePeered;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiPage {
    pub title: String,
    pub content: String,
}

impl LoadStorePeered for WikiPage {
    type DatabaseKey = String; // title
    const GROUP: &str = "wiki";
    const SPECIFIC: &str = "title";

    async fn save_to_database(&self) -> Result<(), Error> {
        sqlx::query!(
            "INSERT OR REPLACE INTO wiki_pages (title, content) VALUES (?, ?)",
            self.title,
            self.content
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(title: Self::DatabaseKey) -> Result<Self, Error> {
        let row = sqlx::query!(
            "SELECT title, content FROM wiki_pages WHERE title = ?",
            title
        )
        .fetch_one(db())
        .await?;

        Ok(WikiPage {
            title: row.title,
            content: row.content,
        })
    }

    async fn delete_from_database(title: Self::DatabaseKey) -> Result<(), Error> {
        sqlx::query!("DELETE FROM wiki_pages WHERE title = ?", title)
            .execute(db())
            .await?;
        Ok(())
    }
}

/// Render wiki content from Quill Delta JSON to HTML with wiki links parsed
pub fn render_wiki_content(delta_json: &str) -> String {
    // First convert Delta JSON to HTML
    let html = match common::quill::delta_to_html(delta_json) {
        Ok(html) => html,
        Err(_) => {
            // If Delta parsing fails, return error message
            return format!("<p class=\"text-red-600\">Error parsing wiki content</p>");
        }
    };

    // Then parse wiki links in the HTML
    parse_wiki_links_in_html(&html)
}

/// Parse wiki links in HTML content, converting [Page Name] to HTML links
fn parse_wiki_links_in_html(html: &str) -> String {
    let mut result = String::new();
    let mut chars = html.chars().peekable();
    let mut in_link = false;
    let mut link_text = String::new();

    while let Some(ch) = chars.next() {
        if ch == '[' && chars.peek() != Some(&'[') {
            // Start of wiki link
            in_link = true;
            link_text.clear();
        } else if ch == ']' && in_link {
            // End of wiki link
            in_link = false;
            let encoded_title = urlencoding::encode(&link_text);
            result.push_str(&format!(
                r#"<a href="/-admin/wiki/{}" class="text-indigo-600 hover:text-indigo-800 underline">{}</a>"#,
                encoded_title, link_text
            ));
        } else if in_link {
            // Inside wiki link
            link_text.push(ch);
        } else {
            // Regular content
            result.push(ch);
        }
    }

    result
}
