//! Renders the template language
//!
//! The template language consists of the following constructs, with
//! literal text between them:
//! - {$ if name $} .. {$ /if $} with optional {$ else if $} and/or {$ else $} between
//! - {$ each name in name } .. {$ /each $}
//! - {$ times N $} .. {$ /times $} repeats body N times (N can be a number or variable name)
//! - {$ slot name [component=ID key=value...] $} .. {$ /slot $} defines a slot for page builder
//! - {$ param name [label="Label" default="value" description="Help text"] $} declares a parameter
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
use ulid::Ulid;

use super::context::{Context, ContextMode};
use super::value::{Expression, Value};

/// The type of input control to use for a parameter in the editor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParamInputType {
    /// Single-line text input (default)
    #[default]
    Text,
    /// Multi-line textarea
    Textarea,
    /// Rich text editor using Quill JS (stores HTML)
    RichText,
    /// URL input with file upload option
    FileUrl,
}

/// A parameter definition extracted from a template
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDef {
    /// The variable name used in the template
    pub name: String,
    /// Human-readable label for the parameter
    pub label: String,
    /// Optional description/help text
    pub description: String,
    /// Default value if provided
    pub default: Option<Value>,
    /// Input control type for the editor
    pub input_type: ParamInputType,
}

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
    RenderEscaped(Expression),
    /// Render a value without escaping
    RenderRaw(Expression),
    /// Invoke a function
    Use {
        name: String,
        args: SmallVec<[String; 4]>,
    },
    /// Conditional with branches: Vec<(condition, body)> where None condition = else
    If {
        branches: SmallVec<[(Option<Expression>, Vec<Node>); 3]>,
    },
    /// Loop over a collection
    Each {
        var: String,
        collection: Expression,
        body: Vec<Node>,
    },
    /// Repeat body N times
    Times { count: Expression, body: Vec<Node> },
    /// Slot block for page builder
    /// In editing mode: renders as droppable container with metadata
    /// In presenting mode: applies param bindings and renders body
    Slot {
        name: String,
        component_id: Option<Ulid>,
        params: SmallVec<[(String, String); 4]>,
        body: Vec<Node>,
    },
    /// Parameter declaration - renders nothing but declares a configurable parameter
    Param(ParamDef),
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

    /// Extract all variable names referenced in the template
    /// Returns unique variable names, excluding loop-bound variables
    pub fn extract_variables(&self) -> Vec<String> {
        let mut vars = std::collections::HashSet::new();
        let mut bound_vars = std::collections::HashSet::new();
        // Add built-in variables that shouldn't be treated as parameters
        bound_vars.insert("iteration".to_string());
        extract_vars_from_nodes(&self.nodes, &mut vars, &bound_vars);
        let mut result: Vec<String> = vars.into_iter().collect();
        result.sort();
        result
    }

    /// Extract all parameter definitions from the template
    /// Parameters must appear at the start of the template before any other content
    pub fn extract_params(&self) -> Vec<ParamDef> {
        let mut params = Vec::new();
        for node in &self.nodes {
            match node {
                Node::Param(param_def) => params.push(param_def.clone()),
                Node::Text(bytes) if bytes.iter().all(|b| b.is_ascii_whitespace()) => {
                    // Skip whitespace-only text nodes between params
                    continue;
                }
                _ => break, // Stop at first non-param, non-whitespace node
            }
        }
        params
    }
}

