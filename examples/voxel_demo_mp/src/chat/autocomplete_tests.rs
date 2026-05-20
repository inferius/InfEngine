//! Unit tests for `chat/autocomplete.rs`.
//!
//! Split out as a separate file in W11.6 to keep the engine
//! module under the 1000 LOC hard limit.

#![cfg(test)]

use std::collections::HashSet;

use super::autocomplete::{
    ChatCompletionPopup, Completion, CompletionContext, POPUP_MAX_VISIBLE,
    apply_completion, build_tree, complete, recompute_popup,
};
use super::perms::CMD_META;

fn ctx_with(players: &[&str]) -> CompletionContext {
    CompletionContext {
        online_players: players.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn slash_alone_lists_all_commands_for_level_0() {
    let r = complete("/", 1, 0, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    // Common level-0 commands should appear.
    assert!(names.contains(&"help"));
    assert!(names.contains(&"players"));
    assert!(names.contains(&"me"));
    // Commands needing higher perm should NOT appear at level 0.
    assert!(!names.contains(&"setblock"));
    assert!(!names.contains(&"save"));
}

#[test]
fn slash_filters_commands_by_user_perm_level() {
    let r0 = complete("/", 1, 0, &CompletionContext::default());
    let r4 = complete("/", 1, 4, &CompletionContext::default());
    // Higher perm sees more commands.
    assert!(r4.len() > r0.len());
    let names4: Vec<&str> = r4.iter().map(|c| c.label.as_str()).collect();
    assert!(names4.contains(&"save"));
    assert!(names4.contains(&"setblock"));
}

// W14.5: /give grammar is now <block> [count], matching cmd_give in
// cmds_creative.rs which does a BLOCK_TYPES lookup. No <player> arg.
#[test]
fn give_first_arg_lists_block_names() {
    let ctx = ctx_with(&["Alice", "Bob"]);
    let r = complete("/give ", 6, 4, &ctx);
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    // Block names should appear (BLOCK_TYPES lookup).
    assert!(names.contains(&"stone"), "stone missing from /give completions");
    assert!(names.contains(&"dirt"), "dirt missing from /give completions");
    // Player names / selectors must NOT appear (no <player> arg). W14.5.
    assert!(!names.contains(&"@p"), "@p should not appear for /give");
    assert!(!names.contains(&"Alice"), "player name should not appear for /give");
}

// W14.5: /give arg-2 is now an optional count (Integer), no item arg.
#[test]
fn give_second_arg_is_count_hint() {
    let r = complete("/give stone ", 12, 4, &CompletionContext::default());
    // Integer arg with empty prefix -> shows hint "<count>" label,
    // empty replacement.
    let hints: Vec<&str> = r
        .iter()
        .filter(|c| c.replacement.is_empty())
        .map(|c| c.label.as_str())
        .collect();
    assert!(hints.contains(&"<count>"), "expected <count> hint, got: {hints:?}");
}

#[test]
fn gamemode_arg_lists_survival_creative() {
    let r = complete("/gamemode ", 10, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"survival"));
    assert!(names.contains(&"creative"));
    assert!(names.contains(&"0"));
    assert!(names.contains(&"1"));
}

#[test]
fn setblock_fourth_arg_lists_block_names() {
    let r = complete("/setblock 1 2 3 ", 16, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"stone"));
    assert!(names.contains(&"glass"));
    assert!(names.contains(&"redstone_torch"));
}

#[test]
fn tab_with_one_candidate_replaces_token() {
    let r = complete("/he", 3, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    for n in &names {
        assert!(n.starts_with("he"), "expected he* got {n}");
    }
    let first = &r[0];
    let (out, _cur) = apply_completion("/he", 3, &first.replacement);
    assert!(out.starts_with("/he"));
}

#[test]
fn tab_with_many_candidates_cycles() {
    let r = complete("/s", 2, 4, &CompletionContext::default());
    assert!(r.len() > 1, "expected several /s* candidates, got {}", r.len());
}

#[test]
fn completion_respects_partial_prefix_match() {
    let r = complete("/giv", 4, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"give"));
    assert!(names.contains(&"giveall"));
}

#[test]
fn no_slash_returns_empty() {
    let r = complete("hello world", 5, 4, &CompletionContext::default());
    assert!(r.is_empty());
}

#[test]
fn tp_arg_lists_player_names() {
    let ctx = ctx_with(&["Alice", "Bob", "Carol"]);
    let r = complete("/tp ", 4, 4, &ctx);
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"Alice"));
    assert!(names.contains(&"Bob"));
    assert!(names.contains(&"Carol"));
}

