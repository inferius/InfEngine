//! Chat-command tab autocomplete (W11.4).
//!
//! Mirrors MC's Brigadier dispatch-tree completion model
//! (`net/minecraft/commands/Commands.java` +
//! `com/mojang/brigadier/CommandDispatcher`): a tree of nodes
//! where each node is either a literal (verb) or a typed argument
//! kind. Walking the tree from the start of the input emits a
//! list of completion candidates for the token under the cursor.
//!
//! Scope:
//! - `complete(input, cursor, user_level, ctx)` returns the
//!   candidate completions for the token at `cursor`.
//! - The command tree is the union of:
//!     1. Every verb in `CMD_META` as a Literal at depth 0.
//!     2. Per-command argument grammars (give, tp, gamemode,
//!        setblock, kick, mute, dim, worldgen, ...) hard-coded
//!        in `build_tree`. Plugin commands extend the tree at
//!        runtime in a future commit; today plugin commands
//!        appear as bare Literals with no argument hint.
//! - The completion context (`CompletionContext`) carries live
//!   data the argument resolvers need: online player names,
//!   item registry names, block registry names. Built from the
//!   `Voxel` at the call site.
//!
//! Out of scope for W11.4:
//! - Selector resolution (@p, @a, @s) - listed as completions
//!   but not semantically resolved.
//! - Sub-command nesting beyond a single argument level (e.g.
//!   /gamemode <mode> [player] - we offer modes for the first
//!   arg, players for the second; chains deeper than 3 args
//!   fall back to empty completions).
//! - Numeric range validation - any integer/float arg returns
//!   no suggestions (the user types freely).

// W11.4: this module's surface is consumed by W11.5 (UI wiring).
// Until W11.5 lands the module looks fully unused from the bin
// crate's vantage; the cfg(test) submodule still exercises every
// item.
#![allow(dead_code)]

use std::collections::HashSet;

use super::perms::{CMD_META, PermLevel};
use crate::block::BLOCK_TYPES;
use crate::item::ITEM_TYPES;

// ---------------------------------------------------------------
// Argument kinds.
// ---------------------------------------------------------------

/// One node's argument shape. `Literal` matches exactly one verb
/// (e.g. "gamemode", "survival"); the typed kinds emit a list of
/// dynamic candidates resolved from `CompletionContext` at run time.
#[derive(Debug, Clone)]
pub(crate) enum ArgKind {
    /// Exact verb match. The node's `name` is the verb.
    Literal,
    /// Player name. Resolver pulls from `ctx.online_players` +
    /// the four MC-style selectors (@p, @a, @s, @r).
    Player,
    /// Block id name. Resolver pulls from `BLOCK_TYPES.name`.
    Block,
    /// Item id name. Resolver pulls from `ITEM_TYPES.name`.
    Item,
    /// Open-ended integer. No suggestions (user types freely).
    Integer,
    /// Open-ended float. No suggestions.
    #[allow(dead_code)] // wired up by future per-command grammars
    Float,
    /// Open-ended string token. No suggestions.
    String,
}

// ---------------------------------------------------------------
// Command tree.
// ---------------------------------------------------------------

/// One node in the command dispatch tree. The root has implicit
/// `Literal` children = every verb from `CMD_META`.
#[derive(Debug, Clone)]
pub(crate) struct CommandNode {
    /// Literal verb (e.g. "give") for Literal nodes; argument
    /// label (e.g. "<player>") for typed nodes. The label is
    /// only used for the popup hint - the completion algorithm
    /// reads it as text content for Literals, and as a typed
    /// resolver for everything else.
    pub(crate) name: String,
    /// Argument kind. Drives which resolver runs against the
    /// token under the cursor.
    pub(crate) arg_kind: ArgKind,
    /// Permission level required for this branch (mirrors the
    /// command's `CMD_META` row; deeper nodes inherit unless
    /// they specifically override).
    pub(crate) min_perm: PermLevel,
    /// Children: deeper nodes for the next token in the line.
    pub(crate) children: Vec<CommandNode>,
}

