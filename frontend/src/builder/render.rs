//! Renders the template language
//!
//! The template language consists of the following constructs, with
//! literal text between them:
//! - {$ if name $} .. {$ /if $} with optional {$ else if $} and/or {$ else $} between
//! - {$ each name in name } .. {$ /each $}
//! - {$ use name $} renders nothing directly, but invokes the named function to update the local bindings
//! - {$= name $} to render the value bound to name
//! - {$- name $} to render the value bound to name, without HTML escaping
//! - One or more ^ can be included after the opening $, e.g. {$^^ if name $}
//!     or {$^= name $}, making the expression literal. When the
//!     expression is rendered, one ^ is removed and otherwise the
//!     expression is rendered literally into the output

use std::sync::Arc;

use anyhow::{Error, anyhow};
use smallvec::SmallVec;

use super::context::{Context, ContextMode};
use super::value::Value;

/// A compiled template ready for efficient rendering
#[derive(Debug, Clone)]
pub struct CompiledTemplate {
    nodes: Vec<Node>,
}

/// AST node representing a parsed template element
#[derive(Debug, Clone)]
enum Node {
    /// Literal text to output
    Text(Vec<u8>),
    /// Literal item (^ escape) - outputs {$ followed by these bytes
    Literal(Vec<u8>),
    /// Render a value with HTML escaping
    RenderEscaped(String),
    /// Render a value without escaping
    RenderRaw(String),
    /// Invoke a function
    Use {
        name: String,
        args: SmallVec<[String; 4]>,
    },
    /// Conditional with branches: Vec<(condition, body)> where None condition = else
    If {
        branches: SmallVec<[(Option<String>, Vec<Node>); 3]>,
    },
    /// Loop over a collection
    Each {
        var: String,
        collection: String,
        body: Vec<Node>,
    },
}

impl CompiledTemplate {
    /// Compile a template string into an efficient representation
    pub fn compile(template: &str) -> Result<Self, Error> {
        Self::compile_bytes(template.as_bytes())
    }

    /// Compile template bytes into an efficient representation
    pub fn compile_bytes(template: &[u8]) -> Result<Self, Error> {
        let tokens = tokenize(template);
        let nodes = parse_nodes(template, &tokens, 0, tokens.len())?;
        Ok(CompiledTemplate { nodes })
    }

    /// Render the compiled template to a string
    pub async fn render(&self, ctx: &Arc<Context>) -> Result<String, Error> {
        let mut buffer = Vec::with_capacity(256);
        self.render_into(ctx, &mut buffer).await?;
        String::from_utf8(buffer).map_err(|e| anyhow!("Invalid UTF-8 in output: {}", e))
    }

    /// Render the compiled template into a buffer
    pub async fn render_into(&self, ctx: &Arc<Context>, buffer: &mut Vec<u8>) -> Result<(), Error> {
        render_nodes(ctx, &self.nodes, buffer).await
    }
}

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

