//! A padlock over the icon of every item the player cannot fully use yet: one of its actions needs a skill level
//! they do not have. The icon is dimmed and the padlock sits in its bottom-left corner, clear of the stack count.
//! Drawn from the cached requirements; an item not answered yet is asked for and gets its padlock once the answer
//! is in.

use horizon_client::mod_api::{Host, HudBatch, ItemStack};

use super::cache::{Corner, with};
use super::paint::{PADLOCK_SIZE, padlock};

/// An item icon is 16 pixels square.
const ICON: f32 = 16.0;
/// A translucent black over a locked item's icon, so it reads as out of reach at a glance.
const DIMMED: u32 = 0x6608_090C;

/// Dims the item's icon at `x`, `y` and draws the padlock over it when the player lacks a level one of its
/// actions needs.
pub fn draw_lock_badge(host: &dyn Host, batch: &mut HudBatch, stack: &ItemStack, x: f32, y: f32) {
    let settings = with(|cache| cache.settings.clone());
    if !settings.lock_icon || !locked(stack) {
        return;
    }
    if settings.dim_locked {
        host.fill(batch, [x, y, ICON, ICON], DIMMED);
    }
    let [w, h] = PADLOCK_SIZE;
    let corner = match settings.lock_corner {
        Corner::BottomLeft => [x, y + ICON - h],
        Corner::TopLeft => [x, y],
        Corner::TopRight => [x + ICON - w, y],
        Corner::BottomRight => [x + ICON - w, y + ICON - h],
    };
    padlock(host, batch, corner);
}

