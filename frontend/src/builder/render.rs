//! Renders the template language

//! The template language consists of the following constructs, with
//! literal text between them:
//! - {$ if name $} .. {$ /if $} with optional {$ else if $} and {$ else $}
//! - {$ each name item_name } .. {$ /each $}
//! - {$ use name $} renders nothing directly, but invokes the named function to update the bindings
//! - {$= name $} to render the value bound to name
//! - {$- name $} to render the value bound to name, without HTML escaping
//! - One or more ^ can be included after the opening $, e.g. {$^^ if name $}
//!   or {$^= name $}, making the expression literal. When the
//!   expression is rendered, one ^ is removed and otherwise the
//!   expression is rendered literally into the output

use std::sync::Arc;

use anyhow::Error;

use super::context::Context;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum TokenKind {
    Text,
    Item,
}

#[derive(Copy, Clone, Debug)]
struct Token {
    kind: TokenKind,
    begin: usize,
    end: usize,
}

fn tokenize(template: &[u8]) -> Vec<Token> {
    let mut out = Vec::with_capacity(64);

    let mut token = Token {
        kind: TokenKind::Text,
        begin: 0,
        end: 0,
    };

    let mut last = b' ';
    let mut offset = 0;

    let mut kind = TokenKind::Text;

    for ch in template.iter() {
        let ch = *ch;

        if kind == TokenKind::Text && last == b'{' && ch == b'$' {
            token.end = offset - 1;
            token.kind = kind;
            out.push(token);
            token.begin = offset - 1;
            kind = TokenKind::Item;
        } else if kind == TokenKind::Item && last == b'$' && ch == b'}' {
            token.end = offset + 1;
            token.kind = kind;
            out.push(token);
            token.begin = offset + 1;
            kind = TokenKind::Text;
        }

        last = ch;
        offset += 1;
    }

    token.end = offset;
    token.kind = kind;
    out.push(token);

    out
}

pub async fn render_into(
    ctx: &Arc<Context>,
    template: &[u8],
    buffer: &mut Vec<u8>,
) -> Result<(), Error> {
    let tokens = tokenize(template);

    Ok(())
}

pub async fn render(ctx: &Arc<Context>, template: &str) -> Result<String, Error> {
    let mut buffer = Vec::with_capacity(template.len() * 2);
    render_into(ctx, template.as_bytes(), &mut buffer).await?;
    Ok(String::try_from(buffer)?)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use assert2::check;

    #[test]
    fn test_tokenization_empty() {
        let tokens = tokenize(b"");
        check!(1 == tokens.len());
        check!(tokens[0].begin == 0 && tokens[0].end == 0);
    }

    #[test]
    fn test_tokenization_single_item() {
        let tokens = tokenize(b"{$ if cows $}");
        check!(3 == tokens.len());
        check!(tokens[0].begin == 0 && tokens[0].end == 0);
        check!(tokens[1].begin == 0 && tokens[1].end == 13);
        check!(tokens[2].begin == 13 && tokens[2].end == 13);
    }

    #[test]
    fn test_tokenization_unterminated() {
        let tokens = tokenize(b"{$ if cows ");
        check!(2 == tokens.len());
        check!(tokens[0].begin == 0 && tokens[0].end == 0);
        check!(tokens[1].begin == 0 && tokens[1].end == 11);
    }

    #[test]
    fn test_tokenization_uninitiated() {
        let tokens = tokenize(b" if cows $}");
        check!(1 == tokens.len());
        check!(tokens[0].begin == 0 && tokens[0].end == 11);
    }

    #[test]
    fn test_tokenization_multiple_items() {
        let source = b"a{$ if cows $}b{$ else $}c{$ /if $}d";
        let tokens = tokenize(source);

        check!(7 == tokens.len());

        check!(tokens[0].begin == 0 && tokens[0].end == 1);
        check!(tokens[1].begin == 1 && tokens[1].end == 14);
        check!(tokens[2].begin == 14 && tokens[2].end == 15);
        check!(tokens[3].begin == 15 && tokens[3].end == 25);
        check!(tokens[4].begin == 25 && tokens[4].end == 26);
        check!(tokens[5].begin == 26 && tokens[5].end == 35);
        check!(tokens[6].begin == 35 && tokens[6].end == 36);

        check!(
            &[
                Cow::from("a"),
                Cow::from("{$ if cows $}"),
                Cow::from("b"),
                Cow::from("{$ else $}"),
                Cow::from("c"),
                Cow::from("{$ /if $}"),
                Cow::from("d")
            ] == &[
                String::from_utf8_lossy(&source[tokens[0].begin..tokens[0].end]),
                String::from_utf8_lossy(&source[tokens[1].begin..tokens[1].end]),
                String::from_utf8_lossy(&source[tokens[2].begin..tokens[2].end]),
                String::from_utf8_lossy(&source[tokens[3].begin..tokens[3].end]),
                String::from_utf8_lossy(&source[tokens[4].begin..tokens[4].end]),
                String::from_utf8_lossy(&source[tokens[5].begin..tokens[5].end]),
                String::from_utf8_lossy(&source[tokens[6].begin..tokens[6].end]),
            ]
        );
    }

    #[tokio::test]
    async fn test_template_identity() {}
}