/// Represents a parsed item (the content between {$ and $})
#[derive(Debug, Clone, PartialEq)]
enum Item<'a> {
    /// Literal item (has ^ prefix, render with one fewer ^)
    Literal(&'a [u8]),
    /// {$ if name $}
    If(&'a str),
    /// {$ else if name $}
    ElseIf(&'a str),
    /// {$ else $}
    Else,
    /// {$ /if $}
    EndIf,
    /// {$ each var in collection $}
    Each { var: &'a str, collection: &'a str },
    /// {$ /each $}
    EndEach,
    /// {$ use name arg1 arg2 ... $}
    Use { name: &'a str, args: Vec<&'a str> },
    /// {$= name $} - render with HTML escaping
    RenderEscaped(&'a str),
    /// {$- name $} - render without escaping
    RenderRaw(&'a str),
}

/// Parse the content of an item token (between {$ and $})
fn parse_item(raw: &[u8]) -> Result<Item<'_>, Error> {
    // Strip {$ prefix and $} suffix
    if raw.len() < 4 {
        return Err(anyhow!("Item too short"));
    }

    let inner = &raw[2..raw.len() - 2];

    // Check for literal mode (^ after $)
    if !inner.is_empty() && inner[0] == b'^' {
        // Return as literal with one ^ removed
        return Ok(Item::Literal(&raw[3..raw.len()]));
    }

    let inner_str = std::str::from_utf8(inner)?.trim();

    // Check for value rendering
    if let Some(name) = inner_str.strip_prefix('=') {
        return Ok(Item::RenderEscaped(name.trim()));
    }

    if let Some(name) = inner_str.strip_prefix('-') {
        return Ok(Item::RenderRaw(name.trim()));
    }

    // Check for keywords
    if inner_str == "/if" {
        return Ok(Item::EndIf);
    }

    if inner_str == "/each" {
        return Ok(Item::EndEach);
    }

    if inner_str == "else" {
        return Ok(Item::Else);
    }

    if let Some(rest) = inner_str.strip_prefix("else if ") {
        return Ok(Item::ElseIf(rest.trim()));
    }

    if let Some(rest) = inner_str.strip_prefix("if ") {
        return Ok(Item::If(rest.trim()));
    }

    if let Some(rest) = inner_str.strip_prefix("each ") {
        // Parse "var in collection"
        let parts: Vec<&str> = rest.split(" in ").collect();
        if parts.len() != 2 {
            return Err(anyhow!("Invalid each syntax: expected 'var in collection'"));
        }
        return Ok(Item::Each {
            var: parts[0].trim(),
            collection: parts[1].trim(),
        });
    }

    if let Some(rest) = inner_str.strip_prefix("use ") {
        let mut parts = rest.split_whitespace();
        let name = parts
            .next()
            .ok_or_else(|| anyhow!("use requires a function name"))?;
        let args: Vec<&str> = parts.collect();
        return Ok(Item::Use { name, args });
    }

    Err(anyhow!("Unknown item: {}", inner_str))
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

/// Parse tokens into an AST of nodes
fn parse_nodes(
    template: &[u8],
    tokens: &[Token],
    start: usize,
    end: usize,
) -> Result<Vec<Node>, Error> {
    let mut nodes = Vec::new();
    let mut i = start;

    while i < end {
        let token = &tokens[i];

        match token.kind {
            TokenKind::Text => {
                let text = &template[token.begin..token.end];
                if !text.is_empty() {
                    nodes.push(Node::Text(text.to_vec()));
                }
                i += 1;
            }
            TokenKind::Item => {
                let raw = &template[token.begin..token.end];
                let item = parse_item(raw)?;

                match item {
                    Item::Literal(bytes) => {
                        nodes.push(Node::Literal(bytes.to_vec()));
                        i += 1;
                    }
                    Item::RenderEscaped(name) => {
                        nodes.push(Node::RenderEscaped(name.to_string()));
                        i += 1;
                    }
                    Item::RenderRaw(name) => {
                        nodes.push(Node::RenderRaw(name.to_string()));
                        i += 1;
                    }
                    Item::Use { name, args } => {
                        nodes.push(Node::Use {
                            name: name.to_string(),
                            args: args.iter().map(|s| s.to_string()).collect(),
                        });
                        i += 1;
                    }
                    Item::If(condition) => {
                        let (branches, end_idx) =
                            parse_if_branches(template, tokens, i, condition)?;
                        nodes.push(Node::If { branches });
                        i = end_idx + 1;
                    }
                    Item::Each { var, collection } => {
                        let (body, end_idx) = parse_each_body(template, tokens, i)?;
                        nodes.push(Node::Each {
                            var: var.to_string(),
                            collection: collection.to_string(),
                            body,
                        });
                        i = end_idx + 1;
                    }
                    Item::ElseIf(_) | Item::Else | Item::EndIf | Item::EndEach => {
                        // These should be handled by their parent constructs
                        return Err(anyhow!("Unexpected {:?} at top level", item));
                    }
                }
            }
        }
    }

    Ok(nodes)
}

/// Parse if/else-if/else/endif into branches, returning (branches, end_token_idx)
fn parse_if_branches(
    template: &[u8],
    tokens: &[Token],
    if_idx: usize,
    initial_condition: &str,
) -> Result<(SmallVec<[(Option<String>, Vec<Node>); 3]>, usize), Error> {
    let mut branches = SmallVec::new();
    let mut depth = 0;
    let mut branch_start = if_idx + 1;
    let mut current_cond: Option<String> = Some(initial_condition.to_string());

    for i in (if_idx + 1)..tokens.len() {
        let token = &tokens[i];

        if token.kind == TokenKind::Item {
            let raw = &template[token.begin..token.end];
            if let Ok(item) = parse_item(raw) {
                match item {
                    Item::If(_) => {
                        depth += 1;
                    }
                    Item::EndIf => {
                        if depth == 0 {
                            let body = parse_nodes(template, tokens, branch_start, i)?;
                            branches.push((current_cond, body));
                            return Ok((branches, i));
                        }
                        depth -= 1;
                    }
                    Item::ElseIf(cond) if depth == 0 => {
                        let body = parse_nodes(template, tokens, branch_start, i)?;
                        branches.push((current_cond, body));
                        current_cond = Some(cond.to_string());
                        branch_start = i + 1;
                    }
                    Item::Else if depth == 0 => {
                        let body = parse_nodes(template, tokens, branch_start, i)?;
                        branches.push((current_cond, body));
                        current_cond = None;
                        branch_start = i + 1;
                    }
                    _ => {}
                }
            }
        }
    }

    Err(anyhow!("Unclosed if block"))
}

/// Parse each body, returning (body_nodes, end_token_idx)
fn parse_each_body(
    template: &[u8],
    tokens: &[Token],
    each_idx: usize,
) -> Result<(Vec<Node>, usize), Error> {
    let mut depth = 0;
    let body_start = each_idx + 1;

    for i in (each_idx + 1)..tokens.len() {
        let token = &tokens[i];

        if token.kind == TokenKind::Item {
            let raw = &template[token.begin..token.end];
            if let Ok(item) = parse_item(raw) {
                match item {
                    Item::Each { .. } => {
                        depth += 1;
                    }
                    Item::EndEach => {
                        if depth == 0 {
                            let body = parse_nodes(template, tokens, body_start, i)?;
                            return Ok((body, i));
                        }
                        depth -= 1;
                    }
                    _ => {}
                }
            }
        }
    }

    Err(anyhow!("Unclosed each block"))
}

/// Render pre-compiled nodes to buffer
fn render_nodes<'a>(
    ctx: &'a Arc<Context>,
    nodes: &'a [Node],
    buffer: &'a mut Vec<u8>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Error>> + Send + 'a>> {
    Box::pin(async move {
        for node in nodes {
            match node {
                Node::Text(bytes) => {
                    buffer.extend_from_slice(bytes);
                }
                Node::Literal(bytes) => {
                    buffer.extend_from_slice(b"{$");
                    buffer.extend_from_slice(bytes);
                }
                Node::RenderEscaped(name) => {
                    if let Some(value) = ctx.value(name)? {
                        value.write_escaped(buffer);
                    }
                }
                Node::RenderRaw(name) => {
                    if let Some(value) = ctx.value(name)? {
                        value.write_to(buffer);
                    }
                }
                Node::Use { name, args } => {
                    let args_refs: SmallVec<[&str; 4]> = args.iter().map(|s| s.as_str()).collect();
                    ctx.dispatch(name, &args_refs).await?;
                }
                Node::If { branches } => {
                    if ctx.mode() == ContextMode::Editing {
                        // In editing mode, render all branches
                        for (_cond, body) in branches {
                            render_nodes(ctx, body, buffer).await?;
                        }
                    } else {
                        // In presenting mode, render only the first true branch
                        for (cond, body) in branches {
                            let should_render = match cond {
                                Some(name) => ctx.truth(name)?,
                                None => true, // else branch
                            };
                            if should_render {
                                render_nodes(ctx, body, buffer).await?;
                                break;
                            }
                        }
                    }
                }
                Node::Each {
                    var,
                    collection,
                    body,
                } => {
                    if ctx.mode() == ContextMode::Editing {
                        // In editing mode, render body exactly once as preview
                        render_nodes(ctx, body, buffer).await?;
                    } else if let Some(value) = ctx.value(collection)? {
                        match value {
                            Value::List(items) => {
                                for item_value in items {
                                    let child_ctx = Context::from_parent(ctx)?;
                                    child_ctx.bind_string(var, &item_value)?;
                                    render_nodes(&child_ctx, body, buffer).await?;
                                }
                            }
                            Value::Single(_) => {
                                let child_ctx = Context::from_parent(ctx)?;
                                child_ctx.bind_string(var, &value)?;
                                render_nodes(&child_ctx, body, buffer).await?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    })
}

/// Convenience function: compile and render in one step
pub async fn render_into(
    ctx: &Arc<Context>,
    template: &[u8],
    buffer: &mut Vec<u8>,
) -> Result<(), Error> {
    let compiled = CompiledTemplate::compile_bytes(template)?;
    compiled.render_into(ctx, buffer).await
}

/// Convenience function: compile and render to string in one step
pub async fn render(ctx: &Arc<Context>, template: &str) -> Result<String, Error> {
    let compiled = CompiledTemplate::compile(template)?;
    compiled.render(ctx).await
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use sqlx::SqlitePool;
    use tokio::sync::OnceCell;

    use super::*;
    use assert2::check;

    /// Get a static test database pool (async-safe)
    static TEST_POOL: OnceCell<SqlitePool> = OnceCell::const_new();

    async fn test_db() -> &'static SqlitePool {
        TEST_POOL
            .get_or_init(|| async { SqlitePool::connect(":memory:").await.unwrap() })
            .await
    }

    /// Create a test context in presenting mode
    async fn test_ctx() -> Arc<Context> {
        Context::new(test_db().await, &None, ContextMode::Presenting).unwrap()
    }

    /// Create a test context in editing mode
    async fn test_ctx_editing() -> Arc<Context> {
        Context::new(test_db().await, &None, ContextMode::Editing).unwrap()
    }

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

    #[test]
    fn test_parse_item_if() {
        let item = parse_item(b"{$ if foo $}").unwrap();
        check!(item == Item::If("foo"));
    }

    #[test]
    fn test_parse_item_else_if() {
        let item = parse_item(b"{$ else if bar $}").unwrap();
        check!(item == Item::ElseIf("bar"));
    }

    #[test]
    fn test_parse_item_else() {
        let item = parse_item(b"{$ else $}").unwrap();
        check!(item == Item::Else);
    }

    #[test]
    fn test_parse_item_endif() {
        let item = parse_item(b"{$ /if $}").unwrap();
        check!(item == Item::EndIf);
    }

    #[test]
    fn test_parse_item_each() {
        let item = parse_item(b"{$ each item in items $}").unwrap();
        check!(
            item == Item::Each {
                var: "item",
                collection: "items"
            }
        );
    }

    #[test]
    fn test_parse_item_endeach() {
        let item = parse_item(b"{$ /each $}").unwrap();
        check!(item == Item::EndEach);
    }

    #[test]
    fn test_parse_item_use() {
        let item = parse_item(b"{$ use load_data $}").unwrap();
        check!(
            item == Item::Use {
                name: "load_data",
                args: vec![]
            }
        );
    }

    #[test]
    fn test_parse_item_use_with_args() {
        let item = parse_item(b"{$ use load_data foo bar baz $}").unwrap();
        check!(
            item == Item::Use {
                name: "load_data",
                args: vec!["foo", "bar", "baz"]
            }
        );
    }

    #[test]
    fn test_parse_item_render_escaped() {
        let item = parse_item(b"{$= name $}").unwrap();
        check!(item == Item::RenderEscaped("name"));
    }

    #[test]
    fn test_parse_item_render_raw() {
        let item = parse_item(b"{$- html $}").unwrap();
        check!(item == Item::RenderRaw("html"));
    }

    #[test]
    fn test_parse_item_literal() {
        // {$^ if foo $} -> renders as {$ if foo $}
        let item = parse_item(b"{$^ if foo $}").unwrap();
        check!(item == Item::Literal(b" if foo $}"));
    }

    #[test]
    fn test_parse_item_literal_double_caret() {
        // {$^^ if foo $} -> renders as {$^ if foo $}
        let item = parse_item(b"{$^^ if foo $}").unwrap();
        check!(item == Item::Literal(b"^ if foo $}"));
    }

    #[test]
    fn test_html_escape() {
        let value = Value::from("<script>alert('xss')</script>");
        let mut buffer = Vec::new();
        value.write_escaped(&mut buffer);
        check!(buffer == b"&lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;");
    }

    #[test]
    fn test_html_escape_ampersand() {
        let value = Value::from("foo & bar");
        let mut buffer = Vec::new();
        value.write_escaped(&mut buffer);
        check!(buffer == b"foo &amp; bar");
    }

    #[test]
    fn test_html_escape_quotes() {
        let value = Value::from(r#"say "hello""#);
        let mut buffer = Vec::new();
        value.write_escaped(&mut buffer);
        check!(buffer == b"say &quot;hello&quot;");
    }

    #[test]
    fn test_parse_if_branches_simple() {
        let source = b"{$ if foo $}content{$ /if $}";
        let tokens = tokenize(source);
        let (branches, end_idx) = parse_if_branches(source, &tokens, 1, "foo").unwrap();

        check!(branches.len() == 1);
        check!(branches[0].0 == Some("foo".to_string()));
        check!(end_idx == 3);
    }

    #[test]
    fn test_parse_if_branches_with_else() {
        let source = b"{$ if foo $}yes{$ else $}no{$ /if $}";
        let tokens = tokenize(source);
        let (branches, end_idx) = parse_if_branches(source, &tokens, 1, "foo").unwrap();

        check!(branches.len() == 2);
        check!(branches[0].0 == Some("foo".to_string()));
        check!(branches[1].0 == None); // else branch
        check!(end_idx == 5);
    }

    #[test]
    fn test_parse_if_branches_with_else_if() {
        let source = b"{$ if a $}A{$ else if b $}B{$ else $}C{$ /if $}";
        let tokens = tokenize(source);
        let (branches, _end_idx) = parse_if_branches(source, &tokens, 1, "a").unwrap();

        check!(branches.len() == 3);
        check!(branches[0].0 == Some("a".to_string()));
        check!(branches[1].0 == Some("b".to_string()));
        check!(branches[2].0 == None);
    }

    #[test]
    fn test_parse_each_body() {
        let source = b"{$ each x in xs $}item{$ /each $}";
        let tokens = tokenize(source);
        let (body, end_idx) = parse_each_body(source, &tokens, 1).unwrap();

        check!(body.len() == 1);
        check!(end_idx == 3);
    }

    #[test]
    fn test_parse_each_body_nested() {
        let source = b"{$ each x in xs $}{$ each y in ys $}inner{$ /each $}outer{$ /each $}";
        let tokens = tokenize(source);
        let (body, end_idx) = parse_each_body(source, &tokens, 1).unwrap();

        // Should find the outer /each, not the inner one
        // Body contains: the inner each loop + "outer" text
        check!(body.len() == 2);
        check!(end_idx == 7);
    }

    // ==================== Rendering Tests ====================

    #[tokio::test]
    async fn test_render_plain_text() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "Hello, world!").await.unwrap();
        check!(result == "Hello, world!");
    }

    #[tokio::test]
    async fn test_render_empty() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "").await.unwrap();
        check!(result == "");
    }

    #[tokio::test]
    async fn test_render_variable_escaped() {
        let ctx = test_ctx().await;
        ctx.bind_string("name", "Alice").unwrap();
        let result = render(&ctx, "Hello, {$= name $}!").await.unwrap();
        check!(result == "Hello, Alice!");
    }

    #[tokio::test]
    async fn test_render_variable_escaped_html() {
        let ctx = test_ctx().await;
        ctx.bind_string("content", "<script>alert('xss')</script>")
            .unwrap();
        let result = render(&ctx, "Content: {$= content $}").await.unwrap();
        check!(result == "Content: &lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;");
    }

    #[tokio::test]
    async fn test_render_variable_raw() {
        let ctx = test_ctx().await;
        ctx.bind_string("html", "<b>bold</b>").unwrap();
        let result = render(&ctx, "Raw: {$- html $}").await.unwrap();
        check!(result == "Raw: <b>bold</b>");
    }

    #[tokio::test]
    async fn test_render_unbound_variable() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "Value: {$= missing $}!").await.unwrap();
        check!(result == "Value: !");
    }

    #[tokio::test]
    async fn test_render_if_true() {
        let ctx = test_ctx().await;
        ctx.bind_string("show", "yes").unwrap();
        let result = render(&ctx, "{$ if show $}visible{$ /if $}").await.unwrap();
        check!(result == "visible");
    }

    #[tokio::test]
    async fn test_render_if_false() {
        let ctx = test_ctx().await;
        ctx.bind_string("show", "").unwrap();
        let result = render(&ctx, "{$ if show $}visible{$ /if $}").await.unwrap();
        check!(result == "");
    }

    #[tokio::test]
    async fn test_render_if_unbound() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$ if missing $}visible{$ /if $}")
            .await
            .unwrap();
        check!(result == "");
    }

    #[tokio::test]
    async fn test_render_if_else_true() {
        let ctx = test_ctx().await;
        ctx.bind_string("cond", "1").unwrap();
        let result = render(&ctx, "{$ if cond $}yes{$ else $}no{$ /if $}")
            .await
            .unwrap();
        check!(result == "yes");
    }

    #[tokio::test]
    async fn test_render_if_else_false() {
        let ctx = test_ctx().await;
        ctx.bind_string("cond", "").unwrap();
        let result = render(&ctx, "{$ if cond $}yes{$ else $}no{$ /if $}")
            .await
            .unwrap();
        check!(result == "no");
    }

    #[tokio::test]
    async fn test_render_if_else_if() {
        let ctx = test_ctx().await;
        ctx.bind_string("b", "1").unwrap();
        let result = render(&ctx, "{$ if a $}A{$ else if b $}B{$ else $}C{$ /if $}")
            .await
            .unwrap();
        check!(result == "B");
    }

    #[tokio::test]
    async fn test_render_if_else_if_fallthrough() {
        let ctx = test_ctx().await;
        // Neither a nor b are bound/truthy
        let result = render(&ctx, "{$ if a $}A{$ else if b $}B{$ else $}C{$ /if $}")
            .await
            .unwrap();
        check!(result == "C");
    }

    #[tokio::test]
    async fn test_render_nested_if() {
        let ctx = test_ctx().await;
        ctx.bind_string("outer", "1").unwrap();
        ctx.bind_string("inner", "1").unwrap();
        let result = render(
            &ctx,
            "{$ if outer $}[{$ if inner $}nested{$ /if $}]{$ /if $}",
        )
        .await
        .unwrap();
        check!(result == "[nested]");
    }

    #[tokio::test]
    async fn test_render_each_list() {
        let ctx = test_ctx().await;
        ctx.bind(
            "items",
            Value::List(vec![Value::from("a"), Value::from("b"), Value::from("c")]),
        )
        .unwrap();
        let result = render(&ctx, "{$ each item in items $}[{$= item $}]{$ /each $}")
            .await
            .unwrap();
        check!(result == "[a][b][c]");
    }

    #[tokio::test]
    async fn test_render_each_empty() {
        let ctx = test_ctx().await;
        ctx.bind("items", Value::List(vec![])).unwrap();
        let result = render(&ctx, "{$ each item in items $}[{$= item $}]{$ /each $}")
            .await
            .unwrap();
        check!(result == "");
    }

    #[tokio::test]
    async fn test_render_each_unbound() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$ each item in missing $}[{$= item $}]{$ /each $}")
            .await
            .unwrap();
        check!(result == "");
    }

    #[tokio::test]
    async fn test_render_each_single_value() {
        let ctx = test_ctx().await;
        ctx.bind_string("item", "solo").unwrap();
        let result = render(&ctx, "{$ each x in item $}[{$= x $}]{$ /each $}")
            .await
            .unwrap();
        check!(result == "[solo]");
    }

    #[tokio::test]
    async fn test_render_nested_each() {
        let ctx = test_ctx().await;
        ctx.bind(
            "rows",
            Value::List(vec![Value::from("A"), Value::from("B")]),
        )
        .unwrap();
        ctx.bind(
            "cols",
            Value::List(vec![Value::from("1"), Value::from("2")]),
        )
        .unwrap();
        let result = render(
            &ctx,
            "{$ each r in rows $}{$ each c in cols $}{$= r $}{$= c $} {$ /each $}{$ /each $}",
        )
        .await
        .unwrap();
        check!(result == "A1 A2 B1 B2 ");
    }

    #[tokio::test]
    async fn test_render_literal_single_caret() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$^ if foo $}").await.unwrap();
        check!(result == "{$ if foo $}");
    }

    #[tokio::test]
    async fn test_render_literal_double_caret() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$^^ if foo $}").await.unwrap();
        check!(result == "{$^ if foo $}");
    }

    #[tokio::test]
    async fn test_render_literal_value() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$^= name $}").await.unwrap();
        check!(result == "{$= name $}");
    }

    #[tokio::test]
    async fn test_render_editing_mode_shows_all_branches() {
        let ctx = test_ctx_editing().await;
        ctx.bind_string("cond", "1").unwrap();
        let result = render(&ctx, "{$ if cond $}yes{$ else $}no{$ /if $}")
            .await
            .unwrap();
        check!(result == "yesno");
    }

    #[tokio::test]
    async fn test_render_editing_mode_each_once() {
        let ctx = test_ctx_editing().await;
        ctx.bind(
            "items",
            Value::List(vec![Value::from("a"), Value::from("b"), Value::from("c")]),
        )
        .unwrap();
        let result = render(&ctx, "{$ each item in items $}[item]{$ /each $}")
            .await
            .unwrap();
        // In editing mode, renders body once (without binding)
        check!(result == "[item]");
    }

    #[tokio::test]
    async fn test_render_mixed_content() {
        let ctx = test_ctx().await;
        ctx.bind_string("name", "World").unwrap();
        ctx.bind_string("show_greeting", "1").unwrap();
        let result = render(
            &ctx,
            "<div>{$ if show_greeting $}<h1>Hello, {$= name $}!</h1>{$ /if $}</div>",
        )
        .await
        .unwrap();
        check!(result == "<div><h1>Hello, World!</h1></div>");
    }

    #[tokio::test]
    async fn test_render_complex_template() {
        let ctx = test_ctx().await;
        ctx.bind_string("title", "My Page").unwrap();
        ctx.bind_string("logged_in", "1").unwrap();
        ctx.bind_string("username", "alice").unwrap();
        ctx.bind(
            "nav",
            Value::List(vec![
                Value::from("Home"),
                Value::from("About"),
                Value::from("Contact"),
            ]),
        )
        .unwrap();

        let template = r#"<html>
<head><title>{$= title $}</title></head>
<body>
{$ if logged_in $}<p>Welcome, {$= username $}!</p>{$ else $}<p>Please log in</p>{$ /if $}
<nav>{$ each link in nav $}<a>{$= link $}</a> {$ /each $}</nav>
</body>
</html>"#;

        let result = render(&ctx, template).await.unwrap();
        check!(result.contains("<title>My Page</title>"));
        check!(result.contains("Welcome, alice!"));
        check!(result.contains("<a>Home</a>"));
        check!(result.contains("<a>About</a>"));
        check!(result.contains("<a>Contact</a>"));
        check!(!result.contains("Please log in"));
    }

    #[tokio::test]
    async fn test_compiled_template_reuse() {
        let compiled = CompiledTemplate::compile("Hello, {$= name $}!").unwrap();

        let ctx1 = test_ctx().await;
        ctx1.bind_string("name", "Alice").unwrap();
        let result1 = compiled.render(&ctx1).await.unwrap();

        let ctx2 = test_ctx().await;
        ctx2.bind_string("name", "Bob").unwrap();
        let result2 = compiled.render(&ctx2).await.unwrap();

        check!(result1 == "Hello, Alice!");
        check!(result2 == "Hello, Bob!");
    }
}