impl CommandNode {
    /// Build a Literal node with no children.
    fn literal(name: &str, perm: PermLevel) -> Self {
        Self {
            name: name.to_string(),
            arg_kind: ArgKind::Literal,
            min_perm: perm,
            children: Vec::new(),
        }
    }

    /// Build a typed node with no children.
    fn typed(label: &str, kind: ArgKind, perm: PermLevel) -> Self {
        Self {
            name: label.to_string(),
            arg_kind: kind,
            min_perm: perm,
            children: Vec::new(),
        }
    }

    /// Attach a child + return self (builder-style).
    fn with(mut self, child: CommandNode) -> Self {
        self.children.push(child);
        self
    }

    /// Attach multiple children + return self.
    fn with_all(mut self, children: Vec<CommandNode>) -> Self {
        self.children.extend(children);
        self
    }
}

/// Build the full command tree. Called once at startup +
/// re-called when plugin commands reload.
///
/// Layout:
/// - Root: a single `Literal("")` whose children are every verb
///   in `CMD_META` plus a small set of explicit per-command
///   argument grammars (give / tp / gamemode / setblock / kick /
///   mute / dim / worldgen / fill / kill / weather / time /
///   summon / spawnmob / ...).
/// - For commands not listed in the grammar table, children stay
///   empty - the popup hides for them once the user types past
///   the verb.
pub(crate) fn build_tree() -> CommandNode {
    let mut root = CommandNode::literal("", 0);
    // Seen verbs in the explicit grammar table; avoid duplicating
    // them through the CMD_META fallback walk below.
    let mut grammar_verbs: HashSet<String> = HashSet::new();

    // Per-command argument grammars. Each grammar block builds a
    // verb-level Literal with the right argument shape underneath.

    // /give <player> <item> [count]
    {
        let count = CommandNode::typed("<count>", ArgKind::Integer, 2);
        let item = CommandNode::typed("<item>", ArgKind::Item, 2).with(count);
        let player = CommandNode::typed("<player>", ArgKind::Player, 2).with(item);
        root.children.push(CommandNode::literal("give", 2).with(player));
        grammar_verbs.insert("give".into());
    }

    // /tp <player>
    {
        let player = CommandNode::typed("<player>", ArgKind::Player, 2);
        root.children.push(CommandNode::literal("tp", 2).with(player));
        grammar_verbs.insert("tp".into());
    }

    // /tppos <x> <y> <z>  +  /goto alias.
    {
        let z = CommandNode::typed("<z>", ArgKind::Float, 2);
        let y = CommandNode::typed("<y>", ArgKind::Float, 2).with(z);
        let x = CommandNode::typed("<x>", ArgKind::Float, 2).with(y);
        root.children.push(CommandNode::literal("tppos", 2).with(x.clone()));
        root.children.push(CommandNode::literal("goto", 2).with(x));
        grammar_verbs.insert("tppos".into());
        grammar_verbs.insert("goto".into());
    }

    // /gamemode <mode> [player]  +  /gm alias.
    {
        let modes: Vec<CommandNode> = ["survival", "creative", "0", "1"]
            .iter()
            .map(|m| {
                let player = CommandNode::typed("<player>", ArgKind::Player, 2);
                CommandNode::literal(m, 2).with(player)
            })
            .collect();
        let gm = CommandNode::literal("gamemode", 2).with_all(modes.clone());
        let gm_alias = CommandNode::literal("gm", 2).with_all(modes);
        root.children.push(gm);
        root.children.push(gm_alias);
        grammar_verbs.insert("gamemode".into());
        grammar_verbs.insert("gm".into());
    }

    // /setblock <x> <y> <z> <block>
    {
        let block = CommandNode::typed("<block>", ArgKind::Block, 3);
        let z = CommandNode::typed("<z>", ArgKind::Float, 3).with(block);
        let y = CommandNode::typed("<y>", ArgKind::Float, 3).with(z);
        let x = CommandNode::typed("<x>", ArgKind::Float, 3).with(y);
        root.children.push(CommandNode::literal("setblock", 3).with(x));
        grammar_verbs.insert("setblock".into());
    }

    // /fill <x1> <y1> <z1> <x2> <y2> <z2> <block>
    {
        let block = CommandNode::typed("<block>", ArgKind::Block, 3);
        let z2 = CommandNode::typed("<z2>", ArgKind::Float, 3).with(block);
        let y2 = CommandNode::typed("<y2>", ArgKind::Float, 3).with(z2);
        let x2 = CommandNode::typed("<x2>", ArgKind::Float, 3).with(y2);
        let z1 = CommandNode::typed("<z1>", ArgKind::Float, 3).with(x2);
        let y1 = CommandNode::typed("<y1>", ArgKind::Float, 3).with(z1);
        let x1 = CommandNode::typed("<x1>", ArgKind::Float, 3).with(y1);
        root.children.push(CommandNode::literal("fill", 3).with(x1));
        grammar_verbs.insert("fill".into());
    }

    // /kill [player]
    {
        let player = CommandNode::typed("<player>", ArgKind::Player, 3);
        root.children.push(CommandNode::literal("kill", 3).with(player));
        grammar_verbs.insert("kill".into());
    }

    // /kick <player> [reason]
    {
        let reason = CommandNode::typed("<reason>", ArgKind::String, 3);
        let player = CommandNode::typed("<player>", ArgKind::Player, 3).with(reason);
        root.children.push(CommandNode::literal("kick", 3).with(player));
        grammar_verbs.insert("kick".into());
    }

    // /mute <player>  +  /unmute <player>
    {
        let player_mute = CommandNode::typed("<player>", ArgKind::Player, 3);
        let player_unmute = CommandNode::typed("<player>", ArgKind::Player, 3);
        root.children.push(CommandNode::literal("mute", 3).with(player_mute));
        root.children.push(CommandNode::literal("unmute", 3).with(player_unmute));
        grammar_verbs.insert("mute".into());
        grammar_verbs.insert("unmute".into());
    }

    // /dim <name>
    {
        let dims: Vec<CommandNode> = ["overworld", "nether"]
            .iter()
            .map(|d| CommandNode::literal(d, 3))
            .collect();
        root.children.push(CommandNode::literal("dim", 3).with_all(dims));
        grammar_verbs.insert("dim".into());
    }

    // /worldgen <profile>  +  /gen alias.
    {
        let profiles: Vec<CommandNode> = ["classic", "flat", "islands", "amplified", "realistic"]
            .iter()
            .map(|p| CommandNode::literal(p, 4))
            .collect();
        let wg = CommandNode::literal("worldgen", 4).with_all(profiles.clone());
        let gen_alias = CommandNode::literal("gen", 4).with_all(profiles);
        root.children.push(wg);
        root.children.push(gen_alias);
        grammar_verbs.insert("worldgen".into());
        grammar_verbs.insert("gen".into());
    }

    // /weather <kind>
    {
        let kinds: Vec<CommandNode> = ["clear", "rain", "storm"]
            .iter()
            .map(|k| CommandNode::literal(k, 3))
            .collect();
        root.children.push(CommandNode::literal("weather", 3).with_all(kinds));
        grammar_verbs.insert("weather".into());
    }

    // /time <kind>
    {
        let kinds: Vec<CommandNode> = ["day", "night", "noon", "midnight"]
            .iter()
            .map(|k| CommandNode::literal(k, 3))
            .collect();
        root.children.push(CommandNode::literal("time", 3).with_all(kinds));
        grammar_verbs.insert("time".into());
    }

    // /summon <kind>  +  /spawnmob <kind>
    {
        let kinds: Vec<CommandNode> = ["zombie", "skeleton", "creeper", "pig", "cow", "chicken", "sheep"]
            .iter()
            .map(|k| CommandNode::literal(k, 3))
            .collect();
        root.children.push(CommandNode::literal("summon", 3).with_all(kinds.clone()));
        root.children.push(CommandNode::literal("spawnmob", 3).with_all(kinds));
        grammar_verbs.insert("summon".into());
        grammar_verbs.insert("spawnmob".into());
    }

    // /whisper <player> <message>  +  /w / /tell / /msg aliases.
    for verb in ["w", "whisper", "tell", "msg"] {
        let message = CommandNode::typed("<message>", ArgKind::String, 0);
        let player = CommandNode::typed("<player>", ArgKind::Player, 0).with(message);
        root.children.push(CommandNode::literal(verb, 0).with(player));
        grammar_verbs.insert(verb.into());
    }

    // /tool <name>
    {
        let tools: Vec<CommandNode> = [
            "pickaxe_wood",
            "pickaxe_stone",
            "pickaxe_iron",
            "sword_wood",
            "sword_stone",
            "sword_iron",
            "shovel_wood",
            "shovel_stone",
            "shovel_iron",
            "axe_wood",
            "axe_stone",
            "axe_iron",
            "bucket",
            "flint_and_steel",
        ]
        .iter()
        .map(|t| CommandNode::literal(t, 2))
        .collect();
        root.children.push(CommandNode::literal("tool", 2).with_all(tools));
        grammar_verbs.insert("tool".into());
    }

    // Fallback: every CMD_META verb not already in the grammar
    // table gets a bare Literal with no children. The popup
    // disappears as soon as the user types past the verb, which
    // is the right behaviour for commands taking 0 args.
    for meta in CMD_META {
        if !grammar_verbs.contains(meta.name) {
            root.children.push(CommandNode::literal(meta.name, meta.level));
        }
    }

    root
}

