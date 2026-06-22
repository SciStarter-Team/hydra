//! Source manipulation functions for page builder
//!
//! These functions manipulate the template source of pages to insert, remove,
//! and update components in slots.

use anyhow::{Error, anyhow, bail};
use ulid::Ulid;

use crate::builder::{
    context::{Context, ContextMode},
    render::render,
};
use crate::peering::LoadStorePeered;

use super::PageBuilderComponent;

/// Information about a slot found in source
#[derive(Debug, Clone)]
pub struct SlotInfo {
    pub name: String,
    /// Full hierarchical path (e.g., "main/t_3/t_0" for nested slots)
    pub path: String,
    pub component_id: Option<Ulid>,
    pub params: Vec<(String, String)>,
    pub is_empty: bool,
    /// Start position of the slot tag (including {$)
    pub start: usize,
    /// End position of the /slot tag (including $})
    pub end: usize,
    /// Start position of the slot body content
    pub body_start: usize,
    /// End position of the slot body content
    pub body_end: usize,
}

/// Extract all slots from the source (including nested slots)
pub fn extract_slots(source: &str) -> Result<Vec<SlotInfo>, Error> {
    let mut slots = Vec::new();
    let bytes = source.as_bytes();
    let mut pos = 0;

    // Stack of (slot_name, slot_end_position) for building paths
    let mut parent_stack: Vec<(String, usize)> = Vec::new();

    while let Some(slot_start) = find_slot_start(bytes, pos) {
        // Pop any parents we've moved past
        while let Some((_, parent_end)) = parent_stack.last() {
            if slot_start >= *parent_end {
                parent_stack.pop();
            } else {
                break;
            }
        }

        let tag_end = find_tag_end(bytes, slot_start)
            .ok_or_else(|| anyhow!("Unclosed slot tag at position {}", slot_start))?;

        // Parse the slot tag to get name, component, and params
        let tag_content = &source[slot_start + 2..tag_end - 2]; // Strip {$ and $}
        let tag_content = tag_content.trim();

        if let Some(rest) = tag_content.strip_prefix("slot ") {
            let (name, component_id, params) = parse_slot_attributes(rest.trim())?;

            // Find the matching /slot
            let (body_end, slot_end) = find_slot_end(bytes, tag_end, &name)?;

            // Build the full path (using . as separator to avoid URL encoding issues)
            let path = if parent_stack.is_empty() {
                name.clone()
            } else {
                let parent_path: String = parent_stack
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                format!("{}.{}", parent_path, name)
            };

            let body_content = source[tag_end..body_end].trim();
            let is_empty = body_content.is_empty();

            slots.push(SlotInfo {
                name: name.clone(),
                path,
                component_id,
                params,
                is_empty,
                start: slot_start,
                end: slot_end,
                body_start: tag_end,
                body_end,
            });

            // Push this slot onto the stack for nested slots
            parent_stack.push((name, slot_end));

            // Continue from right after the opening tag to find nested slots
            pos = tag_end;
        } else {
            pos = tag_end;
        }
    }

    Ok(slots)
}

/// Insert a component into a slot, replacing the slot's content with the
/// component rendered in place.
///
/// Thin adapter over [`compose_component_into_slot`] for the common
/// case (replace, no extra slot content). The `slot_path` should be the
/// full hierarchical path (e.g., "main.t_3.t_0").
pub async fn insert_component_into_slot(
    page_source: &str,
    slot_path: &str,
    component: &PageBuilderComponent,
    params: &[(String, String)],
    db: &'static sqlx::SqlitePool,
) -> Result<String, Error> {
    compose_component_into_slot(page_source, slot_path, component, params, db).await
}