/// Recursively extract variable names from AST nodes
fn extract_vars_from_nodes(
    nodes: &[Node],
    vars: &mut std::collections::HashSet<String>,
    bound_vars: &std::collections::HashSet<String>,
) {
    for node in nodes {
        match node {
            Node::Text(_) | Node::Literal(_) => {}
            Node::RenderEscaped(expr) | Node::RenderRaw(expr) => {
                if let Expression::Variable(name) = expr {
                    if !bound_vars.contains(name) {
                        vars.insert(name.clone());
                    }
                }
            }
            Node::Use { args, .. } => {
                for arg in args {
                    if !bound_vars.contains(arg) {
                        vars.insert(arg.clone());
                    }
                }
            }
            Node::If { branches } => {
                for (cond, body) in branches {
                    if let Some(Expression::Variable(name)) = cond {
                        if !bound_vars.contains(name) {
                            vars.insert(name.clone());
                        }
                    }
                    extract_vars_from_nodes(body, vars, bound_vars);
                }
            }
            Node::Each {
                var,
                collection,
                body,
            } => {
                if let Expression::Variable(name) = collection {
                    if !bound_vars.contains(name) {
                        vars.insert(name.clone());
                    }
                }
                // The loop variable is bound inside the body
                let mut inner_bound = bound_vars.clone();
                inner_bound.insert(var.clone());
                extract_vars_from_nodes(body, vars, &inner_bound);
            }
            Node::Times { count, body } => {
                if let Expression::Variable(name) = count {
                    if !bound_vars.contains(name) {
                        vars.insert(name.clone());
                    }
                }
                extract_vars_from_nodes(body, vars, bound_vars);
            }
            Node::Slot { body, .. } => {
                extract_vars_from_nodes(body, vars, bound_vars);
            }
            Node::Param(_) => {
                // Param declarations don't reference variables
            }
        }
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
    /// {$ if expr $}
    If(Expression),
    /// {$ else if expr $}
    ElseIf(Expression),
    /// {$ else $}
    Else,
    /// {$ /if $}
    EndIf,
    /// {$ each var in expr $}
    Each {
        var: &'a str,
        collection: Expression,
    },
    /// {$ /each $}
    EndEach,
    /// {$ times expr $} where expr evaluates to a count
    Times(Expression),
    /// {$ /times $}
    EndTimes,
    /// {$ use name arg1 arg2 ... $}
    Use { name: &'a str, args: Vec<&'a str> },
    /// {$= expr $} - render with HTML escaping
    RenderEscaped(Expression),
    /// {$- expr $} - render without escaping
    RenderRaw(Expression),
    /// {$ slot name [component=ID key=value...] $} - start of slot block
    Slot {
        name: &'a str,
        component: Option<&'a str>,
        params: Vec<(&'a str, String)>,
    },
    /// {$ /slot $} - end of slot block
    EndSlot,
    /// {$ param name [label="Label" default="value" description="Help" type="textarea"] $}
    /// The default is an Expression (variable lookups are rejected during parsing)
    Param {
        name: &'a str,
        label: Option<&'a str>,
        default: Option<Expression>,
        description: Option<&'a str>,
        input_type: ParamInputType,
    },
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
    if let Some(expr_str) = inner_str.strip_prefix('=') {
        let expr = Expression::parse(expr_str.trim())?;
        return Ok(Item::RenderEscaped(expr));
    }

    if let Some(expr_str) = inner_str.strip_prefix('-') {
        let expr = Expression::parse(expr_str.trim())?;
        return Ok(Item::RenderRaw(expr));
    }

    // Check for keywords
    if inner_str == "/if" {
        return Ok(Item::EndIf);
    }

    if inner_str == "/each" {
        return Ok(Item::EndEach);
    }

    if inner_str == "/times" {
        return Ok(Item::EndTimes);
    }

    if inner_str == "/slot" {
        return Ok(Item::EndSlot);
    }

    if inner_str == "else" {
        return Ok(Item::Else);
    }

    if let Some(rest) = inner_str.strip_prefix("else if ") {
        let expr = Expression::parse(rest.trim())?;
        return Ok(Item::ElseIf(expr));
    }

    if let Some(rest) = inner_str.strip_prefix("if ") {
        let expr = Expression::parse(rest.trim())?;
        return Ok(Item::If(expr));
    }

    if let Some(rest) = inner_str.strip_prefix("each ") {
        // Parse "var in collection"
        let parts: Vec<&str> = rest.split(" in ").collect();
        if parts.len() != 2 {
            return Err(anyhow!("Invalid each syntax: expected 'var in collection'"));
        }
        let collection = Expression::parse(parts[1].trim())?;
        return Ok(Item::Each {
            var: parts[0].trim(),
            collection,
        });
    }

    if let Some(rest) = inner_str.strip_prefix("times ") {
        let expr = Expression::parse(rest.trim())?;
        return Ok(Item::Times(expr));
    }

    if let Some(rest) = inner_str.strip_prefix("use ") {
        let mut parts = rest.split_whitespace();
        let name = parts
            .next()
            .ok_or_else(|| anyhow!("use requires a function name"))?;
        let args: Vec<&str> = parts.collect();
        return Ok(Item::Use { name, args });
    }

    if let Some(rest) = inner_str.strip_prefix("slot ") {
        return parse_slot_item(rest.trim());
    }

    if inner_str == "slot" {
        return Err(anyhow!("slot requires a name"));
    }

    if let Some(rest) = inner_str.strip_prefix("param ") {
        return parse_param_item(rest.trim());
    }

    if inner_str == "param" {
        return Err(anyhow!("param requires a name"));
    }

    Err(anyhow!("Unknown item: {}", inner_str))
}

/// Parse slot item: `name [component=ID] [key=value...]`
fn parse_slot_item(rest: &str) -> Result<Item<'_>, Error> {
    use super::parse::{parse_attributes, unescape_value};

    let mut parser = parse_attributes(rest);
    let name = parser
        .name()
        .ok_or_else(|| anyhow!("slot requires a name"))?;

    let mut component = None;
    let mut params = Vec::new();

    for attr in parser {
        if attr.key == "component" {
            component = Some(attr.value);
        } else {
            // Unescape the value to handle escaped quotes in param values
            params.push((attr.key, unescape_value(attr.value)));
        }
    }

    Ok(Item::Slot {
        name,
        component,
        params,
    })
}

/// Parsed param attribute value
enum ParamAttrValue<'a> {
    /// String value from quoted attribute: label="Hello" or label='Hello'
    String(&'a str),
    /// Integer value from unquoted attribute: default=42
    Integer(i64),
}

/// Parse param item: `name [label="Label"] [default="value" or default=42] [description="Help"]`
fn parse_param_item(rest: &str) -> Result<Item<'_>, Error> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Err(anyhow!("param requires a name"));
    }

    // Find end of name (first whitespace or end of string)
    let mut name_end = rest.len();
    for (i, c) in rest.char_indices() {
        if c.is_whitespace() {
            name_end = i;
            break;
        }
    }

    let name = &rest[..name_end];
    if name.is_empty() {
        return Err(anyhow!("param requires a name"));
    }

    let attrs_str = rest[name_end..].trim();

    let mut label = None;
    let mut default = None;
    let mut description = None;
    let mut input_type = ParamInputType::Text;

    // Parse key=value or key="value" or key='value' pairs
    let mut remaining = attrs_str;
    while !remaining.is_empty() {
        remaining = remaining.trim_start();
        if remaining.is_empty() {
            break;
        }

        // Find key
        let eq_pos = remaining
            .find('=')
            .ok_or_else(|| anyhow!("Invalid param attribute syntax"))?;
        let key = remaining[..eq_pos].trim();
        remaining = &remaining[eq_pos + 1..];
        remaining = remaining.trim_start();

        // Parse value - can be quoted (string) or unquoted (integer)
        let attr_value = if remaining.starts_with('"') {
            // Double-quoted string
            remaining = &remaining[1..];
            let close_pos = remaining
                .find('"')
                .ok_or_else(|| anyhow!("Unterminated quoted value"))?;
            let value = &remaining[..close_pos];
            remaining = &remaining[close_pos + 1..];
            ParamAttrValue::String(value)
        } else if remaining.starts_with('\'') {
            // Single-quoted string
            remaining = &remaining[1..];
            let close_pos = remaining
                .find('\'')
                .ok_or_else(|| anyhow!("Unterminated quoted value"))?;
            let value = &remaining[..close_pos];
            remaining = &remaining[close_pos + 1..];
            ParamAttrValue::String(value)
        } else {
            // Unquoted value - must be an integer, read until whitespace
            let end_pos = remaining
                .find(char::is_whitespace)
                .unwrap_or(remaining.len());
            let value_str = &remaining[..end_pos];
            remaining = &remaining[end_pos..];
            let n = value_str.parse::<i64>().map_err(|_| {
                anyhow!(
                    "Unquoted param attribute value must be an integer: '{}'",
                    value_str
                )
            })?;
            ParamAttrValue::Integer(n)
        };

        match key {
            "label" => {
                if let ParamAttrValue::String(s) = attr_value {
                    label = Some(s);
                } else {
                    return Err(anyhow!("label must be a quoted string"));
                }
            }
            "default" => {
                default = Some(match attr_value {
                    ParamAttrValue::String(s) => Expression::StringLiteral(s.to_string()),
                    ParamAttrValue::Integer(n) => Expression::IntegerLiteral(n),
                });
            }
            "description" => {
                if let ParamAttrValue::String(s) = attr_value {
                    description = Some(s);
                } else {
                    return Err(anyhow!("description must be a quoted string"));
                }
            }
            "type" => {
                if let ParamAttrValue::String(s) = attr_value {
                    input_type = match s {
                        "text" => ParamInputType::Text,
                        "textarea" => ParamInputType::Textarea,
                        "richtext" => ParamInputType::RichText,
                        "fileurl" => ParamInputType::FileUrl,
                        _ => {
                            return Err(anyhow!(
                                "Unknown param type: '{}'. Valid types: text, textarea, richtext, fileurl",
                                s
                            ));
                        }
                    };
                } else {
                    return Err(anyhow!("type must be a quoted string"));
                }
            }
            _ => {} // Ignore unknown attributes
        }
    }

    Ok(Item::Param {
        name,
        label,
        default,
        description,
        input_type,
    })
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
                    Item::RenderEscaped(expr) => {
                        nodes.push(Node::RenderEscaped(expr));
                        i += 1;
                    }
                    Item::RenderRaw(expr) => {
                        nodes.push(Node::RenderRaw(expr));
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
                            collection,
                            body,
                        });
                        i = end_idx + 1;
                    }
                    Item::Times(count) => {
                        let (body, end_idx) = parse_times_body(template, tokens, i)?;
                        nodes.push(Node::Times { count, body });
                        i = end_idx + 1;
                    }
                    Item::Slot {
                        name,
                        component,
                        params,
                    } => {
                        let (body, end_idx) = parse_slot_body(template, tokens, i)?;
                        let component_id = component
                            .map(|s| Ulid::from_string(s))
                            .transpose()
                            .map_err(|e| anyhow!("Invalid component ULID: {}", e))?;
                        nodes.push(Node::Slot {
                            name: name.to_string(),
                            component_id,
                            params: params
                                .into_iter()
                                .map(|(k, v)| (k.to_string(), v))
                                .collect(),
                            body,
                        });
                        i = end_idx + 1;
                    }
                    Item::Param {
                        name,
                        label,
                        default,
                        description,
                        input_type,
                    } => {
                        // Convert to titlecase for default label
                        let default_label = name
                            .chars()
                            .enumerate()
                            .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
                            .collect::<String>()
                            .replace('_', " ");
                        // Convert expression to Value
                        let default_value = match default {
                            Some(Expression::StringLiteral(s)) => Some(Value::String(s)),
                            Some(Expression::IntegerLiteral(n)) => Some(Value::Integer(n)),
                            Some(Expression::Variable(_)) => {
                                // Should not happen - validated in parse_param_item
                                None
                            }
                            None => None,
                        };
                        nodes.push(Node::Param(ParamDef {
                            name: name.to_string(),
                            label: label.map(|s| s.to_string()).unwrap_or(default_label),
                            default: default_value,
                            description: description.unwrap_or("").to_string(),
                            input_type,
                        }));
                        i += 1;
                    }
                    Item::ElseIf(_)
                    | Item::Else
                    | Item::EndIf
                    | Item::EndEach
                    | Item::EndTimes
                    | Item::EndSlot => {
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
    initial_condition: Expression,
) -> Result<(SmallVec<[(Option<Expression>, Vec<Node>); 3]>, usize), Error> {
    let mut branches = SmallVec::new();
    let mut depth = 0;
    let mut branch_start = if_idx + 1;
    let mut current_cond: Option<Expression> = Some(initial_condition);

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
                        current_cond = Some(cond);
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

/// Parse times body, returning (body_nodes, end_token_idx)
fn parse_times_body(
    template: &[u8],
    tokens: &[Token],
    times_idx: usize,
) -> Result<(Vec<Node>, usize), Error> {
    let mut depth = 0;
    let body_start = times_idx + 1;

    for i in (times_idx + 1)..tokens.len() {
        let token = &tokens[i];

        if token.kind == TokenKind::Item {
            let raw = &template[token.begin..token.end];
            if let Ok(item) = parse_item(raw) {
                match item {
                    Item::Times(_) => {
                        depth += 1;
                    }
                    Item::EndTimes => {
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

    Err(anyhow!("Unclosed times block"))
}

/// Parse slot body, returning (body_nodes, end_token_idx)
fn parse_slot_body(
    template: &[u8],
    tokens: &[Token],
    slot_idx: usize,
) -> Result<(Vec<Node>, usize), Error> {
    let mut depth = 0;
    let body_start = slot_idx + 1;

    for i in (slot_idx + 1)..tokens.len() {
        let token = &tokens[i];

        if token.kind == TokenKind::Item {
            let raw = &template[token.begin..token.end];
            if let Ok(item) = parse_item(raw) {
                match item {
                    Item::Slot { .. } => {
                        depth += 1;
                    }
                    Item::EndSlot => {
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

    Err(anyhow!("Unclosed slot block"))
}

/// Evaluate an expression in the given context
fn evaluate_expr(ctx: &Arc<Context>, expr: &Expression) -> Result<Option<Value>, Error> {
    match expr {
        Expression::Variable(name) => ctx.value(name),
        Expression::StringLiteral(s) => Ok(Some(Value::String(s.clone()))),
        Expression::IntegerLiteral(n) => Ok(Some(Value::Integer(*n))),
    }
}

/// Evaluate an expression's truthiness
fn evaluate_truth(ctx: &Arc<Context>, expr: &Expression) -> Result<bool, Error> {
    match expr {
        Expression::Variable(name) => ctx.truth(name),
        Expression::StringLiteral(s) => Ok(!s.is_empty()),
        Expression::IntegerLiteral(n) => Ok(*n != 0),
    }
}

/// Get the collection name for iteration variable naming
fn collection_name(expr: &Expression) -> &str {
    match expr {
        Expression::Variable(name) => name,
        Expression::StringLiteral(_) => "lit",
        Expression::IntegerLiteral(_) => "num",
    }
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
                    // Count leading ^ to determine depth level
                    // One ^ was already stripped by the parser, so:
                    // - no leading ^ in bytes = depth 1
                    // - one leading ^ = depth 2, etc.
                    let mut depth = 1;
                    while depth - 1 < bytes.len() && bytes[depth - 1] == b'^' {
                        depth += 1;
                    }

                    // Output literal with *varname interpolation support
                    // Interpolation only happens when # of * matches depth
                    buffer.extend_from_slice(b"{$");

                    let mut i = 0;
                    while i < bytes.len() {
                        // Look for * pattern - count consecutive *s
                        if bytes[i] == b'*' {
                            let star_start = i;
                            let mut star_count = 0;
                            while i < bytes.len() && bytes[i] == b'*' {
                                star_count += 1;
                                i += 1;
                            }

                            // Read variable name (alphanumeric and underscore)
                            let var_start = i;
                            while i < bytes.len()
                                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                            {
                                i += 1;
                            }

                            if i > var_start && star_count == depth {
                                // Interpolate: # of * matches depth
                                let var_name =
                                    std::str::from_utf8(&bytes[var_start..i]).unwrap_or("");
                                if let Ok(Some(value)) = ctx.value(var_name) {
                                    value.write_to(buffer);
                                }
                            } else {
                                // Don't interpolate: output stars and variable literally
                                buffer.extend_from_slice(&bytes[star_start..i]);
                            }
                            continue;
                        }
                        buffer.push(bytes[i]);
                        i += 1;
                    }
                }
                Node::RenderEscaped(expr) => {
                    if let Some(value) = evaluate_expr(ctx, expr)? {
                        value.write_escaped(buffer);
                    }
                }
                Node::RenderRaw(expr) => {
                    if let Some(value) = evaluate_expr(ctx, expr)? {
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
                                Some(expr) => evaluate_truth(ctx, expr)?,
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
                    // In both editing and presenting modes, iterate over all values
                    let coll_name = collection_name(collection);
                    if let Some(value) = evaluate_expr(ctx, collection)? {
                        match value {
                            Value::List(items) => {
                                for (i, item_value) in items.iter().enumerate() {
                                    let child_ctx = Context::from_parent(ctx)?;
                                    child_ctx.bind_string(var, item_value)?;
                                    // Bind iteration with unique slot-safe identifier
                                    let iteration_id = format!("{}_{}", coll_name, i);
                                    child_ctx.bind_string("iteration", &iteration_id)?;
                                    render_nodes(&child_ctx, body, buffer).await?;
                                }
                            }
                            Value::String(_) | Value::Integer(_) => {
                                let child_ctx = Context::from_parent(ctx)?;
                                child_ctx.bind_string(var, &value)?;
                                let iteration_id = format!("{}_0", coll_name);
                                child_ctx.bind_string("iteration", &iteration_id)?;
                                render_nodes(&child_ctx, body, buffer).await?;
                            }
                        }
                    }
                }
                Node::Times { count, body } => {
                    // Resolve count using expression evaluation
                    let n: usize = match count {
                        Expression::IntegerLiteral(n) => (*n).max(0) as usize,
                        Expression::Variable(_) | Expression::StringLiteral(_) => {
                            if let Some(value) = evaluate_expr(ctx, count)? {
                                match value {
                                    Value::String(s) => s.parse::<usize>().unwrap_or(0),
                                    Value::Integer(n) => n.max(0) as usize,
                                    Value::List(items) => items.len(),
                                }
                            } else {
                                0
                            }
                        }
                    };

                    for i in 0..n {
                        let child_ctx = Context::from_parent(ctx)?;
                        // Bind iteration with unique slot-safe identifier
                        let iteration_id = format!("t_{}", i);
                        child_ctx.bind_string("iteration", &iteration_id)?;
                        render_nodes(&child_ctx, body, buffer).await?;
                    }
                }
                Node::Slot {
                    name,
                    component_id,
                    params,
                    body,
                } => {
                    if ctx.mode() == ContextMode::Editing {
                        // Build the hierarchical slot path (using . as separator for URL safety)
                        let parent_path = ctx
                            .value("__slot_path__")
                            .ok()
                            .flatten()
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        let slot_path = if parent_path.is_empty() {
                            name.to_string()
                        } else {
                            format!("{}.{}", parent_path, name)
                        };

                        // In editing mode, render as droppable container with metadata
                        buffer.extend_from_slice(b"<div class=\"pb-slot");
                        if body.is_empty() {
                            buffer.extend_from_slice(b" pb-slot-empty");
                        } else {
                            buffer.extend_from_slice(b" pb-slot-filled");
                        }
                        buffer.extend_from_slice(b"\" data-slot-name=\"");
                        buffer.extend_from_slice(name.as_bytes());
                        buffer.extend_from_slice(b"\" data-slot-path=\"");
                        buffer.extend_from_slice(slot_path.as_bytes());
                        buffer.extend_from_slice(b"\"");

                        if let Some(cid) = component_id {
                            buffer.extend_from_slice(b" data-component-id=\"");
                            buffer.extend_from_slice(cid.to_string().as_bytes());
                            buffer.extend_from_slice(b"\" draggable=\"true\"");
                        }

                        buffer.extend_from_slice(b" data-pb-droppable=\"true\">");

                        if body.is_empty() {
                            // Empty state placeholder
                            buffer.extend_from_slice(
                                b"<span class=\"pb-slot-label\">Drop component here (",
                            );
                            buffer.extend_from_slice(name.as_bytes());
                            buffer.extend_from_slice(b")</span>");
                        } else {
                            // Slot label that appears on hover for filled slots
                            // Show full path with clickable segments
                            buffer.extend_from_slice(b"<div class=\"pb-slot-badge\">");

                            let parts: Vec<&str> = slot_path.split('.').collect();
                            for (i, part) in parts.iter().enumerate() {
                                if i > 0 {
                                    buffer.extend_from_slice(
                                        b"<span class=\"pb-badge-sep\">/</span>",
                                    );
                                }
                                // Build the path up to this segment
                                let partial_path: String = parts[..=i].join(".");
                                buffer.extend_from_slice(b"<button type=\"button\" class=\"pb-badge-link\" draggable=\"true\" data-slot-path=\"");
                                buffer.extend_from_slice(partial_path.as_bytes());
                                buffer.extend_from_slice(b"\" hx-on:click=\"selectSlot('");
                                buffer.extend_from_slice(partial_path.as_bytes());
                                buffer.extend_from_slice(b"')\">");
                                buffer.extend_from_slice(part.as_bytes());
                                buffer.extend_from_slice(b"</button>");
                            }

                            buffer.extend_from_slice(b"</div>");

                            // Render body content with path context
                            let child_ctx = Context::from_parent(ctx)?;
                            child_ctx.bind_string("__slot_path__", &slot_path)?;
                            for (key, value) in params {
                                child_ctx.bind_string(key, value)?;
                            }
                            render_nodes(&child_ctx, body, buffer).await?;
                        }

                        buffer.extend_from_slice(b"</div>");
                    } else {
                        // In presenting mode, apply params and render body directly
                        let child_ctx = Context::from_parent(ctx)?;
                        for (key, value) in params {
                            child_ctx.bind_string(key, value)?;
                        }
                        render_nodes(&child_ctx, body, buffer).await?;
                    }
                }
                Node::Param(_) => {
                    // Param declarations render nothing - they're just metadata
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
        check!(item == Item::If(Expression::Variable("foo".to_string())));
    }

    #[test]
    fn test_parse_item_else_if() {
        let item = parse_item(b"{$ else if bar $}").unwrap();
        check!(item == Item::ElseIf(Expression::Variable("bar".to_string())));
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
                collection: Expression::Variable("items".to_string())
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
        check!(item == Item::RenderEscaped(Expression::Variable("name".to_string())));
    }

    #[test]
    fn test_parse_item_render_raw() {
        let item = parse_item(b"{$- html $}").unwrap();
        check!(item == Item::RenderRaw(Expression::Variable("html".to_string())));
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
        let initial_cond = Expression::Variable("foo".to_string());
        let (branches, end_idx) =
            parse_if_branches(source, &tokens, 1, initial_cond.clone()).unwrap();

        check!(branches.len() == 1);
        check!(branches[0].0 == Some(initial_cond));
        check!(end_idx == 3);
    }

    #[test]
    fn test_parse_if_branches_with_else() {
        let source = b"{$ if foo $}yes{$ else $}no{$ /if $}";
        let tokens = tokenize(source);
        let initial_cond = Expression::Variable("foo".to_string());
        let (branches, end_idx) =
            parse_if_branches(source, &tokens, 1, initial_cond.clone()).unwrap();

        check!(branches.len() == 2);
        check!(branches[0].0 == Some(initial_cond));
        check!(branches[1].0 == None); // else branch
        check!(end_idx == 5);
    }

    #[test]
    fn test_parse_if_branches_with_else_if() {
        let source = b"{$ if a $}A{$ else if b $}B{$ else $}C{$ /if $}";
        let tokens = tokenize(source);
        let initial_cond = Expression::Variable("a".to_string());
        let (branches, _end_idx) =
            parse_if_branches(source, &tokens, 1, initial_cond.clone()).unwrap();

        check!(branches.len() == 3);
        check!(branches[0].0 == Some(initial_cond));
        check!(branches[1].0 == Some(Expression::Variable("b".to_string())));
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
    async fn test_render_literal_interpolation() {
        let ctx = test_ctx().await;
        ctx.bind_string("slot_name", "header").unwrap();
        // One ^ means one * triggers interpolation
        let result = render(&ctx, "{$^ slot *slot_name $}").await.unwrap();
        check!(result == "{$ slot header $}");
    }

    #[tokio::test]
    async fn test_render_literal_interpolation_in_times() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$ times 2 $}[{$^ slot *iteration $}]{$ /times $}")
            .await
            .unwrap();
        check!(result == "[{$ slot t_0 $}][{$ slot t_1 $}]");
    }

    #[tokio::test]
    async fn test_render_literal_no_interpolation_wrong_depth() {
        let ctx = test_ctx().await;
        ctx.bind_string("name", "test").unwrap();
        // Two ^ but only one * - no interpolation
        let result = render(&ctx, "{$^^ slot *name $}").await.unwrap();
        check!(result == "{$^ slot *name $}");
    }

    #[tokio::test]
    async fn test_render_literal_double_depth_interpolation() {
        let ctx = test_ctx().await;
        ctx.bind_string("name", "test").unwrap();
        // Two ^ and two * - interpolation happens
        let result = render(&ctx, "{$^^ slot **name $}").await.unwrap();
        check!(result == "{$^ slot test $}");
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
    async fn test_render_editing_mode_each_iterates() {
        let ctx = test_ctx_editing().await;
        ctx.bind(
            "items",
            Value::List(vec![Value::from("a"), Value::from("b"), Value::from("c")]),
        )
        .unwrap();
        let result = render(&ctx, "{$ each item in items $}[{$= item $}]{$ /each $}")
            .await
            .unwrap();
        // In editing mode, now iterates over all values (same as presenting mode)
        check!(result == "[a][b][c]");
    }

    #[tokio::test]
    async fn test_render_times_literal() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$ times 3 $}X{$ /times $}").await.unwrap();
        check!(result == "XXX");
    }

    #[tokio::test]
    async fn test_render_times_variable() {
        let ctx = test_ctx().await;
        ctx.bind_string("count", "4").unwrap();
        let result = render(&ctx, "{$ times count $}Y{$ /times $}")
            .await
            .unwrap();
        check!(result == "YYYY");
    }

    #[tokio::test]
    async fn test_render_times_zero() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$ times 0 $}X{$ /times $}").await.unwrap();
        check!(result == "");
    }

    #[tokio::test]
    async fn test_render_times_unbound() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$ times missing $}X{$ /times $}")
            .await
            .unwrap();
        check!(result == "");
    }

    #[tokio::test]
    async fn test_render_times_iteration_variable() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$ times 3 $}[{$= iteration $}]{$ /times $}")
            .await
            .unwrap();
        check!(result == "[t_0][t_1][t_2]");
    }

    #[tokio::test]
    async fn test_render_each_iteration_variable() {
        let ctx = test_ctx().await;
        ctx.bind(
            "items",
            Value::List(vec![Value::from("a"), Value::from("b")]),
        )
        .unwrap();
        let result = render(
            &ctx,
            "{$ each item in items $}[{$= iteration $}]{$ /each $}",
        )
        .await
        .unwrap();
        check!(result == "[items_0][items_1]");
    }

    // ============ Expression literal tests ============

    #[tokio::test]
    async fn test_render_string_literal_double_quotes() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$= \"hello world\" $}").await.unwrap();
        check!(result == "hello world");
    }

    #[tokio::test]
    async fn test_render_string_literal_single_quotes() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$= 'hello world' $}").await.unwrap();
        check!(result == "hello world");
    }

    #[tokio::test]
    async fn test_render_integer_literal() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$= 42 $}").await.unwrap();
        check!(result == "42");
    }

    #[tokio::test]
    async fn test_render_negative_integer_literal() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$= -5 $}").await.unwrap();
        check!(result == "-5");
    }

    #[tokio::test]
    async fn test_if_string_literal_truthy() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$ if \"yes\" $}shown{$ /if $}")
            .await
            .unwrap();
        check!(result == "shown");
    }

    #[tokio::test]
    async fn test_if_string_literal_empty_falsy() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$ if \"\" $}shown{$ else $}hidden{$ /if $}")
            .await
            .unwrap();
        check!(result == "hidden");
    }

    #[tokio::test]
    async fn test_if_integer_literal_truthy() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$ if 1 $}shown{$ /if $}").await.unwrap();
        check!(result == "shown");
    }

    #[tokio::test]
    async fn test_if_integer_literal_zero_falsy() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$ if 0 $}shown{$ else $}hidden{$ /if $}")
            .await
            .unwrap();
        check!(result == "hidden");
    }

    #[test]
    fn test_parse_expression_string_double_quotes() {
        let expr = Expression::parse("\"hello\"").unwrap();
        check!(expr == Expression::StringLiteral("hello".to_string()));
    }

    #[test]
    fn test_parse_expression_string_single_quotes() {
        let expr = Expression::parse("'world'").unwrap();
        check!(expr == Expression::StringLiteral("world".to_string()));
    }

    #[test]
    fn test_parse_expression_integer() {
        let expr = Expression::parse("123").unwrap();
        check!(expr == Expression::IntegerLiteral(123));
    }

    #[test]
    fn test_parse_expression_negative_integer() {
        let expr = Expression::parse("-456").unwrap();
        check!(expr == Expression::IntegerLiteral(-456));
    }

    #[test]
    fn test_parse_expression_variable() {
        let expr = Expression::parse("my_var").unwrap();
        check!(expr == Expression::Variable("my_var".to_string()));
    }

    #[test]
    fn test_parse_expression_variable_with_whitespace() {
        let expr = Expression::parse("  my_var  ").unwrap();
        check!(expr == Expression::Variable("my_var".to_string()));
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

    // ============ Slot syntax tests ============

    #[test]
    fn test_parse_item_slot_name_only() {
        let item = parse_item(b"{$ slot header $}").unwrap();
        check!(
            item == Item::Slot {
                name: "header",
                component: None,
                params: vec![],
            }
        );
    }

    #[test]
    fn test_parse_item_slot_with_component() {
        let item = parse_item(b"{$ slot main component=01ARZ3NDEKTSV4RRFFQ69G5FAV $}").unwrap();
        check!(
            item == Item::Slot {
                name: "main",
                component: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
                params: vec![],
            }
        );
    }

    #[test]
    fn test_parse_item_slot_with_params() {
        let item = parse_item(b"{$ slot hero title=Welcome subtitle=Hello $}").unwrap();
        check!(
            item == Item::Slot {
                name: "hero",
                component: None,
                params: vec![("title", "Welcome"), ("subtitle", "Hello")],
            }
        );
    }

    #[test]
    fn test_parse_item_slot_with_component_and_params() {
        let item =
            parse_item(b"{$ slot content component=01ARZ3NDEKTSV4RRFFQ69G5FAV color=blue $}")
                .unwrap();
        check!(
            item == Item::Slot {
                name: "content",
                component: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
                params: vec![("color", "blue")],
            }
        );
    }

    #[test]
    fn test_parse_item_endslot() {
        let item = parse_item(b"{$ /slot $}").unwrap();
        check!(item == Item::EndSlot);
    }

    #[tokio::test]
    async fn test_render_empty_slot_presenting_mode() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "<div>{$ slot main $}{$ /slot $}</div>")
            .await
            .unwrap();
        // In presenting mode, empty slot renders nothing
        check!(result == "<div></div>");
    }

    #[tokio::test]
    async fn test_render_empty_slot_editing_mode() {
        let ctx = test_ctx_editing().await;
        let result = render(&ctx, "<div>{$ slot main $}{$ /slot $}</div>")
            .await
            .unwrap();
        // In editing mode, empty slot renders as droppable placeholder
        check!(result.contains("class=\"pb-slot pb-slot-empty\""));
        check!(result.contains("data-slot-name=\"main\""));
        check!(result.contains("data-pb-droppable=\"true\""));
        check!(result.contains("Drop component here (main)"));
    }

    #[tokio::test]
    async fn test_render_filled_slot_presenting_mode() {
        let ctx = test_ctx().await;
        let result = render(
            &ctx,
            "<div>{$ slot main $}<p>Content here</p>{$ /slot $}</div>",
        )
        .await
        .unwrap();
        // In presenting mode, just renders content directly
        check!(result == "<div><p>Content here</p></div>");
    }

    #[tokio::test]
    async fn test_render_filled_slot_editing_mode() {
        let ctx = test_ctx_editing().await;
        let result = render(
            &ctx,
            "<div>{$ slot main $}<p>Content here</p>{$ /slot $}</div>",
        )
        .await
        .unwrap();
        // In editing mode, renders with wrapper
        check!(result.contains("class=\"pb-slot pb-slot-filled\""));
        check!(result.contains("data-slot-name=\"main\""));
        check!(result.contains("<p>Content here</p>"));
        check!(!result.contains("Drop component here"));
    }

    #[tokio::test]
    async fn test_render_slot_with_component_id_editing_mode() {
        let ctx = test_ctx_editing().await;
        let result = render(
            &ctx,
            "{$ slot main component=01ARZ3NDEKTSV4RRFFQ69G5FAV $}<p>Test</p>{$ /slot $}",
        )
        .await
        .unwrap();
        check!(result.contains("data-component-id=\"01ARZ3NDEKTSV4RRFFQ69G5FAV\""));
    }

    #[tokio::test]
    async fn test_render_slot_with_params_presenting_mode() {
        let ctx = test_ctx().await;
        let result = render(
            &ctx,
            "{$ slot hero title=Welcome $}<h1>{$= title $}</h1>{$ /slot $}",
        )
        .await
        .unwrap();
        // Params should be bound and rendered
        check!(result == "<h1>Welcome</h1>");
    }

    #[tokio::test]
    async fn test_render_slot_with_params_editing_mode() {
        let ctx = test_ctx_editing().await;
        let result = render(
            &ctx,
            "{$ slot hero title=Welcome $}<h1>{$= title $}</h1>{$ /slot $}",
        )
        .await
        .unwrap();
        // Params should be bound even in editing mode
        check!(result.contains("<h1>Welcome</h1>"));
        check!(result.contains("data-slot-name=\"hero\""));
    }

    #[tokio::test]
    async fn test_render_nested_slots_presenting_mode() {
        let ctx = test_ctx().await;
        let result = render(
            &ctx,
            "{$ slot outer $}<div>{$ slot inner $}<p>Nested</p>{$ /slot $}</div>{$ /slot $}",
        )
        .await
        .unwrap();
        check!(result == "<div><p>Nested</p></div>");
    }

    #[tokio::test]
    async fn test_render_nested_slots_editing_mode() {
        let ctx = test_ctx_editing().await;
        let result = render(
            &ctx,
            "{$ slot outer $}<div>{$ slot inner $}<p>Nested</p>{$ /slot $}</div>{$ /slot $}",
        )
        .await
        .unwrap();
        // Both slots should have wrappers
        check!(result.contains("data-slot-name=\"outer\""));
        check!(result.contains("data-slot-name=\"inner\""));
        check!(result.contains("<p>Nested</p>"));
    }

    #[tokio::test]
    async fn test_slot_params_dont_leak_to_outer_context() {
        let ctx = test_ctx().await;
        ctx.bind_string("title", "Outer").unwrap();
        let result = render(
            &ctx,
            "{$= title $} {$ slot hero title=Inner $}{$= title $}{$ /slot $} {$= title $}",
        )
        .await
        .unwrap();
        // Inner title should not affect outer context
        check!(result == "Outer Inner Outer");
    }

    #[tokio::test]
    async fn test_slot_with_if_inside() {
        let ctx = test_ctx().await;
        ctx.bind_string("show", "1").unwrap();
        let result = render(
            &ctx,
            "{$ slot main $}{$ if show $}Visible{$ /if $}{$ /slot $}",
        )
        .await
        .unwrap();
        check!(result == "Visible");
    }

    #[tokio::test]
    async fn test_slot_with_each_inside() {
        let ctx = test_ctx().await;
        ctx.bind(
            "items",
            Value::List(vec![Value::from("A"), Value::from("B")]),
        )
        .unwrap();
        let result = render(
            &ctx,
            "{$ slot main $}{$ each item in items $}[{$= item $}]{$ /each $}{$ /slot $}",
        )
        .await
        .unwrap();
        check!(result == "[A][B]");
    }

    #[test]
    fn test_unclosed_slot_error() {
        let result = CompiledTemplate::compile("{$ slot main $}content without end");
        check!(result.is_err());
        check!(result.unwrap_err().to_string().contains("Unclosed slot"));
    }

    #[test]
    fn test_unexpected_endslot_error() {
        let result = CompiledTemplate::compile("{$ /slot $}");
        check!(result.is_err());
        check!(result.unwrap_err().to_string().contains("Unexpected"));
    }

    #[test]
    fn test_slot_requires_name() {
        let result = parse_item(b"{$ slot $}");
        check!(result.is_err());
        check!(result.unwrap_err().to_string().contains("requires a name"));

        // Also with trailing whitespace
        let result = parse_item(b"{$ slot  $}");
        check!(result.is_err());
        check!(result.unwrap_err().to_string().contains("requires a name"));
    }

    // ============ Param syntax tests ============

    #[test]
    fn test_parse_item_param_name_only() {
        let item = parse_item(b"{$ param title $}").unwrap();
        check!(
            item == Item::Param {
                name: "title",
                label: None,
                default: None,
                description: None,
                input_type: ParamInputType::Text,
            }
        );
    }

    #[test]
    fn test_parse_item_param_with_label() {
        let item = parse_item(b"{$ param title label=\"Page Title\" $}").unwrap();
        check!(
            item == Item::Param {
                name: "title",
                label: Some("Page Title"),
                default: None,
                description: None,
                input_type: ParamInputType::Text,
            }
        );
    }

    #[test]
    fn test_parse_item_param_with_string_default_double_quotes() {
        let item = parse_item(b"{$ param title default=\"Welcome\" $}").unwrap();
        check!(
            item == Item::Param {
                name: "title",
                label: None,
                default: Some(Expression::StringLiteral("Welcome".to_string())),
                description: None,
                input_type: ParamInputType::Text,
            }
        );
    }

    #[test]
    fn test_parse_item_param_with_string_default_single_quotes() {
        let item = parse_item(b"{$ param title default='Welcome' $}").unwrap();
        check!(
            item == Item::Param {
                name: "title",
                label: None,
                default: Some(Expression::StringLiteral("Welcome".to_string())),
                description: None,
                input_type: ParamInputType::Text,
            }
        );
    }

    #[test]
    fn test_parse_item_param_with_integer_default() {
        let item = parse_item(b"{$ param count default=42 $}").unwrap();
        check!(
            item == Item::Param {
                name: "count",
                label: None,
                default: Some(Expression::IntegerLiteral(42)),
                description: None,
                input_type: ParamInputType::Text,
            }
        );
    }

    #[test]
    fn test_parse_item_param_with_negative_integer_default() {
        let item = parse_item(b"{$ param offset default=-10 $}").unwrap();
        check!(
            item == Item::Param {
                name: "offset",
                label: None,
                default: Some(Expression::IntegerLiteral(-10)),
                description: None,
                input_type: ParamInputType::Text,
            }
        );
    }

    #[test]
    fn test_parse_item_param_unquoted_non_integer_error() {
        let result = parse_item(b"{$ param title default=some_var $}");
        check!(result.is_err());
        check!(
            result
                .unwrap_err()
                .to_string()
                .contains("must be an integer")
        );
    }

    #[test]
    fn test_parse_item_param_all_attributes() {
        let item = parse_item(
            b"{$ param hero_title label=\"Hero Title\" default=\"Welcome to our site\" description=\"The main heading\" $}"
        ).unwrap();
        check!(
            item == Item::Param {
                name: "hero_title",
                label: Some("Hero Title"),
                default: Some(Expression::StringLiteral("Welcome to our site".to_string())),
                description: Some("The main heading"),
                input_type: ParamInputType::Text,
            }
        );
    }

    #[test]
    fn test_parse_item_param_with_textarea_type() {
        let item = parse_item(b"{$ param content type=\"textarea\" $}").unwrap();
        check!(
            item == Item::Param {
                name: "content",
                label: None,
                default: None,
                description: None,
                input_type: ParamInputType::Textarea,
            }
        );
    }

    #[test]
    fn test_parse_item_param_requires_name() {
        let result = parse_item(b"{$ param $}");
        check!(result.is_err());
        check!(result.unwrap_err().to_string().contains("requires a name"));
    }

    #[test]
    fn test_extract_params_basic() {
        let template = CompiledTemplate::compile(
            r#"{$ param title label="Title" default="Hello" $}
{$ param subtitle default='World' $}
<h1>{$= title $}</h1>"#,
        )
        .unwrap();

        let params = template.extract_params();
        check!(params.len() == 2);
        check!(params[0].name == "title");
        check!(params[0].label == "Title");
        check!(params[0].default == Some(Value::String("Hello".to_string())));
        check!(params[0].input_type == ParamInputType::Text);
        check!(params[1].name == "subtitle");
        check!(params[1].label == "Subtitle"); // Auto-generated
        check!(params[1].default == Some(Value::String("World".to_string())));
        check!(params[1].input_type == ParamInputType::Text);
    }

    #[test]
    fn test_extract_params_with_textarea() {
        let template = CompiledTemplate::compile(
            r#"{$ param body label="Body" type="textarea" $}
<div>{$= body $}</div>"#,
        )
        .unwrap();

        let params = template.extract_params();
        check!(params.len() == 1);
        check!(params[0].name == "body");
        check!(params[0].input_type == ParamInputType::Textarea);
    }

    #[test]
    fn test_extract_params_stops_at_content() {
        let template = CompiledTemplate::compile(
            r#"{$ param title $}
<h1>Content</h1>
{$ param ignored $}"#,
        )
        .unwrap();

        let params = template.extract_params();
        // Should only extract params before first non-whitespace content
        check!(params.len() == 1);
        check!(params[0].name == "title");
    }

    #[test]
    fn test_extract_params_integer_default() {
        let template = CompiledTemplate::compile(
            r#"{$ param count default=5 $}
{$= count $}"#,
        )
        .unwrap();

        let params = template.extract_params();
        check!(params.len() == 1);
        check!(params[0].name == "count");
        check!(params[0].default == Some(Value::Integer(5)));
    }

    #[test]
    fn test_extract_params_empty() {
        let template = CompiledTemplate::compile("<p>No params</p>").unwrap();
        let params = template.extract_params();
        check!(params.is_empty());
    }

    #[tokio::test]
    async fn test_render_param_produces_nothing() {
        let ctx = test_ctx().await;
        let result = render(&ctx, "{$ param title default=\"Test\" $}Content")
            .await
            .unwrap();
        // Param declarations should render nothing
        check!(result == "Content");
    }
}
