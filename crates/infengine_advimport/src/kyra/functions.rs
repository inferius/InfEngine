//! A45.1: the engine's function table, by number.
//!
//! A script's calls are numbers into a table the interpreter holds, so
//! `Call 45` is only meaningful once you know slot 45 moves the player
//! to another scene. Without the table, half of every decompiled room
//! reads `kyra:55` and the editor cannot tell a sound effect from a
//! cursor tweak.
//!
//! The numbering is a property of Kyrandia's data. What each slot does
//! is public knowledge about the format; what we do with it - which
//! ones become actions, which are dropped as machinery, which stay
//! honestly untranslated - is the part written here.
//!
//! Nine slots were named before this, and two of them were wrong: 24
//! was read as "start an animation" when it starts music, and 42 as
//! "stop an animation" when it closes a file. Both had confident
//! comments. The lesson is not to name a slot from what a call looks
//! like it does in the one room you happened to read.

/// Names of the engine's functions, in table order.
///
/// The index is the number a `Call` carries. A gap at the end is not an
/// error: scripts never call past what the table holds.
const NAMES: &[&str] = &[
    "magicInMouseItem",
    "characterSays",
    "delay",
    "drawSceneAnimShape",
    "queryGameFlag",
    "setGameFlag",
    "resetGameFlag",
    "runNpcScript",
    "setSpecialExitList",
    "blockInWalkableRegion",
    "blockOutWalkableRegion",
    "walkPlayerToPoint",
    "dropItemInScene",
    "drawAnimShapeIntoScene",
    "setHandItem",
    "savePageToDisk",
    "sceneAnimOn",
    "sceneAnimOff",
    "getElapsedSeconds",
    "mouseIsPointer",
    "removeHandItem",
    "runSceneAnimUntilDone",
    "fadeSpecialPalette",
    "playSoundEffect",
    "playWanderScoreViaMap",
    "phaseInSameScene",
    "setScenePhasingFlag",
    "resetScenePhasingFlag",
    "queryScenePhasingFlag",
    "sceneToDirection",
    "setBirthstoneGem",
    "placeItemInGenericMapScene",
    "setBrandonStatusBit",
    "delaySecs",
    "getCharacterScene",
    "runNpcSubscript",
    "magicOutMouseItem",
    "internalAnimOn",
    "forceBrandonToNormal",
    "poisonDeathNow",
    "setScaleMode",
    "openWsaFile",
    "closeWsaFile",
    "runWsaFromBeginningToEnd",
    "displayWsaFrame",
    "enterNewScene",
    "setSpecialEnterXAndY",
    "runWsaFrames",
    "popBrandonIntoScene",
    "restoreAllObjectBackgrounds",
    "setCustomPaletteRange",
    "loadPageFromDisk",
    "customPrintTalkString",
    "restoreCustomPrintBackground",
    "hideMouse",
    "showMouse",
    "getCharacterX",
    "getCharacterY",
    "setCharacterFacing",
    "copyWsaRegion",
    "printText",
    "getRand",
    "loadSoundFile",
    "displayWsaFrameOnHidPage",
    "displayWsaSequentialFrames",
    "refreshCharacter",
    "internalAnimOff",
    "changeCharactersXAndY",
    "clearSceneAnimatorBeacon",
    "querySceneAnimatorBeacon",
    "refreshSceneAnimator",
    "placeItemInOffScene",
    "wipeDownMouseItem",
    "placeCharacterInOtherScene",
    "getKey",
    "specificItemInInventory",
    "popMobileNpcIntoScene",
    "mobileCharacterInScene",
    "hideMobileCharacter",
    "unhideMobileCharacter",
    "setCharacterLocation",
    "walkCharacterToPoint",
    "displayBrynnsNote",
    "removeBrynnsNote",
    "setLogicPage",
    "fatPrint",
    "preserveAllObjectBackgrounds",
    "updateSceneAnimations",
    "sceneAnimationActive",
    "setCharacterMovementDelay",
    "getCharacterFacing",
    "scrollSceneAndMasksRight",
    "dispelMagicAnimation",
    "findBrightestFireberry",
    "setFireberryGlowPalette",
    "setDeathHandler",
    "drinkPotionAnimation",
    "makeAmuletAppear",
    "drawItemShapeIntoScene",
    "setCharacterCurrentFrame",
    "waitForConfirmationMouseClick",
    "pageFlip",
    "setSceneFile",
    "getItemInMarbleVase",
    "setItemInMarbleVase",
    "addItemToInventory",
    "intPrint",
    "shakeScreen",
    "createAmuletJewel",
    "setSceneAnimCurrXy",
    "poisonBrandonAndRemaps",
    "fillFlaskWithWater",
    "getCharacterMovementDelay",
    "getBirthstoneGem",
    "queryBrandonStatusBit",
    "playFluteAnimation",
    "playWinterScrollSequence",
    "getIdolGem",
    "setIdolGem",
    "totalItemsInScene",
    "restoreBrandonsMovementDelay",
    "setMousePos",
    "getMouseState",
    "setEntranceMouseCursorTrack",
    "itemAppearsOnGround",
    "setNoDrawShapesFlag",
    "fadeEntirePalette",
    "itemOnGroundHere",
    "queryCauldronState",
    "setCauldronState",
    "queryCrystalState",
    "setCrystalState",
    "setPaletteRange",
    "shrinkBrandonDown",
    "growBrandonUp",
    "setBrandonScaleXAndY",
    "resetScaleMode",
    "getScaleDepthTableValue",
    "setScaleDepthTableValue",
    "message",
    "checkClickOnNpc",
    "getFoyerItem",
    "setFoyerItem",
    "setNoItemDropRegion",
    "walkMalcolmOn",
    "passiveProtection",
    "setPlayingLoop",
    "brandonToStoneSequence",
    "brandonHealingSequence",
    "protectCommandLine",
    "pauseMusicSeconds",
    "resetMaskRegion",
    "setPaletteChangeFlag",
    "fillRect",
    "vocUnload",
    "vocLoad",
];