/// Place a component into a slot, rendering it in place (render-on-placement)
/// and recording `component=` metadata and params on the slot tag, replacing
/// whatever occupied the slot.
///
/// Slots authored with the `{$^ slot $}` escape survive the render as real,
/// still-empty page slots, so a placed container's slots remain available to
/// place further components into.
pub async fn compose_component_into_slot(
    page_source: &str,
    slot_path: &str,
    component: &PageBuilderComponent,
    params: &[(String, String)],
    db: &'static sqlx::SqlitePool,
) -> Result<String, Error> {
    // Render the component in place with its params bound. `{$^ slot $}`
    // slots survive this render as real page slots for later composition.
    let ctx = Context::new(db, &None, ContextMode::Presenting)?;
    for (key, value) in params {
        ctx.bind_string(key, value)?;
    }
    let body = render(&ctx, &component.source).await?;

    let slots = extract_slots(page_source)?;
    let target = slots
        .iter()
        .find(|s| s.path == slot_path)
        .ok_or_else(|| slot_not_found(slot_path, &slots))?;

    let new_tag = build_slot_tag(&target.name, Some(&component.component_id), params);
    let mut result = String::with_capacity(page_source.len() + body.len());
    result.push_str(&page_source[..target.start]);
    result.push_str(&new_tag);
    result.push_str(&body);
    result.push_str("{$ /slot $}");
    result.push_str(&page_source[target.end..]);
    Ok(result)
}

/// Remove a component from a slot, leaving it empty
///
/// The `slot_path` should be the full hierarchical path (e.g., "main/t_3/t_0").
pub fn clear_slot(page_source: &str, slot_path: &str) -> Result<String, Error> {
    let slots = extract_slots(page_source)?;
    let slot = slots
        .iter()
        .find(|s| s.path == slot_path)
        .ok_or_else(|| anyhow!("Slot '{}' not found", slot_path))?;

    // Build an empty slot tag (no component, no params) - use slot name, not path
    let new_tag = build_slot_tag(&slot.name, None, &[]);

    // Reconstruct the source with empty slot
    let mut result = String::with_capacity(page_source.len());
    result.push_str(&page_source[..slot.start]);
    result.push_str(&new_tag);
    result.push_str("{$ /slot $}");
    result.push_str(&page_source[slot.end..]);

    Ok(result)
}

/// Update parameters on a slot's component and re-render
///
/// The `slot_path` should be the full hierarchical path (e.g., "main.t_3.t_0").
///
/// This function preserves nested component instances: any slots that exist both
/// before and after the re-render will retain their component assignments and content.
pub async fn update_slot_params(
    page_source: &str,
    slot_path: &str,
    component: &PageBuilderComponent,
    params: &[(String, String)],
    db: &'static sqlx::SqlitePool,
) -> Result<String, Error> {
    // Find the current slot
    let slots = extract_slots(page_source)?;
    let slot = slots
        .iter()
        .find(|s| s.path == slot_path)
        .ok_or_else(|| anyhow!("Slot '{}' not found", slot_path))?;

    // Extract nested slots that have components (these need to be preserved)
    // Their paths start with "slot_path."
    let prefix = format!("{}.", slot_path);
    let nested_slots: Vec<_> = slots
        .iter()
        .filter(|s| s.path.starts_with(&prefix) && s.component_id.is_some())
        .collect();

    // Store the nested slot content (relative path -> full slot source including tags)
    let mut nested_content: Vec<(String, String)> = Vec::new();
    for nested in &nested_slots {
        // Get the relative path (strip the parent path prefix)
        let relative_path = nested.path.strip_prefix(&prefix).unwrap();
        // Extract the full slot source including tags
        let slot_source = &page_source[nested.start..nested.end];
        nested_content.push((relative_path.to_string(), slot_source.to_string()));
    }

    // Create context with params as bindings
    let ctx = Context::new(db, &None, ContextMode::Presenting)?;
    for (key, value) in params {
        ctx.bind_string(key, value)?;
    }

    // Render the component source fresh
    let mut rendered = render(&ctx, &component.source).await?;

    // Restore nested slot content
    // We need to process in reverse order of path depth to handle nested-within-nested correctly
    let mut sorted_nested: Vec<_> = nested_content.iter().collect();
    sorted_nested.sort_by(|a, b| {
        let depth_a = a.0.matches('.').count();
        let depth_b = b.0.matches('.').count();
        depth_b.cmp(&depth_a) // Deepest first
    });

    for (relative_path, old_slot_source) in sorted_nested {
        // Find this slot in the fresh render
        let fresh_slots = extract_slots(&rendered)?;
        if let Some(fresh_slot) = fresh_slots.iter().find(|s| s.path == *relative_path) {
            // Replace the empty/default slot with the preserved content
            let mut new_rendered = String::with_capacity(rendered.len() + old_slot_source.len());
            new_rendered.push_str(&rendered[..fresh_slot.start]);
            new_rendered.push_str(old_slot_source);
            new_rendered.push_str(&rendered[fresh_slot.end..]);
            rendered = new_rendered;
        }
    }

    // Build the new slot tag with metadata
    let new_tag = build_slot_tag(&slot.name, Some(&component.component_id), params);

    // Reconstruct the source
    let mut result = String::with_capacity(page_source.len() + rendered.len());
    result.push_str(&page_source[..slot.start]);
    result.push_str(&new_tag);
    result.push_str(&rendered);
    result.push_str("{$ /slot $}");
    result.push_str(&page_source[slot.end..]);

    Ok(result)
}

