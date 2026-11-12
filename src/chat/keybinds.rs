//! Rebinding the shortcuts, and registering the system-wide chords (Quick Ask, Ask about
//! selection) through the [`ShortcutRegistrar`] the app attaches.

use std::rc::Rc;

use rune::{Shortcut, ShortcutError};
use rune_kit::ShortcutRecorderState;
use tracing::warn;

use super::Chat;
use crate::keymap::{Action, Keymap, Refusal, ShortcutRegistrar};

impl Chat {
    pub(super) fn sync_recorder(&mut self, action: Action) {
        let mut recorder = ShortcutRecorderState::new(self.keymap.get(action).cloned());
        if self.shortcut_warnings.contains_key(&action) {
            recorder.mark_unregistered();
        }
        self.recorders[action.index()] = recorder;
    }

    /// Why `action`'s system-wide chord isn't registered, when it isn't.
    pub fn shortcut_warning(&self, action: Action) -> Option<&str> {
        self.shortcut_warnings.get(&action).map(String::as_str)
    }

    /// Proposes `next` (None unbinds) for `action`. A chord another action holds, or a
    /// reserved one, is refused with a note for the recorder; a system-wide one must also
    /// register with the system, new before old, so a refusal keeps the working binding.
    pub fn rebind(&mut self, action: Action, next: Option<&Shortcut>) -> Result<(), ShortcutError> {
        if let Some(chord) = next
            && let Err(refusal) = self.keymap.check(action, chord)
        {
            self.keybind_note = Some(refusal.message());
            return Err(match refusal {
                Refusal::Reserved => ShortcutError::InvalidAccelerator("reserved".into()),
                Refusal::UsedBy(_) => ShortcutError::Conflict,
            });
        }
        if action.is_global()
            && let Err(error) = self.register_global(action, next)
        {
            self.keybind_note = Some(self.registration_failure(next, &error));
            return Err(error);
        }
        self.keymap.set(action, next.cloned());
        self.keybind_note = None;
        self.shortcut_warnings.remove(&action);
        self.sync_recorder(action);
        self.refresh_menu();
        self.persist();
        Ok(())
    }

    /// Whether any shortcut differs from its default, or a system-wide chord failed.
    pub fn keybinds_customised(&self) -> bool {
        Action::ALL.iter().any(|&a| !self.keymap.is_default(a))
            || !self.shortcut_warnings.is_empty()
    }

    pub fn reset_keybind(&mut self, action: Action) {
        let default = Keymap::default().get(action).cloned();
        if let Err(error) = self.rebind(action, default.as_ref()) {
            warn!("could not reset the {action:?} shortcut: {error:?}");
        }
        self.sync_recorder(action);
    }

    pub fn restore_default_keybinds(&mut self) {
        for action in Action::ALL {
            if !self.keymap.is_default(action) || self.shortcut_warnings.contains_key(&action) {
                self.reset_keybind(action);
            }
        }
        self.keybind_note = None;
    }

    /// Hands over what registers the system-wide chords, and registers the ones in the
    /// keymap. A chord that can't be taken leaves a warning for the keyboard section.
    pub fn attach_registrar(&mut self, registrar: Rc<dyn ShortcutRegistrar>) {
        self.registrar = Some(registrar);
        for action in Action::ALL.into_iter().filter(|a| a.is_global()) {
            let chord = self.keymap.get(action).cloned();
            // A headless run registers nothing, so its refusal says nothing about the chord.
            if let Err(error) = self.register_global(action, chord.as_ref())
                && !rune::is_headless()
            {
                let warning = self.registration_failure(chord.as_ref(), &error);
                self.shortcut_warnings.insert(action, warning);
                self.sync_recorder(action);
            }
        }
    }

    fn register_global(
        &mut self,
        action: Action,
        next: Option<&Shortcut>,
    ) -> Result<(), ShortcutError> {
        let Some(registrar) = self.registrar.clone() else {
            return Ok(());
        };
        let new = next
            .map(|chord| registrar.register(action, chord))
            .transpose()?;
        if let Some(old) = self.registrations.remove(&action)
            && let Err(error) = registrar.unregister(old)
        {
            warn!("could not release the old {action:?} shortcut: {error:?}");
        }
        if let Some(new) = new {
            self.registrations.insert(action, new);
        }
        Ok(())
    }

    fn registration_failure(&self, chord: Option<&Shortcut>, error: &ShortcutError) -> String {
        let caps = chord
            .map(|c| {
                c.keycaps()
                    .join(if cfg!(target_os = "macos") { "" } else { "+" })
            })
            .unwrap_or_default();
        let owner = chord
            .zip(self.registrar.as_ref())
            .and_then(|(c, r)| r.owner(c));
        match (error, owner) {
            (ShortcutError::Conflict, Some(owner)) => {
                format!("{caps} is used by {owner}. Pick another shortcut.")
            }
            (ShortcutError::Conflict, None) => {
                format!("{caps} is already taken. Pick another shortcut.")
            }
            _ => format!("{caps} couldn't be registered. Pick another shortcut."),
        }
    }
}