// ---------------------------------------------------------------
// Completion context + result.
// ---------------------------------------------------------------

/// Live data the typed argument resolvers consult. Built at call
/// site from the `Voxel`. Cheap to construct (just borrows /
/// short-lived clones).
#[derive(Debug, Default, Clone)]
pub(crate) struct CompletionContext {
    /// Online player names, lower-cased for case-insensitive
    /// matching. Includes the local player.
    pub(crate) online_players: Vec<String>,
}

/// One completion candidate. `replacement` is the text the popup
/// inserts when the user accepts; `label` is the human-readable
/// form shown in the list (same as replacement for most cases,
/// but distinct for things like `<player>` -> "@p" where the
/// label is a hint and the replacement is the literal text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Completion {
    pub(crate) replacement: String,
    pub(crate) label: String,
}

impl Completion {
    pub(crate) fn new(text: &str) -> Self {
        Self {
            replacement: text.to_string(),
            label: text.to_string(),
        }
    }
}

// ---------------------------------------------------------------
// Tokenization.
// ---------------------------------------------------------------

/// Split `input` into whitespace-separated tokens with their byte
/// offsets (start, end). The end of the slice (cursor past the
/// last char) counts as a trailing empty token if the prior char
/// was whitespace.
fn tokenize_with_offsets(input: &str) -> Vec<(usize, usize, &str)> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // Skip whitespace.
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        tokens.push((start, i, &input[start..i]));
    }
    tokens
}