/// What a call is, for the purpose of importing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Becomes one of our actions.
    Action,
    /// Describes how the original drew the screen. Dropped on purpose.
    ///
    /// Page flips, palette ranges, background preservation, logic
    /// pages: 1992 machinery our engine has no equivalent for. Carrying
    /// them through would bury the genuinely unresolved calls under
    /// thousands of irrelevant ones - and discarded is not the same
    /// claim as unrecognised, so they are counted apart.
    Plumbing,
    /// Asks the engine something rather than telling it something.
    ///
    /// A query is half of a condition, and a condition is not a step.
    /// Until the decompiler reconstructs expressions these cannot be
    /// placed, and listing them as steps would read as a room doing
    /// something when it is only deciding something.
    Query,
    /// Real behaviour we have no action for yet.
    Unmapped,
}

/// Slots that draw or manage the original's screen.
///
/// The cursor and the character redraw are in here on the evidence of
/// how often they appear: 375 calls to show a cursor, hide it, and
/// repaint the player, none of which our engine needs told.
const PLUMBING: &[i32] = &[
    3, 13, 15, 22, 41, 42, 49, 50, 51, 53, 54, 55, 59, 63, 65, 68, 70, 84,
    86, 87, 94, 98, 101, 125, 126, 132, 137, 138, 149, 151, 152, 153, 154,
    155,
];

/// Slots that answer a question.
const QUERY: &[i32] = &[
    4, 18, 19, 28, 29, 34, 56, 57, 61, 69, 74, 75, 77, 88, 90, 103, 112, 113,
    114, 117, 119, 122, 127, 128, 130, 141,
];

/// The engine function at `number`, if the table has one.
pub fn name(number: i32) -> Option<&'static str> {
    usize::try_from(number).ok().and_then(|n| NAMES.get(n)).copied()
}

/// How this call should be imported.
pub fn kind(number: i32, becomes_action: bool) -> Kind {
    if becomes_action {
        Kind::Action
    } else if PLUMBING.contains(&number) {
        Kind::Plumbing
    } else if QUERY.contains(&number) {
        Kind::Query
    } else {
        Kind::Unmapped
    }
}

/// A call written the way it should read in the editor.
///
/// The name when the table has one, the number when it does not.
/// `kyra:55` tells a reader nothing; `kyra:showMouse` tells them it can
/// be ignored, which is most of what they need.
pub fn label(number: i32) -> String {
    match name(number) {
        Some(name) => format!("kyra:{name}"),
        None => format!("kyra:{number}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_the_length_the_game_uses() {
        // The last slot the retail scripts can reach. A table shorter
        // than the highest call would silently start returning numbers
        // again for the calls furthest down it.
        assert_eq!(NAMES.len(), 156);
        assert_eq!(name(0), Some("magicInMouseItem"));
        assert_eq!(name(155), Some("vocLoad"));
        assert_eq!(name(156), None);
        assert_eq!(name(-1), None);
    }

    #[test]
    fn the_slots_that_were_read_wrong_are_named_right() {
        // 24 was imported as "start an animation" for a year. It starts
        // music. 42 was "stop an animation"; it closes a file.
        assert_eq!(name(24), Some("playWanderScoreViaMap"));
        assert_eq!(name(42), Some("closeWsaFile"));
        assert_eq!(name(16), Some("sceneAnimOn"));
        assert_eq!(name(17), Some("sceneAnimOff"));
        // And these were listed as machinery under wrong descriptions.
        assert_eq!(name(58), Some("setCharacterFacing"), "a set, not a read");
        assert_eq!(name(64), Some("displayWsaSequentialFrames"));
    }

    #[test]
    fn a_call_reads_as_a_name_rather_than_a_number() {
        assert_eq!(label(45), "kyra:enterNewScene");
        assert_eq!(label(999), "kyra:999");
    }

    #[test]
    fn machinery_and_questions_are_told_apart_from_unmapped_behaviour() {
        assert_eq!(kind(101, false), Kind::Plumbing, "pageFlip");
        assert_eq!(kind(4, false), Kind::Query, "queryGameFlag");
        assert_eq!(kind(107, false), Kind::Unmapped, "shakeScreen is real");
        assert_eq!(kind(1, true), Kind::Action);
    }

    #[test]
    fn nothing_is_both_machinery_and_a_question() {
        for number in PLUMBING {
            assert!(!QUERY.contains(number), "{number} is in both lists");
        }
    }

    #[test]
    fn every_classified_slot_exists_in_the_table() {
        for number in PLUMBING.iter().chain(QUERY) {
            assert!(name(*number).is_some(), "slot {number} is past the table");
        }
    }
}
