//! W458 - the FULL villager trade table, generated 1:1 from the MC
//! 26.1.2 data JSONs (`data/minecraft/villager_trade/<profession>/
//! <tier>/*.json`), tiers 1-5 (novice..master). Regenerate with the
//! generator script rather than editing rows by hand. Entries with
//! `given_item_modifiers` (enchanted gear, exploration maps, dyed
//! armor) or items missing from the registry are skipped and listed
//! below; they land together with the item-modifier system.
//!
//! Skipped source entries:
//   armorer/2/emerald_chainmail_boots: unmapped chainmail_boots
//   armorer/2/emerald_chainmail_leggings: unmapped chainmail_leggings
//   armorer/3/emerald_chainmail_helmet: unmapped chainmail_helmet
//   armorer/4/emerald_enchanted_diamond_boots: given_item_modifiers
//   armorer/4/emerald_enchanted_diamond_leggings: given_item_modifiers
//   armorer/5/emerald_enchanted_diamond_chestplate: given_item_modifiers
//   armorer/5/emerald_enchanted_diamond_helmet: given_item_modifiers
//   butcher/1/emerald_rabbit_stew: unmapped rabbit_stew
//   butcher/1/rabbit_emerald: unmapped rabbit
//   butcher/5/sweet_berries_emerald: unmapped sweet_berries
//   cartographer/2/emerald_and_compass_explorer_jungle_map: given_item_modifiers
//   cartographer/2/emerald_and_compass_explorer_swamp_map: given_item_modifiers
//   cartographer/2/emerald_and_compass_village_desert_map: given_item_modifiers
//   cartographer/2/emerald_and_compass_village_plains_map: given_item_modifiers
//   cartographer/2/emerald_and_compass_village_savanna_map: given_item_modifiers
//   cartographer/2/emerald_and_compass_village_snowy_map: given_item_modifiers
//   cartographer/2/emerald_and_compass_village_taiga_map: given_item_modifiers
//   cartographer/3/emerald_and_compass_ocean_explorer_map: given_item_modifiers
//   cartographer/3/emerald_and_compass_trial_chamber_map: given_item_modifiers
//   cartographer/4/emerald_black_banner: unmapped black_banner
//   cartographer/4/emerald_blue_banner: unmapped blue_banner
//   cartographer/4/emerald_brown_banner: unmapped brown_banner
//   cartographer/4/emerald_cyan_banner: unmapped cyan_banner
//   cartographer/4/emerald_gray_banner: unmapped gray_banner
//   cartographer/4/emerald_green_banner: unmapped green_banner
//   cartographer/4/emerald_item_frame: unmapped item_frame
//   cartographer/4/emerald_light_blue_banner: unmapped light_blue_banner
//   cartographer/4/emerald_lime_banner: unmapped lime_banner
//   cartographer/4/emerald_magenta_banner: unmapped magenta_banner
//   cartographer/4/emerald_orange_banner: unmapped orange_banner
//   cartographer/4/emerald_pink_banner: unmapped pink_banner
//   cartographer/4/emerald_purple_banner: unmapped purple_banner
//   cartographer/4/emerald_red_banner: unmapped red_banner
//   cartographer/4/emerald_white_banner: unmapped white_banner
//   cartographer/4/emerald_yellow_banner: unmapped yellow_banner
//   cartographer/5/emerald_and_compass_woodland_mansion_map: given_item_modifiers
//   cartographer/5/emerald_globe_banner_pattern: unmapped globe_banner_pattern
//   cleric/3/rabbit_foot_emerald: unmapped rabbit_foot
//   cleric/4/glass_bottle_emerald: unmapped glass_bottle
//   cleric/4/turtle_scute_emerald: unmapped turtle_scute
//   farmer/4/emerald_cake: unmapped cake
//   farmer/4/emerald_suspicious_stew: given_item_modifiers
//   fisherman/1/emerald_cod_bucket: unmapped cod_bucket
//   fisherman/3/emerald_enchanted_fishing_rod: given_item_modifiers
//   fisherman/4/tropical_fish_emerald: unmapped tropical_fish
//   fisherman/5/acacia_boat_emerald: unmapped acacia_boat
//   fisherman/5/dark_oak_boat_emerald: unmapped dark_oak_boat
//   fisherman/5/jungle_boat_emerald: unmapped jungle_boat
//   fisherman/5/oak_boat_emerald: unmapped oak_boat
//   fisherman/5/pufferfish_emerald: unmapped pufferfish
//   fisherman/5/spruce_boat_emerald: unmapped spruce_boat
//   fletcher/4/emerald_enchanted_bow: given_item_modifiers
//   fletcher/5/arrow_and_emerald_tipped_arrow: given_item_modifiers
//   fletcher/5/emerald_enchanted_crossbow: given_item_modifiers
//   leatherworker/1/emerald_dyed_leather_chestplate: given_item_modifiers
//   leatherworker/1/emerald_dyed_leather_leggings: given_item_modifiers
//   leatherworker/2/emerald_dyed_leather_boots: given_item_modifiers
//   leatherworker/2/emerald_dyed_leather_helmet: given_item_modifiers
//   leatherworker/3/emerald_dyed_leather_chestplate: given_item_modifiers
//   leatherworker/3/rabbit_hide_emerald: unmapped rabbit_hide
//   leatherworker/4/emerald_dyed_leather_horse_armor: given_item_modifiers
//   leatherworker/4/turtle_scute_emerald: unmapped turtle_scute
//   leatherworker/5/emerald_dyed_leather_helmet: given_item_modifiers
//   librarian/1/emerald_and_book_enchanted_book: given_item_modifiers
//   librarian/2/emerald_and_book_enchanted_book: given_item_modifiers
//   librarian/3/emerald_and_book_enchanted_book: given_item_modifiers
//   librarian/4/emerald_book_and_enchanted_book: given_item_modifiers
//   librarian/4/writable_book_emerald: unmapped writable_book
//   mason/1/clay_ball_emerald: unmapped clay_ball
//   mason/5/emerald_quartz_pillar: unmapped quartz_pillar
//   shepherd/4/emerald_black_banner: unmapped black_banner
//   shepherd/4/emerald_blue_banner: unmapped blue_banner
//   shepherd/4/emerald_brown_banner: unmapped brown_banner
//   shepherd/4/emerald_cyan_banner: unmapped cyan_banner
//   shepherd/4/emerald_gray_banner: unmapped gray_banner
//   shepherd/4/emerald_green_banner: unmapped green_banner
//   shepherd/4/emerald_light_blue_banner: unmapped light_blue_banner
//   shepherd/4/emerald_light_gray_banner: unmapped light_gray_banner
//   shepherd/4/emerald_lime_banner: unmapped lime_banner
//   shepherd/4/emerald_magenta_banner: unmapped magenta_banner
//   shepherd/4/emerald_orange_banner: unmapped orange_banner
//   shepherd/4/emerald_pink_banner: unmapped pink_banner
//   shepherd/4/emerald_purple_banner: unmapped purple_banner
//   shepherd/4/emerald_red_banner: unmapped red_banner
//   shepherd/4/emerald_white_banner: unmapped white_banner
//   shepherd/4/emerald_yellow_banner: unmapped yellow_banner
//   shepherd/5/emerald_painting: unmapped painting
//   toolsmith/1/emerald_stone_shovel: unmapped stone_shovel
//   toolsmith/3/emerald_enchanted_iron_axe: given_item_modifiers
//   toolsmith/3/emerald_enchanted_iron_pickaxe: given_item_modifiers
//   toolsmith/3/emerald_enchanted_iron_shovel: given_item_modifiers
//   toolsmith/4/emerald_enchanted_diamond_axe: given_item_modifiers
//   toolsmith/4/emerald_enchanted_diamond_shovel: given_item_modifiers
//   toolsmith/5/emerald_enchanted_diamond_pickaxe: given_item_modifiers
//   weaponsmith/1/emerald_enchanted_iron_sword: given_item_modifiers
//   weaponsmith/4/emerald_enchanted_diamond_axe: given_item_modifiers
//   weaponsmith/5/emerald_enchanted_diamond_sword: given_item_modifiers