/// Find the token under `cursor`. Returns `(token_index,
/// token_start, token_end, token_text)`. If the cursor is past
/// the last token and the previous char is whitespace, returns
/// a virtual empty token at the cursor position.
fn token_at_cursor(
    input: &str,
    cursor: usize,
) -> (usize, usize, usize, &str) {
    let cursor = cursor.min(input.len());
    let tokens = tokenize_with_offsets(input);
    for (i, (start, end, text)) in tokens.iter().enumerate() {
        if cursor >= *start && cursor <= *end {
            return (i, *start, *end, text);
        }
    }
    // Past the last token (in trailing whitespace or empty input)
    // -> virtual empty token at the cursor.
    (tokens.len(), cursor, cursor, "")
}

// ---------------------------------------------------------------
// Completion algorithm.
// ---------------------------------------------------------------

/// Compute the list of candidate completions for the token at
/// `cursor` in `input`. `user_level` is the caller's permission
/// level (defaults to 0 for anonymous chat).
///
/// Examples:
/// - `complete("/", 1, 0, ...)` -> every level-0 command.
/// - `complete("/give ", 6, 4, ...)` -> player names + @-selectors.
/// - `complete("/setblock 1 2 3 ", 16, 4, ...)` -> block names.
///
/// The result is alphabetically sorted + de-duplicated so a stable
/// popup order survives any order changes in the underlying
/// command tree.
pub(crate) fn complete(
    input: &str,
    cursor: usize,
    user_level: PermLevel,
    ctx: &CompletionContext,
) -> Vec<Completion> {
    let tree = build_tree();
    complete_with_tree(&tree, input, cursor, user_level, ctx)
}

