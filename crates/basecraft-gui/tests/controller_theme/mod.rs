//! Exercise shared preparation through actual QML-created editor controllers.
use cxx_qt::casting::Downcast;
use cxx_qt_lib::{QByteArray, QQmlApplicationEngine, QString, QUrl};
use std::{cell::Cell, rc::Rc};

use crate::{alignment_bridge, bridge, editor_core, tree_bridge};

pub(crate) fn run() {
    // Only the dedicated temporary bootstrap profile may be written. The
    // provider itself never mutates global environment or desktop settings.
    let profile = std::env::var_os("BASECRAFT_PROFILE_DIR").expect("isolated bootstrap profile");
    let paths = basecraft_platform::paths::AppPaths::discover().unwrap();
    assert!(paths.preferences().starts_with(profile));
    basecraft_platform::settings::SettingsStore::new(paths.preferences())
        .update(basecraft_platform::settings::Change::ShowGc(true))
        .unwrap();
    assert_ne!(bridge::theme_stamp(), bridge::ThemeStamp::default());

    let checked = Rc::new(Cell::new(0));
    let count = checked.clone();
    let mut engine = QQmlApplicationEngine::new();
    engine
        .pin_mut()
        .on_object_created(move |_, object, _| {
            // SAFETY: Qt owns the just-created object for this synchronous
            // callback. Only its pinned downcast is used during the check.
            let mut object =
                unsafe { std::pin::Pin::new_unchecked(object.as_mut().expect("theme controller")) };
            // These are assertions on the common public controller interface,
            // not a production abstraction over document/selection ownership.
            macro_rules! check {
                ($controller:expr, $file:expr, $bytes:expr, $select:expr, $selection:expr) => {{
                    let mut controller = $controller;
                    let folder = tempfile::tempdir().unwrap();
                    let path = folder.path().join($file);
                    std::fs::write(&path, $bytes).unwrap();
                    assert!(
                        controller
                            .as_mut()
                            .open_file(&QString::from(path.to_str().unwrap()))
                    );
                    controller.as_mut().resize(800.0, 500.0);
                    $select(controller.as_mut());
                    controller.as_mut().find_query(&QString::from("A"), false);
                    for _ in 0..1000 {
                        let result = controller.as_mut().find_poll(0).to_string();
                        if !result.is_empty() {
                            let result: serde_json::Value = serde_json::from_str(&result).unwrap();
                            if result["busy"] == false {
                                assert!(result["count"].as_u64().unwrap() > 0);
                                break;
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    assert_ne!(controller.overview_matches().to_string(), "[]");
                    macro_rules! state {
                        () => {
                            (
                                controller.file_path().clone(),
                                controller.document_record(),
                                controller.record_revision(),
                                *controller.dirty(),
                                controller.accessible_page(0),
                                controller.status().clone(),
                                $selection(&controller),
                                controller.overview_matches().clone(),
                                controller.message().clone(),
                                controller.message_kind().clone(),
                            )
                        };
                    }
                    macro_rules! palette {
                        () => {
                            (
                                controller.background().clone(),
                                controller.foreground().clone(),
                                controller.accent().clone(),
                                controller.ok_color().clone(),
                                controller.error_color().clone(),
                            )
                        };
                    }
                    let before = state!();
                    let old_palette = palette!();
                    let theme =
                        basecraft_viewmodel::Theme::publication().with_color_blind_friendly(true);
                    let appearance = |font: &str, loaded| {
                        Rc::new(editor_core::testing::Appearance {
                            font: font.into(),
                            observed: bridge::ThemeStamp::default(),
                            loaded,
                            loads: Cell::new(0),
                        })
                    };
                    let sentinel = QByteArray::from("no refresh");
                    controller.as_mut().set_frame(sentinel.clone());
                    let failed = appearance("theme-test-failed-font", None);
                    editor_core::testing::with(failed.clone(), || {
                        controller.as_mut().check_theme()
                    });
                    assert_eq!(failed.loads.get(), 1);
                    assert_eq!(controller.mono_font().to_string(), "theme-test-failed-font");
                    assert_eq!(palette!(), old_palette);
                    assert_eq!(controller.frame(), &sentinel, "failed load refreshed");
                    assert_eq!(state!(), before);
                    assert!(
                        controller.as_mut().find_poll(0).is_empty(),
                        "theme update changed Find state"
                    );

                    let success = appearance(
                        "theme-test-font",
                        Some((theme.clone(), bridge::ThemeStamp::default())),
                    );
                    editor_core::testing::with(success.clone(), || {
                        controller.as_mut().check_theme()
                    });
                    assert_eq!(
                        success.loads.get(),
                        1,
                        "failure must retain the old stamp for retry"
                    );
                    assert_eq!(controller.mono_font().to_string(), "theme-test-font");
                    assert_eq!(
                        palette!(),
                        (
                            QString::from(&bridge::css(theme.background)),
                            QString::from(&bridge::css(theme.foreground)),
                            QString::from(&bridge::css(theme.accent)),
                            QString::from(&bridge::css(theme.nearest_hue(120.0))),
                            QString::from(&bridge::css(theme.nearest_hue(0.0))),
                        )
                    );
                    assert_ne!(
                        controller.frame(),
                        &sentinel,
                        "successful load did not refresh"
                    );
                    assert_eq!(state!(), before);
                    assert!(
                        controller.as_mut().find_poll(0).is_empty(),
                        "theme update changed Find state"
                    );

                    controller.as_mut().set_frame(sentinel.clone());
                    let unchanged = appearance("theme-test-independent-font", None);
                    editor_core::testing::with(unchanged.clone(), || {
                        controller.as_mut().check_theme()
                    });
                    assert_eq!(unchanged.loads.get(), 0, "unchanged stamp reloaded");
                    assert_eq!(
                        controller.mono_font().to_string(),
                        "theme-test-independent-font"
                    );
                    assert_eq!(controller.frame(), &sentinel, "unchanged stamp refreshed");
                    assert_eq!(state!(), before);
                    assert!(
                        controller.as_mut().find_poll(0).is_empty(),
                        "theme update changed Find state"
                    );
                }};
            }
            if let Some(controller) = object
                .as_mut()
                .downcast_pin::<bridge::qobject::GenomeController>()
            {
                check!(
                    controller,
                    "sequence.fa",
                    ">A\nACGTACGT\n",
                    |mut c: std::pin::Pin<&mut bridge::qobject::GenomeController>| {
                        c.as_mut().key(bridge::KEY_A, bridge::CTRL);
                        assert!(c.has_selection());
                    },
                    |c: &bridge::qobject::GenomeController| c.selection_text()
                );
            } else if let Some(controller) = object
                .as_mut()
                .downcast_pin::<alignment_bridge::qobject::AlignmentController>(
            ) {
                check!(
                    controller,
                    "alignment.afa",
                    ">A\nACGT\n>B\nACGA\n",
                    |mut c: std::pin::Pin<&mut alignment_bridge::qobject::AlignmentController>| {
                        c.as_mut().select_block(0, 0, 1, 2);
                        assert!(!c.selection_text().is_empty());
                    },
                    |c: &alignment_bridge::qobject::AlignmentController| c.selection_text()
                );
            } else if let Some(controller) = object
                .as_mut()
                .downcast_pin::<tree_bridge::qobject::TreeController>()
            {
                check!(
                    controller,
                    "tree.nwk",
                    "((A:1,B:2):3,(C:4,D:5):6);\n",
                    |mut c: std::pin::Pin<&mut tree_bridge::qobject::TreeController>| {
                        c.as_mut().select_clade(&QString::from("[\"A\",\"B\"]"));
                        assert!(*c.has_selection());
                    },
                    |c: &tree_bridge::qobject::TreeController| *c.has_selection()
                );
            } else {
                panic!("unexpected theme controller");
            }
            count.set(count.get() + 1);
        })
        .release();
    for name in ["GenomeController", "AlignmentController", "TreeController"] {
        engine.pin_mut().load_data(
            &QByteArray::from(format!("import Basecraft; {name} {{}}").as_str()),
            &QUrl::default(),
        );
    }
    assert_eq!(checked.get(), 3);
    println!("Three real editor theme/font, unchanged stamp and failed-load regressions passed");
}