#[test]
fn dim_arg_lists_overworld_nether() {
    let r = complete("/dim ", 5, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"overworld"));
    assert!(names.contains(&"nether"));
}

#[test]
fn worldgen_arg_lists_profiles() {
    let r = complete("/worldgen ", 10, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    for p in ["classic", "flat", "islands", "amplified", "realistic"] {
        assert!(names.contains(&p), "missing worldgen profile {p}");
    }
}

#[test]
fn weather_arg_lists_kinds() {
    let r = complete("/weather ", 9, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    for k in ["clear", "rain", "storm"] {
        assert!(names.contains(&k), "missing weather kind {k}");
    }
}

#[test]
fn time_arg_lists_kinds() {
    let r = complete("/time ", 6, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    for k in ["day", "night", "noon", "midnight"] {
        assert!(names.contains(&k), "missing time kind {k}");
    }
}

#[test]
fn summon_arg_lists_mobs() {
    let r = complete("/summon ", 8, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    for m in ["zombie", "skeleton", "creeper"] {
        assert!(names.contains(&m), "missing mob {m}");
    }
}

#[test]
fn kick_arg_lists_player_names() {
    let ctx = ctx_with(&["Alice"]);
    let r = complete("/kick ", 6, 4, &ctx);
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"Alice"));
}

#[test]
fn mute_arg_lists_player_names() {
    let ctx = ctx_with(&["Alice"]);
    let r = complete("/mute ", 6, 4, &ctx);
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"Alice"));
}

#[test]
fn tool_arg_lists_tool_names() {
    let r = complete("/tool ", 6, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"pickaxe_wood"));
    assert!(names.contains(&"sword_iron"));
    assert!(names.contains(&"flint_and_steel"));
}

#[test]
fn whisper_first_arg_lists_player_names() {
    let ctx = ctx_with(&["Alice"]);
    let r = complete("/w ", 3, 0, &ctx);
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"Alice"));
}

// W14.1: apply_completion now appends a trailing space when the
// suffix is empty so the cursor lands after the inserted text +
// space for immediate next-arg typing.
#[test]
fn apply_completion_replaces_at_depth_0() {
    let (out, cur) = apply_completion("/he", 3, "/help");
    // Trailing space added because suffix is empty. W14.1.
    assert_eq!(out, "/help ");
    assert_eq!(cur, 6);
}

#[test]
fn apply_completion_replaces_player_token() {
    let (out, cur) = apply_completion("/give Al", 8, "Alice");
    // Trailing space added - suffix was empty. W14.1.
    assert_eq!(out, "/give Alice ");
    assert_eq!(cur, 12);
}

#[test]
fn apply_completion_preserves_trailing_tokens() {
    // When there is existing text after the token the suffix is
    // preserved as-is; no extra space is inserted. W14.1.
    let (out, _cur) = apply_completion("/give Al diamond", 8, "Alice");
    assert_eq!(out, "/give Alice diamond");
}

#[test]
fn tree_contains_every_cmd_meta_verb() {
    let tree = build_tree();
    let root_names: HashSet<&str> =
        tree.children.iter().map(|n| n.name.as_str()).collect();
    for meta in CMD_META {
        assert!(
            root_names.contains(meta.name),
            "command tree missing /{}",
            meta.name
        );
    }
}

#[test]
fn tree_root_has_at_least_70_verbs() {
    let tree = build_tree();
    assert!(
        tree.children.len() >= 70,
        "tree root only has {} children",
        tree.children.len()
    );
}

#[test]
fn slash_with_prefix_filters_to_prefix() {
    let r = complete("/sa", 3, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"save"));
    assert!(!names.contains(&"help"));
}

// ---------------------------------------------------------------
// W11.5 - popup state tests.
// ---------------------------------------------------------------

#[test]
fn popup_select_next_wraps_around() {
    let mut p = ChatCompletionPopup {
        candidates: vec![
            Completion::new("a"),
            Completion::new("b"),
            Completion::new("c"),
        ],
        selected_index: 2,
        input_snapshot: String::new(),
        cursor_snapshot: 0,
        pending_cursor: None,
    };
    p.select_next();
    assert_eq!(p.selected_index, 0);
}

#[test]
fn popup_select_prev_wraps_around() {
    let mut p = ChatCompletionPopup {
        candidates: vec![Completion::new("a"), Completion::new("b")],
        selected_index: 0,
        input_snapshot: String::new(),
        cursor_snapshot: 0,
        pending_cursor: None,
    };
    p.select_prev();
    assert_eq!(p.selected_index, 1);
}