/// Lower-level entry: caller supplies the tree (lets a benchmark /
/// test reuse a cached tree instead of rebuilding each call).
pub(crate) fn complete_with_tree(
    tree: &CommandNode,
    input: &str,
    cursor: usize,
    user_level: PermLevel,
    ctx: &CompletionContext,
) -> Vec<Completion> {
    // No leading slash -> the input is a chat message, not a
    // command. Return nothing so the autocomplete popup hides.
    if !input.starts_with('/') {
        return Vec::new();
    }
    let body = &input[1..];
    let body_cursor = cursor.saturating_sub(1);
    let (token_idx, _start, _end, token_text) =
        token_at_cursor(body, body_cursor);

    // Walk down the tree following the prior tokens; the node we
    // land on dictates the candidates for the current token.
    let tokens = tokenize_with_offsets(body);
    let mut current = tree;
    for i in 0..token_idx {
        if i >= tokens.len() {
            break;
        }
        let prior = tokens[i].2;
        // Find a child that matches the prior token. For Literal
        // children: exact match. For typed children: first
        // matching kind (typed children consume any token).
        let matched = current.children.iter().find(|c| match c.arg_kind {
            ArgKind::Literal => c.name == prior,
            _ => true,
        });
        match matched {
            Some(n) => current = n,
            None => return Vec::new(),
        }
    }

    // Collect candidates from `current`'s children matching the
    // current token's prefix + the user's perm level.
    let mut out: Vec<Completion> = Vec::new();
    let prefix = token_text.to_lowercase();
    for child in &current.children {
        if user_level < child.min_perm {
            continue;
        }
        match child.arg_kind {
            ArgKind::Literal => {
                let name = child.name.to_lowercase();
                if name.starts_with(&prefix) {
                    out.push(Completion::new(&child.name));
                }
            }
            ArgKind::Player => {
                for p in &ctx.online_players {
                    if p.to_lowercase().starts_with(&prefix) {
                        out.push(Completion::new(p));
                    }
                }
                // MC-style selectors. Always offered when prefix
                // is empty or starts with '@'.
                for sel in ["@p", "@a", "@s", "@r"] {
                    if sel.to_lowercase().starts_with(&prefix) {
                        out.push(Completion::new(sel));
                    }
                }
            }
            ArgKind::Block => {
                for b in BLOCK_TYPES {
                    if b.name.to_lowercase().starts_with(&prefix) {
                        out.push(Completion::new(b.name));
                    }
                }
            }
            ArgKind::Item => {
                for it in ITEM_TYPES {
                    if it.name.to_lowercase().starts_with(&prefix) {
                        out.push(Completion::new(it.name));
                    }
                }
            }
            ArgKind::Integer
            | ArgKind::Float
            | ArgKind::String => {
                // Open-ended numeric/string args: no suggestion,
                // but we DO show the label as a hint when the
                // current prefix is empty.
                if prefix.is_empty() {
                    out.push(Completion {
                        replacement: String::new(),
                        label: child.name.clone(),
                    });
                }
            }
        }
    }

    // At depth 0 prepend the leading slash to literal replacements
    // so the popup-accept path produces "/give" instead of "give".
    if token_idx == 0 {
        for c in &mut out {
            if !c.replacement.is_empty() {
                c.replacement = format!("/{}", c.replacement);
            }
        }
    }

    // Stable sort + dedup on replacement so callers get a
    // predictable popup order independent of CMD_META row order.
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out.dedup_by(|a, b| a.replacement == b.replacement);
    out
}

// ---------------------------------------------------------------
// W11.5 - popup state shared with the UI.
// ---------------------------------------------------------------

/// Max visible rows in the popup. Beyond this the user scrolls.
pub(crate) const POPUP_MAX_VISIBLE: usize = 10;