/// Move slot content from source to target, preserving all nested content
///
/// This transplants the entire slot body (including nested slots) from source to target.
/// The source slot is cleared after the move.
pub fn move_slot_content(
    page_source: &str,
    source_path: &str,
    target_path: &str,
) -> Result<String, Error> {
    let slots = extract_slots(page_source)?;

    let source = slots
        .iter()
        .find(|s| s.path == source_path)
        .ok_or_else(|| anyhow!("Source slot '{}' not found", source_path))?;

    let target = slots
        .iter()
        .find(|s| s.path == target_path)
        .ok_or_else(|| anyhow!("Target slot '{}' not found", target_path))?;

    // Extract the source slot's opening tag and body (but not its name - we'll use target's name)
    let source_body = &page_source[source.body_start..source.body_end];

    // Build new target slot tag with source's component and params
    let new_tag = build_slot_tag(&target.name, source.component_id.as_ref(), &source.params);

    // Build empty source slot tag
    let empty_source_tag = build_slot_tag(&source.name, None, &[]);

    // We need to handle the order of modifications carefully
    // If source comes before target, we modify target first (higher positions)
    // If target comes before source, we modify source first
    let mut result = page_source.to_string();

    if source.start < target.start {
        // Source is before target - modify target first (since its positions are still valid)
        // Then modify source
        let mut intermediate = String::with_capacity(result.len() + source_body.len());
        intermediate.push_str(&result[..target.start]);
        intermediate.push_str(&new_tag);
        intermediate.push_str(source_body);
        intermediate.push_str("{$ /slot $}");
        intermediate.push_str(&result[target.end..]);
        // Re-extract slots since positions changed
        let slots = extract_slots(&intermediate)?;
        let source = slots
            .iter()
            .find(|s| s.path == source_path)
            .ok_or_else(|| {
                anyhow!(
                    "Source slot '{}' not found after first modification",
                    source_path
                )
            })?;
        // Clear source slot
        let mut final_result = String::with_capacity(intermediate.len());
        final_result.push_str(&intermediate[..source.start]);
        final_result.push_str(&empty_source_tag);
        final_result.push_str("{$ /slot $}");
        final_result.push_str(&intermediate[source.end..]);
        result = final_result;
    } else {
        // Target is before source - modify source first
        // Clear source slot
        let mut intermediate = String::with_capacity(result.len());
        intermediate.push_str(&result[..source.start]);
        intermediate.push_str(&empty_source_tag);
        intermediate.push_str("{$ /slot $}");
        intermediate.push_str(&result[source.end..]);
        // Re-extract slots since positions changed
        let slots = extract_slots(&intermediate)?;
        let target = slots
            .iter()
            .find(|s| s.path == target_path)
            .ok_or_else(|| {
                anyhow!(
                    "Target slot '{}' not found after first modification",
                    target_path
                )
            })?;
        // Replace target with source content
        let mut final_result = String::with_capacity(intermediate.len() + source_body.len());
        final_result.push_str(&intermediate[..target.start]);
        final_result.push_str(&new_tag);
        final_result.push_str(source_body);
        final_result.push_str("{$ /slot $}");
        final_result.push_str(&intermediate[target.end..]);
        result = final_result;
    }

    Ok(result)
}

/// Build a helpful "slot not found" error listing the available paths.
fn slot_not_found(path: &str, slots: &[SlotInfo]) -> Error {
    let available: Vec<&str> = slots.iter().map(|s| s.path.as_str()).collect();
    anyhow!(
        "Slot '{}' not found. Available slots: {}",
        path,
        if available.is_empty() {
            "(none)".to_string()
        } else {
            available.join(", ")
        }
    )
}

