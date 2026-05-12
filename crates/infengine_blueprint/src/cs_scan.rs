//! Lightweight scanner for `[BlueprintCallable]` C# methods.
//!
//! The blueprint editor needs to know which methods a project's
//! game-side C# exposes so it can offer dedicated context-menu
//! entries (instead of a single "Call C# Method" with a free-text
//! method param). Until the CoreCLR-driven reflection manifest
//! pipeline (`docs/design/blueprint_csharp_callable.md`) lands, we
//! read the `.cs` source directly and extract the attributed
//! methods with a tiny hand-rolled parser.
//!
//! The parser is intentionally forgiving: it tracks the last seen
//! `class X` token and pairs each `[BlueprintCallable(...)]`
//! attribute with the next `public static T Name(...)` declaration.
//! Anything more sophisticated (overloads, generics, instance
//! methods) is rejected silently so the editor never blocks on a
//! parse error.

use std::fs;
use std::path::{Path, PathBuf};

/// One method discovered in a project's C# scripts that should
/// appear as a dedicated node in the blueprint editor.
#[derive(Debug, Clone)]
pub struct BlueprintCallableMethod {
    /// Source file path the method was found in. Relative to the
    /// scan root when the scanner is asked for relative paths.
    pub source_path: PathBuf,
    /// Class name the method lives in (e.g. `"Showcase"`).
    pub class: String,
    /// Method name (e.g. `"OnPickupCollected"`).
    pub method: String,
    /// Fully qualified `Class.Method` form. This is what gets baked
    /// into the `call_cs_method` node's `params["method"]`.
    pub full_name: String,
    /// Display label from the attribute's `DisplayName = "..."`,
    /// falling back to `Class.Method` when absent.
    pub display_name: String,
    /// Category from the attribute, falling back to `"Scripts"`.
    pub category: String,
    /// Optional tooltip text.
    pub tooltip: Option<String>,
    /// Return type token as written in source (e.g. `"void"`,
    /// `"float"`). Stored verbatim - pin-type mapping lives on the
    /// blueprint side once the full pipeline ships.
    pub return_type: String,
    /// Parameter list verbatim, e.g. `"float dt"`. Empty string
    /// when the method takes no parameters.
    pub parameters: String,
}

/// Walk `root` recursively, parse every `.cs` file, and return the
/// flat list of `[BlueprintCallable]` methods found. Hidden / build
/// directories (`bin`, `obj`) are skipped so the build output's
/// generated files don't leak in.
pub fn scan_directory(root: &Path) -> Vec<BlueprintCallableMethod> {
    let mut out = Vec::new();
    let _ = walk_dir(root, &mut out);
    out
}

fn walk_dir(dir: &Path, out: &mut Vec<BlueprintCallableMethod>) -> std::io::Result<()> {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip the .NET build caches.
            if let Some(name) = path.file_name().and_then(|s| s.to_str())
                && (name == "bin" || name == "obj" || name.starts_with('.'))
            {
                continue;
            }
            walk_dir(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("cs")
            && let Ok(text) = fs::read_to_string(&path)
        {
            let mut found = scan_text(&text);
            for m in &mut found {
                m.source_path = path.clone();
            }
            out.extend(found);
        }
    }
    Ok(())
}

/// Parse one .cs source string. Exposed so unit tests can hit the
/// parser without touching the filesystem.
pub fn scan_text(text: &str) -> Vec<BlueprintCallableMethod> {
    let mut out: Vec<BlueprintCallableMethod> = Vec::new();
    let mut current_class: Option<String> = None;
    let mut pending_attr: Option<AttrArgs> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();

        // Track the most recent `class X` so the next attributed
        // method gets the right qualifier.
        if let Some(rest) = line.strip_prefix("public ").or(Some(line))
            && let Some(idx) = rest.find("class ")
        {
            let after = &rest[idx + "class ".len()..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                current_class = Some(name);
            }
        }

        // Attribute line: `[BlueprintCallable]` or
        // `[BlueprintCallable(DisplayName = "...", Category = "...")]`.
        // The attribute and method body sometimes share a line (e.g.
        // `[BlueprintCallable] public static void Foo() {}`), so we
        // peek the same line for a signature before falling through.
        if line.starts_with("[BlueprintCallable") {
            let close_attr = line.find(']');
            let args = if line.starts_with("[BlueprintCallable(") {
                line.find('(')
                    .and_then(|start| {
                        // Use the rfind of `)` BEFORE the attribute's
                        // closing `]`, not the method's `)`.
                        let bracket = close_attr.unwrap_or(line.len());
                        line[..bracket].rfind(')').map(|end| &line[start + 1..end])
                    })
                    .unwrap_or("")
            } else {
                ""
            };
            let attr = parse_attr_args(args);
            // Inline form: signature lives on the same line after the
            // attribute. Try to parse it right away.
            if let Some(end) = close_attr
                && end + 1 < line.len()
                && let Some(sig) = parse_signature(line[end + 1..].trim())
            {
                let class = current_class.clone().unwrap_or_else(|| "Unknown".into());
                let full = format!("{class}.{}", sig.name);
                let display = attr.display_name.unwrap_or_else(|| full.clone());
                let category = attr.category.unwrap_or_else(|| "Scripts".into());
                out.push(BlueprintCallableMethod {
                    source_path: PathBuf::new(),
                    class,
                    method: sig.name,
                    full_name: full,
                    display_name: display,
                    category,
                    tooltip: attr.tooltip,
                    return_type: sig.return_type,
                    parameters: sig.parameters,
                });
                continue;
            }
            pending_attr = Some(attr);
            continue;
        }

        // Method signature: `public static T Name(args)` after a
        // pending attribute.
        if pending_attr.is_some()
            && let Some(sig) = parse_signature(line)
        {
            let attr = pending_attr.take().unwrap();
            let class = current_class.clone().unwrap_or_else(|| "Unknown".into());
            let full = format!("{class}.{}", sig.name);
            let display = attr.display_name.unwrap_or_else(|| full.clone());
            let category = attr.category.unwrap_or_else(|| "Scripts".into());
            out.push(BlueprintCallableMethod {
                source_path: PathBuf::new(),
                class,
                method: sig.name,
                full_name: full,
                display_name: display,
                category,
                tooltip: attr.tooltip,
                return_type: sig.return_type,
                parameters: sig.parameters,
            });
        }
    }

    out
}