fn locked(stack: &ItemStack) -> bool {
    with(|cache| cache.lacks(stack, |_| true))
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::HudBatch;
    use skills_core::defs::{Action, SkillDef};
    use skills_core::locks::Requirements;
    use skills_core::progress::{PlayerState, SkillProgress};
    use skills_core::registry::Defs;
    use skills_core::wire::{ClientDefs, ToServer};

    use crate::client::cache::with;
    use crate::test_host::{TestHost, exclusive, mods};

    const MINING: &str = "reskilled:mining";
    const AT: (f32, f32) = (40.0, 100.0);

    /// Mining known; the diamond pickaxe needs Mining 16, the iron one Mining 8, the stick nothing; the player is at
    /// Mining 10.
    fn known() {
        let mining = SkillDef {
            id: MINING.into(),
            priority: 0,
            name: "n".into(),
            icon: "minecraft:stone".into(),
            max_level: None,
            cost: None,
            points_every: None,
            icon_levels: Default::default(),
        };
        let needs = |level: u32| Requirements {
            by_action: [(Action::Break, [(MINING.to_string(), level)].into())].into(),
        };
        let mut state = PlayerState::default();
        state.skills.insert(
            MINING.into(),
            SkillProgress {
                level: 10,
                ..SkillProgress::default()
            },
        );
        with(|cache| {
            let defs = Defs {
                skills: [(MINING.into(), mining)].into(),
                ..Defs::default()
            };
            cache.defs = Some(ClientDefs {
                defs,
                labels: Default::default(),
            });
            cache
                .reqs
                .insert("minecraft:diamond_pickaxe".into(), needs(16));
            cache.reqs.insert("minecraft:iron_pickaxe".into(), needs(8));
            cache
                .reqs
                .insert("minecraft:stick".into(), Requirements::default());
            cache.state = state;
        });
    }

    fn decorate(host: &TestHost, item: &str) -> Vec<([f32; 4], u32)> {
        host.filled.borrow_mut().clear();
        let stack = horizon_client::mod_api::ItemStack {
            item: item.into(),
            count: 1,
            damage: 0,
            extra: None,
        };
        let mut batch = HudBatch::new([427, 240]);
        horizon_client::mod_api::dispatch_item_decorations(
            host,
            &mods(),
            &stack,
            AT.0,
            AT.1,
            &mut batch,
        );
        host.filled.borrow().clone()
    }

    #[test]
    fn locked_items_get_a_lock_badge() {
        // Given a player at Mining 10
        let _serial = exclusive();
        let host = TestHost::default();
        known();

        // When the diamond pickaxe's icon is drawn
        let drawn = decorate(&host, "minecraft:diamond_pickaxe");

        // Then the whole icon is dimmed first, and the padlock is drawn over it in the bottom-left corner
        assert_eq!(drawn[0].0, [AT.0, AT.1, 16.0, 16.0]);
        assert_eq!(drawn[0].1 >> 24, 0x66);
        assert!(drawn.len() > 10, "{drawn:?}");
        for ([x, y, w, h], colour) in &drawn[1..] {
            let in_corner =
                *x >= AT.0 && *y >= AT.1 + 6.0 && x + w <= AT.0 + 8.0 && y + h <= AT.1 + 16.0;
            assert!(in_corner && colour >> 24 == 0xFF, "{drawn:?}");
        }
    }

    #[test]
    fn the_padlock_follows_the_players_settings() {
        // Given a player at Mining 10 and the diamond pickaxe they cannot use
        let _serial = exclusive();
        let host = TestHost::default();
        known();
        let set = |change: fn(&mut crate::client::cache::ClientSettings)| {
            with(|cache| {
                cache.settings = Default::default();
                change(&mut cache.settings);
            });
        };

        // When its icon is drawn with the padlock off, in the top-right corner, and without dimming
        set(|s| s.lock_icon = false);
        let off = decorate(&host, "minecraft:diamond_pickaxe");
        set(|s| s.lock_corner = crate::client::cache::Corner::TopRight);
        let top_right = decorate(&host, "minecraft:diamond_pickaxe");
        set(|s| s.dim_locked = false);
        let undimmed = decorate(&host, "minecraft:diamond_pickaxe");

        // Then nothing is drawn when off, the padlock sits in the top-right 8 by 10 when moved, and no dark square
        // is drawn when dimming is off
        assert!(off.is_empty());
        for ([x, y, w, h], _) in &top_right[1..] {
            assert!(
                *x >= AT.0 + 8.0 && x + w <= AT.0 + 16.0 && *y >= AT.1 && y + h <= AT.1 + 10.0,
                "{top_right:?}"
            );
        }
        assert!(
            undimmed
                .iter()
                .all(|([_, _, w, h], _)| *w < 16.0 || *h < 16.0)
        );
    }

    #[test]
    fn usable_and_unknown_items_get_none_but_unknown_ones_are_asked_for() {
        // Given a player at Mining 10
        let _serial = exclusive();
        let mut host = TestHost::default();
        known();

        // When icons are drawn for an item they can use, one with no lock, and one the server has not answered for,
        // then a tick passes
        let iron = decorate(&host, "minecraft:iron_pickaxe");
        let stick = decorate(&host, "minecraft:stick");
        let sword = decorate(&host, "minecraft:diamond_sword");
        horizon_client::mod_api::dispatch_tick(&mut host, &mods());

        // Then none gets a padlock, and the unknown one is asked for
        assert_eq!((iron.len(), stick.len(), sword.len()), (0, 0, 0));
        let asked: Vec<ToServer> = host
            .sent_to_server
            .iter()
            .map(|(_, data)| serde_json::from_slice(data).unwrap())
            .collect();
        assert_eq!(
            asked,
            vec![ToServer::Need {
                items: vec!["minecraft:diamond_sword".into()]
            }]
        );
    }

    fn starts_using(host: &mut TestHost, item: &str, entity: Option<i32>) -> bool {
        let stack = horizon_client::mod_api::ItemStack {
            item: item.into(),
            count: 1,
            damage: 0,
            extra: None,
        };
        let (mut duration, mut canceled) = (72_000, false);
        horizon_client::mod_api::dispatch_living_entity_use_item_start(
            host,
            &mods(),
            &stack,
            entity,
            horizon_client::mod_api::Hand::Main,
            &mut duration,
            &mut canceled,
        );
        !canceled
    }

    /// The shield needs Mining 16 to use, which the player at Mining 10 lacks.
    fn shield_locked() {
        with(|cache| {
            let use_lock = Requirements {
                by_action: [(Action::Use, [(MINING.to_string(), 16)].into())].into(),
            };
            cache.reqs.insert("minecraft:shield".into(), use_lock);
        });
    }

    #[test]
    fn a_locked_use_never_starts_on_the_client() {
        // Given a player at Mining 10, a shield needing Mining 16 to use, the diamond pickaxe (locked only for
        // breaking) and an item the server has not answered for
        let _serial = exclusive();
        let mut host = TestHost::default();
        known();
        shield_locked();

        // When the client is about to start using each (raising the shield)
        let started = [
            "minecraft:shield",
            "minecraft:diamond_pickaxe",
            "minecraft:bow",
        ]
        .map(|item| starts_using(&mut host, item, None));

        // Then only the shield's use is stopped before its animation; the server decides what the client cannot
        assert_eq!(started, [false, true, true]);
    }

    #[test]
    fn only_the_windows_own_player_is_stopped() {
        // Given a shield the player may not use
        let _serial = exclusive();
        let mut host = TestHost::default();
        known();
        shield_locked();

        // When a mob (it has an entity id) starts using one, and when a server's game hears the player start
        let mob = starts_using(&mut host, "minecraft:shield", Some(77));
        let mut server = TestHost {
            server: true,
            ..TestHost::default()
        };
        let on_server = starts_using(&mut server, "minecraft:shield", None);

        // Then neither is stopped here: a mob is not this mod's business, and the server refuses the player's use
        // in `use_item`
        assert!(mob && on_server);
    }

    #[test]
    fn nothing_is_locked_while_locks_are_off_for_the_player() {
        // Given a creative player the server says locks are off for
        let _serial = exclusive();
        let mut host = TestHost::default();
        known();
        shield_locked();
        with(|cache| cache.exempt = true);

        // When a locked item's icon is drawn and they raise the shield
        let badge = decorate(&host, "minecraft:diamond_pickaxe");
        let started = starts_using(&mut host, "minecraft:shield", None);

        // Then there is no padlock and the use starts
        assert!(badge.is_empty() && started);
    }
}