/// The direct child slots of `parent_path` (one path segment deeper).
fn direct_children<'a>(slots: &'a [SlotInfo], parent_path: &str) -> Vec<&'a SlotInfo> {
    let prefix = format!("{parent_path}.");
    slots
        .iter()
        .filter(|s| {
            s.path
                .strip_prefix(&prefix)
                .is_some_and(|rest| !rest.contains('.'))
        })
        .collect()
}

/// A component captured out of an occupied slot, for restoration.
struct CapturedSlot {
    name: String,
    component_id: Option<Ulid>,
    params: Vec<(String, String)>,
    body: String,
}

/// Capture the component placed in each occupied slot of the container at
/// `slot_path` (single-occupancy: one component per slot).
fn capture_container_slots(
    page_source: &str,
    slots: &[SlotInfo],
    slot_path: &str,
) -> Result<Vec<CapturedSlot>, Error> {
    let mut captured = Vec::new();
    for inner in direct_children(slots, slot_path) {
        if inner.is_empty {
            continue;
        }
        let (component_id, params, body) = extract_slot_source(page_source, &inner.path)?;
        captured.push(CapturedSlot {
            name: inner.name.clone(),
            component_id,
            params,
            body,
        });
    }
    Ok(captured)
}

/// Swap the component occupying `slot_path` for `component`, moving the
/// component placed in each of its slots across per `slot_map` (old slot
/// name → new slot name), e.g. `{sidebar→col_1, main→col_2}` for
/// Two Column → Three Column.
///
/// With an empty `slot_map`, a single occupied slot is auto-mapped to the
/// new component's single slot when unambiguous; otherwise an error asks
/// for an explicit map, so nothing is silently dropped. `params` configure
/// the new component.
pub async fn replace_component(
    page_source: &str,
    slot_path: &str,
    component: &PageBuilderComponent,
    params: &[(String, String)],
    slot_map: &[(String, String)],
    db: &'static sqlx::SqlitePool,
) -> Result<String, Error> {
    let slots = extract_slots(page_source)?;
    if !slots.iter().any(|s| s.path == slot_path) {
        return Err(slot_not_found(slot_path, &slots));
    }

    let captured = capture_container_slots(page_source, &slots, slot_path)?;

    // Render the new component into the slot (this clears the old content).
    let replaced =
        compose_component_into_slot(page_source, slot_path, component, params, db).await?;

    if captured.is_empty() {
        return Ok(replaced);
    }

    // Resolve where each captured slot lands on the new component.
    let new_slots = extract_slots(&replaced)?;
    let new_names: Vec<String> = direct_children(&new_slots, slot_path)
        .iter()
        .map(|s| s.name.clone())
        .collect();

    let mapping: Vec<(String, String)> = if slot_map.is_empty() {
        if captured.len() == 1 && new_names.len() == 1 {
            vec![(captured[0].name.clone(), new_names[0].clone())]
        } else {
            bail!(
                "ambiguous transplant: occupied slot(s) [{}] and the new component has slot(s) [{}]. Provide slot_map to choose where each goes.",
                captured
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                if new_names.is_empty() {
                    "none".to_string()
                } else {
                    new_names.join(", ")
                }
            );
        }
    } else {
        for c in &captured {
            if !slot_map.iter().any(|(o, _)| o == &c.name) {
                bail!(
                    "slot '{}' is occupied but not in slot_map; its content would be lost. Map it to a slot on the new component.",
                    c.name
                );
            }
        }
        slot_map.to_vec()
    };

    // Restore each captured slot's component into its mapped new slot.
    let mut current = replaced;
    for (old_name, new_name) in &mapping {
        let Some(c) = captured.iter().find(|c| &c.name == old_name) else {
            continue; // mapping referenced an empty or absent old slot
        };
        if !new_names.iter().any(|n| n == new_name) {
            bail!(
                "the new component has no slot '{}'; available: {}",
                new_name,
                if new_names.is_empty() {
                    "none".to_string()
                } else {
                    new_names.join(", ")
                }
            );
        }
        let target_path = format!("{slot_path}.{new_name}");
        current = insert_raw_source(
            &current,
            &target_path,
            c.component_id.as_ref(),
            &c.params,
            &c.body,
        )?;
    }

    Ok(current)
}

