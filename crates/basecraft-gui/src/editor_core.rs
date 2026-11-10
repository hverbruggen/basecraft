//! Shared, Qt-free editor appearance preparation. Bridges retain all live state
//! and decide when to set properties and refresh their domain-specific views.

use basecraft_viewmodel::Theme;

use crate::bridge::{ThemeStamp, css};

pub(crate) struct ThemeUpdate<S = ThemeStamp> {
    pub theme: Theme,
    pub stamp: S,
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub ok: String,
    pub error: String,
}

fn changed_font(current: &str, font: String) -> Option<String> {
    (current != font).then_some(font)
}

fn theme_update<S: PartialEq>(
    current: &S,
    observed: &S,
    load: impl FnOnce() -> Option<(Theme, S)>,
) -> Option<ThemeUpdate<S>> {
    if current == observed {
        return None;
    }
    let (theme, stamp) = load()?;
    Some(ThemeUpdate {
        background: css(theme.background),
        foreground: css(theme.foreground),
        accent: css(theme.accent),
        ok: css(theme.nearest_hue(120.0)),
        error: css(theme.nearest_hue(0.0)),
        theme,
        stamp,
    })
}

pub(crate) fn prepare_font(current: &str) -> Option<String> {
    #[cfg(test)]
    if let Some(font) = testing::font() {
        return changed_font(current, font);
    }
    changed_font(current, crate::bridge::monospace_font())
}

pub(crate) fn prepare_theme(
    current: &ThemeStamp,
    observe: impl FnOnce() -> ThemeStamp,
    load: impl FnOnce() -> Option<(Theme, ThemeStamp)>,
) -> Option<ThemeUpdate> {
    #[cfg(test)]
    if let Some(provider) = testing::provider() {
        return theme_update(current, &provider.observed, || {
            provider.loads.set(provider.loads.get() + 1);
            provider.loaded.clone()
        });
    }
    theme_update(current, &observe(), load)
}

// A scoped, application-thread-only provider for real-controller tests. No
// process-global environment changes, production invokables or new state owner.
#[cfg(test)]
#[allow(dead_code)] // Only the bootstrap target uses the scoped provider.
pub(crate) mod testing {
    use super::{Theme, ThemeStamp};
    use std::{cell::RefCell, rc::Rc};

    pub(crate) struct Appearance {
        pub font: String,
        pub observed: ThemeStamp,
        pub loaded: Option<(Theme, ThemeStamp)>,
        pub loads: std::cell::Cell<usize>,
    }

    thread_local! {
        static PROVIDER: RefCell<Option<Rc<Appearance>>> = const { RefCell::new(None) };
    }

    pub(crate) fn with<R>(appearance: Rc<Appearance>, run: impl FnOnce() -> R) -> R {
        struct Restore(Option<Rc<Appearance>>);
        impl Drop for Restore {
            fn drop(&mut self) {
                PROVIDER.with(|provider| *provider.borrow_mut() = self.0.take());
            }
        }
        let _restore = Restore(PROVIDER.with(|provider| provider.replace(Some(appearance))));
        run()
    }

    pub(super) fn font() -> Option<String> {
        PROVIDER.with(|provider| provider.borrow().as_ref().map(|p| p.font.clone()))
    }

    pub(super) fn provider() -> Option<Rc<Appearance>> {
        PROVIDER.with(|provider| provider.borrow().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_changes_even_when_theme_stamp_is_unchanged() {
        assert_eq!(changed_font("old", "new".into()), Some("new".into()));
        assert_eq!(changed_font("same", "same".into()), None);
        assert!(theme_update(&1, &1, || panic!("unchanged stamp must not load")).is_none());
    }

    #[test]
    fn failed_load_returns_no_replacement_and_can_retry() {
        assert!(theme_update(&1, &2, || None).is_none());
        let theme = Theme::publication().with_color_blind_friendly(true);
        let update = theme_update(&1, &2, || Some((theme.clone(), 3))).unwrap();
        assert_eq!(update.stamp, 3, "retain stamp captured by the loader");
        assert_eq!(update.theme, theme);
        assert_eq!(update.background, css(theme.background));
        assert_eq!(update.foreground, css(theme.foreground));
        assert_eq!(update.accent, css(theme.accent));
        assert_eq!(update.ok, css(theme.nearest_hue(120.0)));
        assert_eq!(update.error, css(theme.nearest_hue(0.0)));
    }
}
