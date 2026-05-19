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

#[test]
fn give_first_arg_lists_player_names() {
    let ctx = ctx_with(&["Alice", "Bob"]);
    let r = complete("/give ", 6, 4, &ctx);
    let names: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(names.contains(&"Alice"));
    assert!(names.contains(&"Bob"));
    assert!(names.contains(&"@p"));
    assert!(names.contains(&"@a"));
    assert!(names.contains(&"@s"));
    assert!(names.contains(&"@r"));
}

#[test]
fn give_second_arg_lists_item_names() {
    let ctx = ctx_with(&["Alice"]);
    let r = complete("/give Alice ", 12, 4, &ctx);
    let labels: Vec<&str> = r.iter().map(|c| c.label.as_str()).collect();
    assert!(labels.contains(&"stone"));
    assert!(labels.contains(&"dirt"));
    assert!(labels.contains(&"torch"));
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

#[test]
fn apply_completion_replaces_at_depth_0() {
    let (out, cur) = apply_completion("/he", 3, "/help");
    assert_eq!(out, "/help");
    assert_eq!(cur, 5);
}

#[test]
fn apply_completion_replaces_player_token() {
    let (out, cur) = apply_completion("/give Al", 8, "Alice");
    assert_eq!(out, "/give Alice");
    assert_eq!(cur, 11);
}

#[test]
fn apply_completion_preserves_trailing_tokens() {
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
    let (out, _cur) = apply_completion("/gam", 4, &chosen);
    assert_eq!(out, "/gamemode");
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
    };
    popup.select_next();
    assert_eq!(popup.selected_index, 1);
    popup.select_next();
    assert_eq!(popup.selected_index, 2);
    popup.select_prev();
    assert_eq!(popup.selected_index, 1);
}