/// Change the `count` of a count-parameterized container at `slot_path`,
/// regenerating its slots while preserving the components already placed
/// in them. Errors if shrinking would drop an occupied slot.
pub async fn resize_container(
    page_source: &str,
    slot_path: &str,
    new_count: usize,
    db: &'static sqlx::SqlitePool,
) -> Result<String, Error> {
    let slots = extract_slots(page_source)?;
    let container = slots
        .iter()
        .find(|s| s.path == slot_path)
        .ok_or_else(|| slot_not_found(slot_path, &slots))?;
    let component_id = container
        .component_id
        .ok_or_else(|| anyhow!("slot '{}' holds no component to resize", slot_path))?;

    // Identity map over the container's current slot names — preserve each
    // occupied slot into the same-named slot of the regenerated container.
    let identity: Vec<(String, String)> = direct_children(&slots, slot_path)
        .iter()
        .map(|s| (s.name.clone(), s.name.clone()))
        .collect();

    // Reuse the container's params, with `count` overridden.
    let mut params: Vec<(String, String)> = container.params.clone();
    params.retain(|(k, _)| k != "count");
    params.push(("count".to_string(), new_count.to_string()));

    let component = PageBuilderComponent::load(component_id).await?;
    replace_component(page_source, slot_path, &component, &params, &identity, db).await
}

/// Extract the source of a slot (component info + body) for staging
///
/// Returns a tuple of (component_id, params, body) where body is the raw content.
/// This can be used to temporarily store slot content in the staging panel.
pub fn extract_slot_source(
    page_source: &str,
    slot_path: &str,
) -> Result<(Option<Ulid>, Vec<(String, String)>, String), Error> {
    let slots = extract_slots(page_source)?;

    let slot = slots
        .iter()
        .find(|s| s.path == slot_path)
        .ok_or_else(|| anyhow!("Slot '{}' not found", slot_path))?;

    let body = page_source[slot.body_start..slot.body_end].to_string();

    Ok((slot.component_id, slot.params.clone(), body))
}

/// Insert raw source into a slot
///
/// This replaces the slot's content with the provided raw source, optionally
/// setting component ID and params. Used to restore content from staging.
pub fn insert_raw_source(
    page_source: &str,
    slot_path: &str,
    component_id: Option<&Ulid>,
    params: &[(String, String)],
    body: &str,
) -> Result<String, Error> {
    let slots = extract_slots(page_source)?;

    let slot = slots
        .iter()
        .find(|s| s.path == slot_path)
        .ok_or_else(|| anyhow!("Slot '{}' not found", slot_path))?;

    // Build new slot tag
    let new_tag = build_slot_tag(&slot.name, component_id, params);

    // Reconstruct the source
    let mut result = String::with_capacity(page_source.len() + body.len());
    result.push_str(&page_source[..slot.start]);
    result.push_str(&new_tag);
    result.push_str(body);
    result.push_str("{$ /slot $}");
    result.push_str(&page_source[slot.end..]);

    Ok(result)
}

/// Copy slot content from source to target, preserving all nested content
///
/// This copies the entire slot body (including nested slots) from source to target.
/// The source slot is left unchanged.
pub fn copy_slot_content(
    page_source: &str,
    source_path: &str,
    target_path: &str,
) -> Result<String, Error> {
    let slots = extract_slots(page_source)?;

    let source = slots
        .iter()
        .find(|s| s.path == source_path)
        .ok_or_else(|| anyhow!("Source slot '{}' not found", source_path))?;

    let target = slots
        .iter()
        .find(|s| s.path == target_path)
        .ok_or_else(|| anyhow!("Target slot '{}' not found", target_path))?;

    // Extract the source slot's body
    let source_body = &page_source[source.body_start..source.body_end];

    // Build new target slot tag with source's component and params
    let new_tag = build_slot_tag(&target.name, source.component_id.as_ref(), &source.params);

    // Replace target slot with source content
    let mut result = String::with_capacity(page_source.len() + source_body.len());
    result.push_str(&page_source[..target.start]);
    result.push_str(&new_tag);
    result.push_str(source_body);
    result.push_str("{$ /slot $}");
    result.push_str(&page_source[target.end..]);

    Ok(result)
}