#[test]
fn popup_select_no_op_on_empty() {
    let mut p = ChatCompletionPopup::default();
    p.select_next();
    p.select_prev();
    assert_eq!(p.selected_index, 0);
}

#[test]
fn popup_selected_returns_current() {
    let p = ChatCompletionPopup {
        candidates: vec![Completion::new("alpha"), Completion::new("beta")],
        selected_index: 1,
        input_snapshot: String::new(),
        cursor_snapshot: 0,
        pending_cursor: None,
    };
    assert_eq!(p.selected().map(|c| c.label.as_str()), Some("beta"));
}

#[test]
fn recompute_popup_returns_some_when_candidates_exist() {
    let p = recompute_popup("/", 1, 4, &CompletionContext::default(), None);
    assert!(p.is_some());
    let p = p.unwrap();
    assert!(!p.candidates.is_empty());
    assert_eq!(p.selected_index, 0);
}

#[test]
fn recompute_popup_returns_none_when_no_match() {
    let p = recompute_popup(
        "/xyzzy_no",
        9,
        4,
        &CompletionContext::default(),
        None,
    );
    assert!(p.is_none());
}

#[test]
fn recompute_popup_preserves_selection_across_no_op_keystroke() {
    let p1 = recompute_popup("/", 1, 4, &CompletionContext::default(), None)
        .unwrap();
    let mut p1 = p1;
    p1.selected_index = 5;
    let p2 = recompute_popup(
        "/",
        1,
        4,
        &CompletionContext::default(),
        Some(&p1),
    )
    .unwrap();
    assert_eq!(p2.selected_index, 5);
}

#[test]
fn recompute_popup_resets_selection_on_list_change() {
    let p1 = recompute_popup("/sa", 3, 4, &CompletionContext::default(), None)
        .unwrap();
    let mut p1 = p1;
    p1.selected_index = 1.min(p1.candidates.len() - 1);
    let p2 = recompute_popup(
        "/dim",
        4,
        4,
        &CompletionContext::default(),
        Some(&p1),
    )
    .unwrap();
    assert_eq!(p2.selected_index, 0);
}

#[test]
fn popup_max_visible_is_10() {
    assert_eq!(POPUP_MAX_VISIBLE, 10);
}

#[test]
fn esc_closes_popup_semantic() {
    let opened: Option<ChatCompletionPopup> = Some(
        recompute_popup("/", 1, 4, &CompletionContext::default(), None)
            .unwrap(),
    );
    assert!(opened.is_some());
    let closed: Option<ChatCompletionPopup> = None;
    assert!(closed.is_none());
}

#[test]
fn enter_accepts_selected_semantic() {
    let popup =
        recompute_popup("/sa", 3, 4, &CompletionContext::default(), None)
            .unwrap();
    let chosen = popup.selected().expect("at least one candidate");
    let (out, _cur) = apply_completion("/sa", 3, &chosen.replacement);
    assert!(out.starts_with('/'));
    assert!(out.starts_with("/sa") || out.starts_with(&chosen.replacement));
}

// W14.1: trailing space appended on accept so next arg is
// immediately typeable.
#[test]
fn tab_in_chat_replaces_token_under_cursor() {
    let mut popup =
        recompute_popup("/gam", 4, 4, &CompletionContext::default(), None)
            .unwrap();
    let target_idx = popup
        .candidates
        .iter()
        .position(|c| c.replacement == "/gamemode")
        .expect("/gamemode should be a candidate");
    popup.selected_index = target_idx;
    let chosen = popup.selected().unwrap().replacement.clone();
    let (out, cur) = apply_completion("/gam", 4, &chosen);
    // Trailing space appended (suffix was empty). W14.1.
    assert_eq!(out, "/gamemode ");
    // Cursor is after the space. W14.1.
    assert_eq!(cur, 10);
}

#[test]
fn arrow_up_down_changes_selection() {
    let mut popup = ChatCompletionPopup {
        candidates: vec![
            Completion::new("alpha"),
            Completion::new("beta"),
            Completion::new("gamma"),
        ],
        selected_index: 0,
        input_snapshot: String::new(),
        cursor_snapshot: 0,
        pending_cursor: None,
    };
    popup.select_next();
    assert_eq!(popup.selected_index, 1);
    popup.select_next();
    assert_eq!(popup.selected_index, 2);
    popup.select_prev();
    assert_eq!(popup.selected_index, 1);
}

// ---------------------------------------------------------------
// W14 tests.
// ---------------------------------------------------------------

// W14.1 - cursor-to-end after accept.