#[derive(Debug, Default)]
struct AttrArgs {
    display_name: Option<String>,
    category: Option<String>,
    tooltip: Option<String>,
}

fn parse_attr_args(raw: &str) -> AttrArgs {
    let mut out = AttrArgs::default();
    // The attribute body uses named arguments separated by commas;
    // values are double-quoted strings. We scan for `Name = "..."`
    // chunks without trying to be a full C# parser.
    let mut chars = raw.chars().peekable();
    while chars.peek().is_some() {
        // Skip whitespace + commas.
        while matches!(chars.peek(), Some(c) if c.is_whitespace() || *c == ',') {
            chars.next();
        }
        // Read identifier.
        let mut name = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_alphanumeric() || c == '_' {
                name.push(c);
                chars.next();
            } else {
                break;
            }
        }
        if name.is_empty() {
            break;
        }
        // Skip "= " then read quoted string.
        while matches!(chars.peek(), Some(c) if c.is_whitespace() || *c == '=') {
            chars.next();
        }
        if chars.peek() != Some(&'"') {
            // Unsupported value type; bail and skip to next comma.
            while let Some(&c) = chars.peek() {
                if c == ',' {
                    break;
                }
                chars.next();
            }
            continue;
        }
        chars.next(); // opening quote
        let mut value = String::new();
        while let Some(&c) = chars.peek() {
            chars.next();
            if c == '"' {
                break;
            }
            if c == '\\'
                && let Some(&esc) = chars.peek()
            {
                value.push(esc);
                chars.next();
                continue;
            }
            value.push(c);
        }
        match name.as_str() {
            "DisplayName" => out.display_name = Some(value),
            "Category" => out.category = Some(value),
            "Tooltip" => out.tooltip = Some(value),
            _ => {}
        }
    }
    out
}

#[derive(Debug)]
struct Signature {
    return_type: String,
    name: String,
    parameters: String,
}

/// Recognise `public static <ret> <name>(<args>)` on a single line.
/// Anything fancier (generics, async, expression-bodied) is rejected
/// silently. Only the prefix matters - trailing `{`, `=>`, etc. are
/// ignored.
fn parse_signature(line: &str) -> Option<Signature> {
    let rest = line.strip_prefix("public static ")?;
    let open = rest.find('(')?;
    let head = &rest[..open];
    let mut tokens = head.split_whitespace();
    let return_type = tokens.next()?.to_string();
    let name = tokens.next()?.to_string();
    if tokens.next().is_some() {
        return None; // unexpected extra token; bail.
    }
    let close = rest.rfind(')')?;
    if close < open {
        return None;
    }
    let parameters = rest[open + 1..close].trim().to_string();
    Some(Signature {
        return_type,
        name,
        parameters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_method() {
        let src = r#"
namespace Demo;

public static class Showcase
{
    [BlueprintCallable(DisplayName = "Add Score", Category = "Score")]
    public static void OnPickupCollected()
    {
        // body
    }
}
        "#;
        let m = scan_text(src);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].class, "Showcase");
        assert_eq!(m[0].method, "OnPickupCollected");
        assert_eq!(m[0].full_name, "Showcase.OnPickupCollected");
        assert_eq!(m[0].display_name, "Add Score");
        assert_eq!(m[0].category, "Score");
        assert_eq!(m[0].return_type, "void");
    }

    #[test]
    fn parses_method_with_tooltip_and_param() {
        let src = r#"
public static class Stuff {
    [BlueprintCallable(Tooltip = "Returns running count.")]
    public static float GetCount(float scale) { return 0; }
}
        "#;
        let m = scan_text(src);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].full_name, "Stuff.GetCount");
        assert_eq!(m[0].return_type, "float");
        assert_eq!(m[0].parameters, "float scale");
        assert_eq!(m[0].tooltip.as_deref(), Some("Returns running count."));
        // Default category when omitted.
        assert_eq!(m[0].category, "Scripts");
    }

    #[test]
    fn ignores_unmarked_methods() {
        let src = r#"
public static class C {
    public static void NoAttribute() {}
    [BlueprintCallable] public static void Marked() {}
}
        "#;
        let m = scan_text(src);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].method, "Marked");
    }
}