/// Build a slot tag string with optional component and params
fn build_slot_tag(name: &str, component_id: Option<&Ulid>, params: &[(String, String)]) -> String {
    let mut tag = format!("{{$ slot {}", name);

    if let Some(cid) = component_id {
        tag.push_str(&format!(" component={}", cid));
    }

    for (key, value) in params {
        // Escape special characters in values if needed
        if value.contains(' ') || value.contains('"') || value.contains('=') {
            tag.push_str(&format!(" {}=\"{}\"", key, escape_attr_value(value)));
        } else {
            tag.push_str(&format!(" {}={}", key, value));
        }
    }

    tag.push_str(" $}");
    tag
}

/// Escape attribute value for safe inclusion in slot tag
fn escape_attr_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Find the start of a {$ slot marker
fn find_slot_start(bytes: &[u8], from: usize) -> Option<usize> {
    let pattern = b"{$ slot ";
    let mut pos = from;

    while pos + pattern.len() <= bytes.len() {
        if &bytes[pos..pos + pattern.len()] == pattern {
            return Some(pos);
        }
        // Also check for {$slot (no space after $)
        if pos + 7 <= bytes.len() && &bytes[pos..pos + 7] == b"{$slot " {
            return Some(pos);
        }
        pos += 1;
    }

    None
}

/// Find the end of a tag ($})
fn find_tag_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut pos = from;

    while pos + 2 <= bytes.len() {
        if bytes[pos] == b'$' && bytes[pos + 1] == b'}' {
            return Some(pos + 2);
        }
        pos += 1;
    }

    None
}

/// Find the matching /slot end tag
fn find_slot_end(bytes: &[u8], from: usize, _name: &str) -> Result<(usize, usize), Error> {
    let mut depth = 0;
    let mut pos = from;

    while pos < bytes.len() {
        // Look for {$ markers
        if pos + 2 < bytes.len() && bytes[pos] == b'{' && bytes[pos + 1] == b'$' {
            // Find the end of this tag
            if let Some(tag_end) = find_tag_end(bytes, pos) {
                let tag_content = std::str::from_utf8(&bytes[pos + 2..tag_end - 2])
                    .map_err(|e| anyhow!("Invalid UTF-8 in tag: {}", e))?
                    .trim();

                if tag_content.starts_with("slot ") {
                    depth += 1;
                } else if tag_content == "/slot" {
                    if depth == 0 {
                        return Ok((pos, tag_end));
                    }
                    depth -= 1;
                }

                pos = tag_end;
                continue;
            }
        }

        pos += 1;
    }

    Err(anyhow!("Unclosed slot block"))
}