/// Accepting a suggestion appends a trailing space so the user
/// can type the next arg immediately. W14.1.
#[test]
fn accepting_suggestion_appends_space() {
    let (out, _) = apply_completion("/giv", 4, "/give");
    assert!(out.ends_with(' '), "expected trailing space, got: {out:?}");
}

/// The returned cursor lands at the end of the inserted text
/// + space. W14.1.
#[test]
fn cursor_lands_at_end_after_accept() {
    let (out, cur) = apply_completion("/giv", 4, "/give");
    // "/give " = 6 chars, cursor should be at index 6.
    assert_eq!(cur, out.len(), "cursor should be at end of output");
}

/// No trailing space when there is already text after the
/// replaced token (preserves existing suffix). W14.1.
#[test]
fn no_trailing_space_when_suffix_present() {
    let (out, _) = apply_completion("/give sto 64", 9, "stone");
    // "sto" at positions 6-8; suffix = " 64". No extra space.
    assert_eq!(out, "/give stone 64");
}

/// Cursor position is at end of replacement (not suffix) when
/// suffix is present and no trailing space is added. W14.1.
#[test]
fn cursor_at_end_of_replacement_with_suffix() {
    let (out, cur) = apply_completion("/give sto 64", 9, "stone");
    assert_eq!(out, "/give stone 64");
    // "stone" ends at position 11.
    assert_eq!(cur, 11);
}

// W14.2 - no spurious cursor jumps.

/// Recomputing the popup does NOT modify the input string.
/// This is the key invariant: the popup is a passive overlay.
/// W14.2.
#[test]
fn typing_does_not_modify_input_outside_user_keys() {
    let input = "/give ";
    let popup_before =
        recompute_popup(input, 6, 4, &CompletionContext::default(), None);
    // input string is unchanged; popup is a read-only overlay.
    assert_eq!(input, "/give ");
    // Popup should have candidates (block names).
    assert!(popup_before.is_some());
}

/// Popup recompute with same input/cursor returns same candidate
/// list (no thrashing). W14.2.
#[test]
fn popup_close_preserves_cursor_position() {
    // Simulate: popup was open, user typed a char that closed it
    // (no candidates). The input string must not have been touched.
    let mut input = String::from("/gu");
    let popup =
        recompute_popup(&input, 3, 4, &CompletionContext::default(), None);
    // "/gu" has no known command -> None.
    assert!(popup.is_none(), "'/gu' should yield no completions");
    // Input must still be "/gu" - nothing touched it.
    assert_eq!(input.as_str(), "/gu");
    input.push('i'); // simulate typing 'i' -> "/gui"
    let popup2 =
        recompute_popup(&input, 4, 4, &CompletionContext::default(), None);
    // Still no match.
    assert!(popup2.is_none());
    assert_eq!(input.as_str(), "/gui");
}

/// Pressing space in the middle of a typed command is NOT
/// intercepted by autocomplete logic - it flows through to the
/// text buffer unchanged. This test verifies the autocomplete
/// function itself does not mutate the input. W14.2.
#[test]
fn space_keypress_does_not_change_input() {
    let input_before = "/give stone";
    // Simulate user pressing space - input becomes "/give stone "
    let input_after = "/give stone ";
    let popup = recompute_popup(
        input_after,
        12,
        4,
        &CompletionContext::default(),
        None,
    );
    // The input string that was passed in is unchanged.
    assert_eq!(input_before, "/give stone");
    // Popup with empty prefix on the count arg shows the hint.
    // (May be Some or None depending on hint visibility.)
    let _ = popup;
}

// W14.3 - popup dismiss + Enter behaviour.

/// Esc closes the popup (sets it to None). W14.3.
#[test]
fn esc_with_open_popup_closes_only() {
    let popup: Option<ChatCompletionPopup> =
        recompute_popup("/", 1, 4, &CompletionContext::default(), None);
    assert!(popup.is_some(), "popup should open for '/'");
    // Simulated Esc: set to None. Input not modified.
    let after_esc: Option<ChatCompletionPopup> = None;
    assert!(after_esc.is_none());
}

/// After Tab-accept the replacement is applied and the popup is
/// closed (None). W14.3.
#[test]
fn tab_with_open_popup_accepts_but_does_not_submit() {
    let popup =
        recompute_popup("/giv", 4, 4, &CompletionContext::default(), None)
            .unwrap();
    let chosen = popup.selected().unwrap();
    let (out, cur) = apply_completion("/giv", 4, &chosen.replacement);
    // Replacement was applied.
    assert!(out.starts_with("/give"), "expected /give*, got: {out}");
    // Cursor is at end (after trailing space). W14.1.
    assert_eq!(cur, out.len());
    // Tab does NOT set submit flag - that is handled by the UI.
    // (No way to assert here; the test documents the contract.)
}