use crate::trades::TradeDef;

pub(crate) const TRADES: &[TradeDef] = &[
    // armorer/1/emerald_iron_boots
    TradeDef { profession: "armorer", tier: 1, want: 317, want_n: 4, want2: None, give: 890, give_n: 1, max_uses: 12, xp: 1 },
    // armorer/1/emerald_iron_chestplate
    TradeDef { profession: "armorer", tier: 1, want: 317, want_n: 9, want2: None, give: 888, give_n: 1, max_uses: 12, xp: 1 },
    // armorer/1/emerald_iron_helmet
    TradeDef { profession: "armorer", tier: 1, want: 317, want_n: 5, want2: None, give: 887, give_n: 1, max_uses: 12, xp: 1 },
    // armorer/1/emerald_iron_leggings
    TradeDef { profession: "armorer", tier: 1, want: 317, want_n: 7, want2: None, give: 889, give_n: 1, max_uses: 12, xp: 1 },
    // armorer/3/diamond_emerald
    TradeDef { profession: "armorer", tier: 3, want: 316, want_n: 1, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 20 },
    // armorer/3/emerald_chainmail_chestplate
    TradeDef { profession: "armorer", tier: 3, want: 317, want_n: 4, want2: None, give: 886, give_n: 1, max_uses: 12, xp: 10 },
    // armorer/3/emerald_shield
    TradeDef { profession: "armorer", tier: 3, want: 317, want_n: 5, want2: None, give: 642, give_n: 1, max_uses: 12, xp: 10 },
    // armorer/3/lava_bucket_emerald
    TradeDef { profession: "armorer", tier: 3, want: 40, want_n: 1, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 20 },
    // butcher/1/chicken_emerald
    TradeDef { profession: "butcher", tier: 1, want: 165, want_n: 14, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // butcher/1/porkchop_emerald
    TradeDef { profession: "butcher", tier: 1, want: 907, want_n: 7, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // butcher/2/coal_emerald
    TradeDef { profession: "butcher", tier: 2, want: 325, want_n: 15, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // butcher/2/emerald_cooked_chicken
    TradeDef { profession: "butcher", tier: 2, want: 317, want_n: 1, want2: None, give: 166, give_n: 8, max_uses: 16, xp: 5 },
    // butcher/2/emerald_cooked_porkchop
    TradeDef { profession: "butcher", tier: 2, want: 317, want_n: 1, want2: None, give: 285, give_n: 5, max_uses: 16, xp: 5 },
    // butcher/3/beef_emerald
    TradeDef { profession: "butcher", tier: 3, want: 342, want_n: 10, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 20 },
    // butcher/3/mutton_emerald
    TradeDef { profession: "butcher", tier: 3, want: 286, want_n: 7, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 20 },
    // butcher/4/dried_kelp_block_emerald
    TradeDef { profession: "butcher", tier: 4, want: 436, want_n: 10, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 30 },
    // cartographer/1/emerald_map
    TradeDef { profession: "cartographer", tier: 1, want: 317, want_n: 7, want2: None, give: 306, give_n: 1, max_uses: 12, xp: 1 },
    // cartographer/1/paper_emerald
    TradeDef { profession: "cartographer", tier: 1, want: 304, want_n: 24, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 2 },
    // cartographer/2/glass_pane_emerald
    TradeDef { profession: "cartographer", tier: 2, want: 80, want_n: 11, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 10 },
    // cartographer/3/compass_emerald
    TradeDef { profession: "cartographer", tier: 3, want: 307, want_n: 1, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 20 },
    // cleric/1/emerald_redstone
    TradeDef { profession: "cleric", tier: 1, want: 317, want_n: 1, want2: None, give: 309, give_n: 2, max_uses: 12, xp: 1 },
    // cleric/1/rotten_flesh_emerald
    TradeDef { profession: "cleric", tier: 1, want: 293, want_n: 32, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // cleric/2/emerald_lapis_lazuli
    TradeDef { profession: "cleric", tier: 2, want: 317, want_n: 1, want2: None, give: 315, give_n: 1, max_uses: 12, xp: 5 },
    // cleric/2/gold_ingot_emerald
    TradeDef { profession: "cleric", tier: 2, want: 318, want_n: 3, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 10 },
    // cleric/3/emerald_glowstone
    TradeDef { profession: "cleric", tier: 3, want: 317, want_n: 4, want2: None, give: 28, give_n: 1, max_uses: 12, xp: 10 },
    // cleric/4/emerald_ender_pearl
    TradeDef { profession: "cleric", tier: 4, want: 317, want_n: 5, want2: None, give: 295, give_n: 1, max_uses: 12, xp: 15 },
    // cleric/5/emerald_experience_bottle
    TradeDef { profession: "cleric", tier: 5, want: 317, want_n: 3, want2: None, give: 917, give_n: 1, max_uses: 12, xp: 30 },
    // cleric/5/nether_wart_emerald
    TradeDef { profession: "cleric", tier: 5, want: 502, want_n: 22, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 30 },
    // farmer/1/beetroot_emerald
    TradeDef { profession: "farmer", tier: 1, want: 345, want_n: 15, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // farmer/1/carrot_emerald
    TradeDef { profession: "farmer", tier: 1, want: 344, want_n: 22, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // farmer/1/emerald_bread
    TradeDef { profession: "farmer", tier: 1, want: 317, want_n: 1, want2: None, give: 163, give_n: 6, max_uses: 16, xp: 1 },
    // farmer/1/potato_emerald
    TradeDef { profession: "farmer", tier: 1, want: 343, want_n: 26, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // farmer/1/wheat_emerald
    TradeDef { profession: "farmer", tier: 1, want: 493, want_n: 20, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // farmer/2/emerald_apple
    TradeDef { profession: "farmer", tier: 2, want: 317, want_n: 1, want2: None, give: 162, give_n: 4, max_uses: 16, xp: 5 },
    // farmer/2/emerald_pumpkin_pie
    TradeDef { profession: "farmer", tier: 2, want: 317, want_n: 1, want2: None, give: 281, give_n: 4, max_uses: 12, xp: 5 },
    // farmer/2/pumpkin_emerald
    TradeDef { profession: "farmer", tier: 2, want: 448, want_n: 6, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 10 },
    // farmer/3/emerald_cookie
    TradeDef { profession: "farmer", tier: 3, want: 317, want_n: 3, want2: None, give: 167, give_n: 18, max_uses: 12, xp: 10 },
    // farmer/3/melon_emerald
    TradeDef { profession: "farmer", tier: 3, want: 447, want_n: 4, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 20 },
    // farmer/5/emerald_glistening_melon_slice
    TradeDef { profession: "farmer", tier: 5, want: 317, want_n: 4, want2: None, give: 913, give_n: 3, max_uses: 12, xp: 30 },
    // farmer/5/emerald_golden_carrot
    TradeDef { profession: "farmer", tier: 5, want: 317, want_n: 3, want2: None, give: 908, give_n: 3, max_uses: 12, xp: 30 },
    // fisherman/1/coal_emerald
    TradeDef { profession: "fisherman", tier: 1, want: 325, want_n: 10, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // fisherman/1/raw_cod_and_emerald_cooked_cod
    TradeDef { profession: "fisherman", tier: 1, want: 288, want_n: 6, want2: Some((317, 1)), give: 289, give_n: 6, max_uses: 16, xp: 1 },
    // fisherman/1/string_emerald
    TradeDef { profession: "fisherman", tier: 1, want: 301, want_n: 20, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // fisherman/2/cod_emerald
    TradeDef { profession: "fisherman", tier: 2, want: 288, want_n: 15, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 10 },
    // fisherman/2/emerald_campfire
    TradeDef { profession: "fisherman", tier: 2, want: 317, want_n: 2, want2: None, give: 979, give_n: 1, max_uses: 12, xp: 5 },
    // fisherman/2/salmon_and_emerald_cooked_salmon
    TradeDef { profession: "fisherman", tier: 2, want: 290, want_n: 6, want2: Some((317, 1)), give: 291, give_n: 6, max_uses: 16, xp: 5 },
    // fisherman/3/salmon_emerald
    TradeDef { profession: "fisherman", tier: 3, want: 290, want_n: 13, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 20 },
    // fletcher/1/emerald_arrow
    TradeDef { profession: "fletcher", tier: 1, want: 317, want_n: 1, want2: None, give: 643, give_n: 16, max_uses: 12, xp: 1 },
    // fletcher/1/gravel_and_emerald_flint
    TradeDef { profession: "fletcher", tier: 1, want: 644, want_n: 10, want2: Some((317, 1)), give: 327, give_n: 10, max_uses: 12, xp: 1 },
    // fletcher/1/stick_emerald
    TradeDef { profession: "fletcher", tier: 1, want: 302, want_n: 32, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // fletcher/2/emerald_bow
    TradeDef { profession: "fletcher", tier: 2, want: 317, want_n: 2, want2: None, give: 638, give_n: 1, max_uses: 12, xp: 5 },
    // fletcher/2/flint_emerald
    TradeDef { profession: "fletcher", tier: 2, want: 327, want_n: 26, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 10 },
    // fletcher/3/emerald_crossbow
    TradeDef { profession: "fletcher", tier: 3, want: 317, want_n: 3, want2: None, give: 639, give_n: 1, max_uses: 12, xp: 10 },
    // fletcher/3/string_emerald
    TradeDef { profession: "fletcher", tier: 3, want: 301, want_n: 14, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 20 },
    // fletcher/4/feather_emerald
    TradeDef { profession: "fletcher", tier: 4, want: 19, want_n: 24, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 30 },
    // fletcher/5/tripwire_hook_emerald
    TradeDef { profession: "fletcher", tier: 5, want: 969, want_n: 8, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 30 },
    // leatherworker/1/leather_emerald
    TradeDef { profession: "leatherworker", tier: 1, want: 303, want_n: 6, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // leatherworker/2/flint_emerald
    TradeDef { profession: "leatherworker", tier: 2, want: 327, want_n: 26, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 10 },
    // leatherworker/5/emerald_saddle
    TradeDef { profession: "leatherworker", tier: 5, want: 317, want_n: 6, want2: None, give: 926, give_n: 1, max_uses: 12, xp: 30 },
    // librarian/1/emerald_bookshelf
    TradeDef { profession: "librarian", tier: 1, want: 317, want_n: 9, want2: None, give: 718, give_n: 1, max_uses: 12, xp: 1 },
    // librarian/1/paper_emerald
    TradeDef { profession: "librarian", tier: 1, want: 304, want_n: 24, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // librarian/2/book_emerald
    TradeDef { profession: "librarian", tier: 2, want: 305, want_n: 4, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 10 },
    // librarian/2/emerald_lantern
    TradeDef { profession: "librarian", tier: 2, want: 317, want_n: 1, want2: None, give: 60, give_n: 1, max_uses: 12, xp: 5 },
    // librarian/3/emerald_glass
    TradeDef { profession: "librarian", tier: 3, want: 317, want_n: 1, want2: None, give: 9, give_n: 4, max_uses: 12, xp: 10 },
    // librarian/3/ink_sac_emerald
    TradeDef { profession: "librarian", tier: 3, want: 348, want_n: 5, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 20 },
    // librarian/4/emerald_clock
    TradeDef { profession: "librarian", tier: 4, want: 317, want_n: 5, want2: None, give: 308, give_n: 1, max_uses: 12, xp: 15 },
    // librarian/4/emerald_compass
    TradeDef { profession: "librarian", tier: 4, want: 317, want_n: 4, want2: None, give: 307, give_n: 1, max_uses: 12, xp: 15 },
    // librarian/5/emerald_red_candle
    TradeDef { profession: "librarian", tier: 5, want: 317, want_n: 3, want2: None, give: 747, give_n: 1, max_uses: 12, xp: 30 },
    // librarian/5/emerald_yellow_candle
    TradeDef { profession: "librarian", tier: 5, want: 317, want_n: 3, want2: None, give: 737, give_n: 1, max_uses: 12, xp: 30 },
    // mason/1/emerald_brick
    TradeDef { profession: "mason", tier: 1, want: 317, want_n: 1, want2: None, give: 675, give_n: 10, max_uses: 16, xp: 1 },
    // mason/2/emerald_chiseled_stone_bricks
    TradeDef { profession: "mason", tier: 2, want: 317, want_n: 1, want2: None, give: 365, give_n: 4, max_uses: 16, xp: 5 },
    // mason/2/stone_emerald
    TradeDef { profession: "mason", tier: 2, want: 1, want_n: 20, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 10 },
    // mason/3/andesite_emerald
    TradeDef { profession: "mason", tier: 3, want: 85, want_n: 16, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 20 },
    // mason/3/diorite_emerald
    TradeDef { profession: "mason", tier: 3, want: 84, want_n: 16, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 20 },
    // mason/3/emerald_dripstone_block
    TradeDef { profession: "mason", tier: 3, want: 317, want_n: 1, want2: None, give: 985, give_n: 4, max_uses: 16, xp: 10 },
    // mason/3/emerald_polished_andesite
    TradeDef { profession: "mason", tier: 3, want: 317, want_n: 1, want2: None, give: 355, give_n: 4, max_uses: 16, xp: 10 },
    // mason/3/emerald_polished_diorite
    TradeDef { profession: "mason", tier: 3, want: 317, want_n: 1, want2: None, give: 354, give_n: 4, max_uses: 16, xp: 10 },
    // mason/3/emerald_polished_granite
    TradeDef { profession: "mason", tier: 3, want: 317, want_n: 1, want2: None, give: 353, give_n: 4, max_uses: 16, xp: 10 },
    // mason/3/granite_emerald
    TradeDef { profession: "mason", tier: 3, want: 83, want_n: 16, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 20 },
    // mason/4/emerald_black_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 700, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_black_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 684, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_blue_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 696, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_blue_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 431, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_brown_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 697, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_brown_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 432, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_cyan_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 694, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_cyan_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 429, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_gray_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 692, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_gray_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 427, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_green_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 698, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_green_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 433, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_light_blue_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 688, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_light_blue_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 423, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_light_gray_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 693, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_light_gray_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 428, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_lime_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 690, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_lime_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 425, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_magenta_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 687, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_magenta_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 422, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_orange_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 686, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_orange_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 421, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_pink_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 691, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_pink_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 426, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_purple_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 695, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_purple_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 430, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_red_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 699, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_red_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 434, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_white_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 685, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_white_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 420, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_yellow_glazed_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 689, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/emerald_yellow_terracotta
    TradeDef { profession: "mason", tier: 4, want: 317, want_n: 1, want2: None, give: 424, give_n: 1, max_uses: 12, xp: 15 },
    // mason/4/quartz_emerald
    TradeDef { profession: "mason", tier: 4, want: 326, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 30 },
    // mason/5/emerald_quartz_block
    TradeDef { profession: "mason", tier: 5, want: 317, want_n: 1, want2: None, give: 837, give_n: 1, max_uses: 12, xp: 30 },
    // shepherd/1/black_wool_emerald
    TradeDef { profession: "shepherd", tier: 1, want: 108, want_n: 18, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // shepherd/1/brown_wool_emerald
    TradeDef { profession: "shepherd", tier: 1, want: 107, want_n: 18, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // shepherd/1/emerald_shears
    TradeDef { profession: "shepherd", tier: 1, want: 317, want_n: 2, want2: None, give: 640, give_n: 1, max_uses: 12, xp: 1 },
    // shepherd/1/gray_wool_emerald
    TradeDef { profession: "shepherd", tier: 1, want: 103, want_n: 18, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // shepherd/1/white_wool_emerald
    TradeDef { profession: "shepherd", tier: 1, want: 11, want_n: 18, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 2 },
    // shepherd/2/black_dye_emerald
    TradeDef { profession: "shepherd", tier: 2, want: 661, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 10 },
    // shepherd/2/emerald_black_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 232, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_black_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 108, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_blue_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 228, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_blue_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 91, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_brown_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 229, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_brown_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 107, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_cyan_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 226, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_cyan_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 105, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_gray_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 224, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_gray_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 103, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_green_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 230, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_green_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 94, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_light_blue_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 220, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_light_blue_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 100, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_light_gray_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 225, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_light_gray_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 104, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_lime_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 222, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_lime_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 101, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_magenta_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 219, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_magenta_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 99, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_orange_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 218, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_orange_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 98, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_pink_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 223, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_pink_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 102, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_purple_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 227, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_purple_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 106, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_red_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 231, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_red_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 92, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_white_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 217, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_white_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 11, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_yellow_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 221, give_n: 4, max_uses: 16, xp: 5 },
    // shepherd/2/emerald_yellow_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 93, give_n: 1, max_uses: 16, xp: 5 },
    // shepherd/2/gray_dye_emerald
    TradeDef { profession: "shepherd", tier: 2, want: 660, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 10 },
    // shepherd/2/light_blue_dye_emerald
    TradeDef { profession: "shepherd", tier: 2, want: 669, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 10 },
    // shepherd/2/lime_dye_emerald
    TradeDef { profession: "shepherd", tier: 2, want: 666, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 10 },
    // shepherd/2/white_dye_emerald
    TradeDef { profession: "shepherd", tier: 2, want: 658, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 10 },
    // shepherd/3/emerald_black_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 591, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_blue_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 587, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_brown_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 588, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_cyan_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 585, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_gray_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 583, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_green_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 589, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_light_blue_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 579, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_light_gray_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 584, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_lime_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 581, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_magenta_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 578, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_orange_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 577, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_pink_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 582, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_purple_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 586, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_red_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 590, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_white_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 576, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/emerald_yellow_bed
    TradeDef { profession: "shepherd", tier: 3, want: 317, want_n: 3, want2: None, give: 580, give_n: 1, max_uses: 12, xp: 10 },
    // shepherd/3/light_gray_dye_emerald
    TradeDef { profession: "shepherd", tier: 3, want: 659, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 20 },
    // shepherd/3/orange_dye_emerald
    TradeDef { profession: "shepherd", tier: 3, want: 664, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 20 },
    // shepherd/3/pink_dye_emerald
    TradeDef { profession: "shepherd", tier: 3, want: 673, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 20 },
    // shepherd/3/red_dye_emerald
    TradeDef { profession: "shepherd", tier: 3, want: 663, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 20 },
    // shepherd/3/yellow_dye_emerald
    TradeDef { profession: "shepherd", tier: 3, want: 665, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 20 },
    // shepherd/4/blue_dye_emerald
    TradeDef { profession: "shepherd", tier: 4, want: 670, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 30 },
    // shepherd/4/brown_dye_emerald
    TradeDef { profession: "shepherd", tier: 4, want: 662, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 30 },
    // shepherd/4/cyan_dye_emerald
    TradeDef { profession: "shepherd", tier: 4, want: 668, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 30 },
    // shepherd/4/green_dye_emerald
    TradeDef { profession: "shepherd", tier: 4, want: 667, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 30 },
    // shepherd/4/magenta_dye_emerald
    TradeDef { profession: "shepherd", tier: 4, want: 672, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 30 },
    // shepherd/4/purple_dye_emerald
    TradeDef { profession: "shepherd", tier: 4, want: 671, want_n: 12, want2: None, give: 317, give_n: 1, max_uses: 16, xp: 30 },
    // toolsmith/1/emerald_stone_axe
    TradeDef { profession: "toolsmith", tier: 1, want: 317, want_n: 1, want2: None, give: 865, give_n: 1, max_uses: 12, xp: 1 },
    // toolsmith/1/emerald_stone_hoe
    TradeDef { profession: "toolsmith", tier: 1, want: 317, want_n: 1, want2: None, give: 333, give_n: 1, max_uses: 12, xp: 1 },
    // toolsmith/1/emerald_stone_pickaxe
    TradeDef { profession: "toolsmith", tier: 1, want: 317, want_n: 1, want2: None, give: 866, give_n: 1, max_uses: 12, xp: 1 },
    // toolsmith/3/emerald_diamond_hoe
    TradeDef { profession: "toolsmith", tier: 3, want: 317, want_n: 4, want2: None, give: 336, give_n: 1, max_uses: 3, xp: 10 },
    // toolsmith/3/flint_emerald
    TradeDef { profession: "toolsmith", tier: 3, want: 327, want_n: 30, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 20 },
    // toolsmith/4/diamond_emerald
    TradeDef { profession: "toolsmith", tier: 4, want: 316, want_n: 1, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 30 },
    // weaponsmith/1/emerald_iron_axe
    TradeDef { profession: "weaponsmith", tier: 1, want: 317, want_n: 3, want2: None, give: 873, give_n: 1, max_uses: 12, xp: 1 },
    // weaponsmith/3/flint_emerald
    TradeDef { profession: "weaponsmith", tier: 3, want: 327, want_n: 24, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 20 },
    // weaponsmith/4/diamond_emerald
    TradeDef { profession: "weaponsmith", tier: 4, want: 316, want_n: 1, want2: None, give: 317, give_n: 1, max_uses: 12, xp: 30 },
];