/// Parse slot attributes: name [component=ID] [key=value...]
/// Supports quoted values: key="value with spaces" or key='value with spaces'
fn parse_slot_attributes(
    rest: &str,
) -> Result<(String, Option<Ulid>, Vec<(String, String)>), Error> {
    use crate::builder::parse::{parse_attributes, unescape_value};

    let mut parser = parse_attributes(rest);
    let name = parser
        .name()
        .ok_or_else(|| anyhow!("slot requires a name"))?
        .to_string();

    let mut component_id = None;
    let mut params = Vec::new();

    for attr in parser {
        let value = unescape_value(attr.value);
        if attr.key == "component" {
            component_id = Some(
                Ulid::from_string(&value).map_err(|e| anyhow!("Invalid component ULID: {}", e))?,
            );
        } else {
            params.push((attr.key.to_string(), value));
        }
    }

    Ok((name, component_id, params))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_slots_empty() {
        let source = "<div>Hello</div>";
        let slots = extract_slots(source).unwrap();
        assert!(slots.is_empty());
    }

    #[test]
    fn test_extract_slots_simple() {
        let source = "<div>{$ slot main $}{$ /slot $}</div>";
        let slots = extract_slots(source).unwrap();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].name, "main");
        assert!(slots[0].is_empty);
        assert!(slots[0].component_id.is_none());
    }

    #[test]
    fn test_extract_slots_with_content() {
        let source = "{$ slot header $}<h1>Title</h1>{$ /slot $}";
        let slots = extract_slots(source).unwrap();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].name, "header");
        assert!(!slots[0].is_empty);
    }

    #[test]
    fn test_extract_slots_with_component() {
        let source = "{$ slot main component=01ARZ3NDEKTSV4RRFFQ69G5FAV $}content{$ /slot $}";
        let slots = extract_slots(source).unwrap();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].name, "main");
        assert!(slots[0].component_id.is_some());
        assert_eq!(
            slots[0].component_id.unwrap().to_string(),
            "01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
    }

    #[test]
    fn test_extract_slots_with_params() {
        let source = "{$ slot hero title=Welcome color=blue $}content{$ /slot $}";
        let slots = extract_slots(source).unwrap();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].params.len(), 2);
        assert_eq!(
            slots[0].params[0],
            ("title".to_string(), "Welcome".to_string())
        );
        assert_eq!(
            slots[0].params[1],
            ("color".to_string(), "blue".to_string())
        );
    }

    #[test]
    fn test_extract_slots_with_quoted_params() {
        let source = r#"{$ slot hero text="This is a paragraph, yo." count=5 $}content{$ /slot $}"#;
        let slots = extract_slots(source).unwrap();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].params.len(), 2);
        assert_eq!(
            slots[0].params[0],
            ("text".to_string(), "This is a paragraph, yo.".to_string())
        );
        assert_eq!(slots[0].params[1], ("count".to_string(), "5".to_string()));
    }

    #[test]
    fn test_extract_slots_with_escaped_quotes() {
        let source = r#"{$ slot hero text="Hello \"World\"" $}content{$ /slot $}"#;
        let slots = extract_slots(source).unwrap();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].params.len(), 1);
        assert_eq!(
            slots[0].params[0],
            ("text".to_string(), "Hello \"World\"".to_string())
        );
    }

    #[test]
    fn test_extract_multiple_slots() {
        let source = "{$ slot header $}H{$ /slot $}<main>{$ slot content $}C{$ /slot $}</main>";
        let slots = extract_slots(source).unwrap();
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].name, "header");
        assert_eq!(slots[1].name, "content");
    }

    #[test]
    fn test_extract_nested_slots() {
        let source = "{$ slot outer $}<div>{$ slot inner $}I{$ /slot $}</div>{$ /slot $}";
        let slots = extract_slots(source).unwrap();
        // extract_slots now returns all slots including nested ones
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].name, "outer");
        assert_eq!(slots[0].path, "outer");
        assert_eq!(slots[1].name, "inner");
        assert_eq!(slots[1].path, "outer.inner");
    }

    #[test]
    fn test_extract_slots_with_duplicate_names() {
        // Simulates the case where a component creates slots with the same names at different nesting levels
        let source = "{$ slot main $}{$ slot t_0 $}{$ /slot $}{$ slot t_1 component=01ARZ3NDEKTSV4RRFFQ69G5FAV $}{$ slot t_0 $}{$ /slot $}{$ /slot $}{$ /slot $}";
        let slots = extract_slots(source).unwrap();
        assert_eq!(slots.len(), 4);
        assert_eq!(slots[0].path, "main");
        assert_eq!(slots[1].path, "main.t_0");
        assert_eq!(slots[2].path, "main.t_1");
        assert_eq!(slots[3].path, "main.t_1.t_0"); // This is the key - different path for same-named slot
    }

    #[test]
    fn test_build_slot_tag_simple() {
        let tag = build_slot_tag("main", None, &[]);
        assert_eq!(tag, "{$ slot main $}");
    }

    #[test]
    fn test_build_slot_tag_with_component() {
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let tag = build_slot_tag("main", Some(&id), &[]);
        assert_eq!(tag, "{$ slot main component=01ARZ3NDEKTSV4RRFFQ69G5FAV $}");
    }

    #[test]
    fn test_build_slot_tag_with_params() {
        let params = vec![
            ("title".to_string(), "Hello".to_string()),
            ("size".to_string(), "large".to_string()),
        ];
        let tag = build_slot_tag("hero", None, &params);
        assert_eq!(tag, "{$ slot hero title=Hello size=large $}");
    }

    #[test]
    fn test_clear_slot() {
        let source =
            "{$ slot main component=01ARZ3NDEKTSV4RRFFQ69G5FAV $}<p>Content</p>{$ /slot $}";
        let result = clear_slot(source, "main").unwrap();
        assert_eq!(result, "{$ slot main $}{$ /slot $}");
    }
}