/// Active completion popup state. Stored on `Voxel::chat_completion`
/// while the popup is visible.
#[derive(Debug, Clone, Default)]
pub(crate) struct ChatCompletionPopup {
    /// Candidate list - results of the last `complete()` call.
    pub(crate) candidates: Vec<Completion>,
    /// Currently highlighted candidate (cycle target for Tab +
    /// arrow keys). Always `< candidates.len()` when the popup
    /// is non-empty.
    pub(crate) selected_index: usize,
    /// Snapshot of the input the popup was computed against. The
    /// UI compares this against the live `chat_input` each frame
    /// to decide whether to recompute or reuse.
    pub(crate) input_snapshot: String,
    /// Cursor position the popup was computed against. Same
    /// staleness check as `input_snapshot`.
    pub(crate) cursor_snapshot: usize,
}

impl ChatCompletionPopup {
    /// Move the highlight forward by one, wrapping at the end.
    /// No-op when the popup is empty.
    pub(crate) fn select_next(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1) % self.candidates.len();
    }

    /// Move the highlight back by one, wrapping past the start.
    pub(crate) fn select_prev(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        self.selected_index = if self.selected_index == 0 {
            self.candidates.len() - 1
        } else {
            self.selected_index - 1
        };
    }

    /// Return the currently highlighted completion's text.
    pub(crate) fn selected(&self) -> Option<&Completion> {
        self.candidates.get(self.selected_index)
    }
}

/// Recompute the popup state for `input` + `cursor`. Returns
/// `None` when there are no candidates (the UI hides the popup).
pub(crate) fn recompute_popup(
    input: &str,
    cursor: usize,
    user_level: PermLevel,
    ctx: &CompletionContext,
    previous: Option<&ChatCompletionPopup>,
) -> Option<ChatCompletionPopup> {
    let candidates = complete(input, cursor, user_level, ctx);
    if candidates.is_empty() {
        return None;
    }
    // Preserve selected_index when possible (same first candidate
    // = treat as same list). Otherwise reset to 0.
    let selected_index = match previous {
        Some(prev)
            if !prev.candidates.is_empty()
                && prev.candidates.first().map(|c| &c.replacement)
                    == candidates.first().map(|c| &c.replacement)
                && prev.selected_index < candidates.len() =>
        {
            prev.selected_index
        }
        _ => 0,
    };
    Some(ChatCompletionPopup {
        candidates,
        selected_index,
        input_snapshot: input.to_string(),
        cursor_snapshot: cursor,
    })
}

/// Replace the token at `cursor` in `input` with `replacement`,
/// preserving the rest of the line. Returns the new (input, cursor)
/// pair. If the token is at the start of input and the replacement
/// already starts with '/', the slash is NOT duplicated.
pub(crate) fn apply_completion(
    input: &str,
    cursor: usize,
    replacement: &str,
) -> (String, usize) {
    if !input.starts_with('/') {
        return (input.to_string(), cursor);
    }
    let body = &input[1..];
    let body_cursor = cursor.saturating_sub(1);
    let (token_idx, start, end, _text) = token_at_cursor(body, body_cursor);
    // Adjust token bounds back to absolute (account for leading '/').
    let abs_start = start + 1;
    let abs_end = end + 1;
    // Depth-0 replacements come with the slash baked in; deeper
    // replacements are bare tokens. Strip the slash from the
    // replacement before splicing so we don't end up with `//give`.
    let (slice_start, replacement_str) = if token_idx == 0 {
        if let Some(stripped) = replacement.strip_prefix('/') {
            (0, stripped.to_string())
        } else {
            (1, replacement.to_string())
        }
    } else {
        (abs_start, replacement.to_string())
    };
    let mut out = String::new();
    out.push_str(&input[..slice_start]);
    if token_idx == 0 {
        out.push('/');
    }
    out.push_str(&replacement_str);
    out.push_str(&input[abs_end..]);
    let new_cursor = if token_idx == 0 {
        1 + replacement_str.len()
    } else {
        abs_start + replacement_str.len()
    };
    (out, new_cursor)
}

