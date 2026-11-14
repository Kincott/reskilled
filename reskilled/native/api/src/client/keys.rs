//! The mod's keys: the one that opens the skills screen (K by default, rebindable as the mod's key `open`,
//! `key.reskilled.open` in `horizon-mods.txt`) and the one that opens a perk's guide from the skill view (G, `guide`).

use horizon_client::mod_api::Host;

use crate::MOD_ID;

/// The mod key's name.
pub const OPEN_KEY: &str = "open";
/// Vanilla's name for K.
pub const DEFAULT_OPEN_KEY: &str = "key.keyboard.k";
/// The guide key's name, and vanilla's name for G.
pub const GUIDE_KEY: &str = "guide";
pub const DEFAULT_GUIDE_KEY: &str = "key.keyboard.g";

/// The open key going down opens the screen while the world is played (no other screen over it), and closes it
/// while it is the one open. The guide key going down on the skill view, with the mouse over a perk, opens that
/// perk's guide.
pub fn on_key(host: &mut dyn Host, key: &str, pressed: bool) {
    if !pressed {
        return;
    }
    let on_screen = host.mod_screen() == Some(MOD_ID);
    if on_screen
        && key == host.mod_key(MOD_ID, GUIDE_KEY, DEFAULT_GUIDE_KEY)
        && super::guide::open_hovered()
    {
        super::guide::read_attributes(host);
        return;
    }
    if key != host.mod_key(MOD_ID, OPEN_KEY, DEFAULT_OPEN_KEY) {
        return;
    }
    if host.mod_screen() == Some(MOD_ID) {
        host.mod_screen_close(MOD_ID);
    } else if host.playing() {
        super::screen::open(host);
    }
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::dispatch_key;

    use super::*;
    use crate::MOD_ID;
    use crate::client::cache::{View, with};
    use crate::test_host::{TestHost, exclusive, mods};

    #[test]
    fn k_opens_the_screen_while_playing() {
        // Given a player playing with nothing over the world
        let _serial = exclusive();
        let mut host = TestHost {
            playing: true,
            ..TestHost::default()
        };

        // When K goes down, then up
        dispatch_key(&mut host, &mods(), "key.keyboard.k", true);
        dispatch_key(&mut host, &mods(), "key.keyboard.k", false);

        // Then the screen opens once
        assert_eq!(host.screens_opened, vec![MOD_ID]);
        assert_eq!(DEFAULT_OPEN_KEY, "key.keyboard.k");
    }

    #[test]
    fn k_closes_the_screen_and_reopens_it_on_the_overview() {
        // Given the skills screen open on a skill's view
        let _serial = exclusive();
        let mut host = TestHost {
            open_screen: Some(MOD_ID.into()),
            ..TestHost::default()
        };
        with(|cache| cache.view = View::Skill);

        // When K goes down, and again once the world is played
        dispatch_key(&mut host, &mods(), "key.keyboard.k", true);
        let closed = host.screens_closed.clone();
        host.open_screen = None;
        host.playing = true;
        dispatch_key(&mut host, &mods(), "key.keyboard.k", true);

        // Then the first press closes it, and the second opens it on the overview
        assert_eq!(closed, vec![MOD_ID]);
        assert_eq!(host.screens_opened, vec![MOD_ID]);
        assert_eq!(with(|cache| cache.view), View::Overview);
    }

    #[test]
    fn k_does_nothing_over_another_screen_or_when_rebound() {
        // Given a player in their inventory, and one who bound the screen to J
        let _serial = exclusive();
        let mut in_menu = TestHost {
            playing: false,
            ..TestHost::default()
        };
        let mut rebound = TestHost {
            playing: true,
            ..TestHost::default()
        };
        rebound
            .mod_keys
            .insert(OPEN_KEY.into(), "key.keyboard.j".into());

        // When K is pressed for both, and J for the second
        dispatch_key(&mut in_menu, &mods(), "key.keyboard.k", true);
        dispatch_key(&mut rebound, &mods(), "key.keyboard.k", true);
        let after_k = rebound.screens_opened.len();
        dispatch_key(&mut rebound, &mods(), "key.keyboard.j", true);

        // Then K does nothing in a menu or once rebound, and J opens it
        assert!(in_menu.screens_opened.is_empty());
        assert_eq!((after_k, rebound.screens_opened.len()), (0, 1));
    }
}