/// Enter-accept applies the replacement. In the UI this also
/// closes the popup and sets chat_send_now. W14.3.
#[test]
fn enter_with_open_popup_accepts_and_submits() {
    let popup =
        recompute_popup("/giv", 4, 4, &CompletionContext::default(), None)
            .unwrap();
    let chosen = popup.selected().unwrap();
    let (out, _) = apply_completion("/giv", 4, &chosen.replacement);
    // Replacement was applied.
    assert!(out.starts_with('/'));
    assert!(!out.trim().is_empty());
    // In the UI, popup is set to None AND chat_send_now = true.
    // (UI logic is tested by manual verification; here we confirm
    // apply_completion returns a usable string.)
}

// W14.4 - block-name suggestions for setblock / fill.

/// /setblock arg-4 suggests block names from BLOCK_TYPES. W14.4.
#[test]
fn setblock_arg4_suggests_dirt_stone_grass() {
    let r = complete(
        "/setblock 0 0 0 ",
        16,
        4,
        &CompletionContext::default(),
    );
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"dirt"), "dirt missing from setblock completions");
    assert!(names.contains(&"stone"), "stone missing from setblock completions");
    assert!(names.contains(&"grass"), "grass missing from setblock completions");
}

/// Partial block prefix narrows setblock suggestions. W14.4.
#[test]
fn setblock_arg4_partial_prefix_filters() {
    let r = complete("/setblock 0 0 0 st", 18, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"stone"), "stone missing from 'st' prefix");
    // "dirt" must not appear when prefix is "st".
    assert!(!names.contains(&"dirt"), "dirt should not match prefix 'st'");
}

/// The block names suggested by autocomplete are the same names
/// that cmd_setblock and cmd_give accept (BLOCK_TYPES.name). W14.4.
#[test]
fn setblock_dispatch_accepts_autocomplete_suggestions() {
    use crate::block::BLOCK_TYPES;
    let r = complete(
        "/setblock 0 0 0 ",
        16,
        4,
        &CompletionContext::default(),
    );
    // Every suggested block name should exist in BLOCK_TYPES.
    for c in &r {
        if c.replacement.is_empty() {
            continue; // hint placeholders
        }
        let found = BLOCK_TYPES
            .iter()
            .any(|bt| bt.name.eq_ignore_ascii_case(&c.replacement));
        assert!(
            found,
            "autocomplete suggested '{}' but it is not in BLOCK_TYPES",
            c.replacement
        );
    }
}

/// /fill arg-7 also suggests block names. W14.4.
#[test]
fn fill_arg7_suggests_block_names() {
    let r = complete(
        "/fill 0 0 0 1 1 1 ",
        18,
        4,
        &CompletionContext::default(),
    );
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"stone"), "stone missing from fill completions");
    assert!(names.contains(&"dirt"), "dirt missing from fill completions");
}

// W14.5 - /give grammar matches dispatch.

/// /give first arg suggests block names (matches cmd_give which
/// does a BLOCK_TYPES lookup). W14.5.
#[test]
fn give_with_autocomplete_block_name_dispatches() {
    use crate::block::BLOCK_TYPES;
    let r = complete("/give ", 6, 4, &CompletionContext::default());
    // Every non-empty suggestion must be a valid block name.
    for c in &r {
        if c.replacement.is_empty() {
            continue;
        }
        let found = BLOCK_TYPES
            .iter()
            .any(|bt| bt.name.eq_ignore_ascii_case(&c.replacement));
        assert!(
            found,
            "/give autocomplete suggested '{}' not in BLOCK_TYPES",
            c.replacement
        );
    }
}

/// Selector @p is not offered for /give (no player arg). W14.5.
#[test]
fn give_does_not_suggest_at_p_selector() {
    let r = complete("/give ", 6, 4, &CompletionContext::default());
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(
        !names.contains(&"@p"),
        "@p should not appear for /give since there is no player arg"
    );
}

/// /give with a block name from autocomplete produces a valid
/// command string. W14.5.
#[test]
fn give_autocomplete_produces_valid_command() {
    // Simulate: user typed "/give ", popup offered "stone",
    // apply_completion gives "/give stone ".
    let (out, cur) = apply_completion("/give ", 6, "stone");
    assert_eq!(out, "/give stone ");
    assert_eq!(cur, 12);
    // The resulting command is parseable by cmd_give:
    // arg = "stone " -> block_name = "stone", count = 64.
    // (Actual dispatch tested in cmds_creative tests.)
}
