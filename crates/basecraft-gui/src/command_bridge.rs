//! QML access to the one list of commands in `basecraft_viewmodel::commands`.
use basecraft_viewmodel::commands;
use cxx_qt_lib::{QString, QStringList};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type AppCommands = super::CommandList;
        #[qinvokable]
        fn caption(self: &AppCommands, id: &QString) -> QString;
        #[qinvokable]
        fn tip(self: &AppCommands, id: &QString) -> QString;
        #[qinvokable]
        fn key(self: &AppCommands, id: &QString) -> QString;
        #[qinvokable]
        fn scope(self: &AppCommands, id: &QString) -> QString;
        #[qinvokable]
        fn icon(self: &AppCommands, id: &QString) -> QString;
        /// `context` is the JSON of `basecraft_viewmodel::commands::Context`.
        #[qinvokable]
        fn disabled_reason(self: &AppCommands, id: &QString, context: &QString) -> QString;
        #[qinvokable]
        fn ribbon_tabs(self: &AppCommands, kind: &QString) -> QStringList;
        #[qinvokable]
        fn ribbon_tab(self: &AppCommands, kind: &QString, chosen: &QString) -> QString;
        /// The ribbon page index that shows `tab`.
        #[qinvokable]
        fn ribbon_panel(self: &AppCommands, tab: &QString) -> i32;
        #[qinvokable]
        fn blast_source(
            self: &AppCommands,
            alignment: bool,
            asked: &QString,
            has_selection: bool,
        ) -> QString;
    }
}

#[derive(Default)]
pub struct CommandList;

impl qobject::AppCommands {
    fn caption(&self, id: &QString) -> QString {
        QString::from(commands::caption(&id.to_string()))
    }
    fn tip(&self, id: &QString) -> QString {
        QString::from(commands::tip(&id.to_string()))
    }
    fn key(&self, id: &QString) -> QString {
        QString::from(commands::key(&id.to_string()))
    }
    fn scope(&self, id: &QString) -> QString {
        QString::from(commands::scope(&id.to_string()))
    }
    fn icon(&self, id: &QString) -> QString {
        QString::from(commands::icon(&id.to_string()))
    }
    fn disabled_reason(&self, id: &QString, context: &QString) -> QString {
        let context = serde_json::from_str(&context.to_string()).unwrap_or_default();
        QString::from(commands::disabled_reason(&id.to_string(), &context))
    }
    fn ribbon_tabs(&self, kind: &QString) -> QStringList {
        let mut tabs = QStringList::default();
        for tab in commands::ribbon_tabs(&kind.to_string()) {
            tabs.append(QString::from(tab));
        }
        tabs
    }
    fn blast_source(&self, alignment: bool, asked: &QString, has_selection: bool) -> QString {
        QString::from(commands::blast_source(
            alignment,
            &asked.to_string(),
            has_selection,
        ))
    }
    fn ribbon_panel(&self, tab: &QString) -> i32 {
        i32::try_from(commands::ribbon_panel(&tab.to_string())).unwrap_or(0)
    }
    fn ribbon_tab(&self, kind: &QString, chosen: &QString) -> QString {
        QString::from(commands::ribbon_tab(&kind.to_string(), &chosen.to_string()))
    }
}
