//! The Rust objects QML talks to.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QByteArray, frame)]
        #[qproperty(QString, status)]
        type Hello = super::HelloRust;

        /// Rebuild the frame for a view of `width` × `height` logical pixels.
        #[qinvokable]
        fn layout(self: Pin<&mut Hello>, width: f64, height: f64);

        /// Resident memory of this process in MB, from /proc.
        #[qinvokable]
        fn rss_mb(self: &Hello) -> f64;

        /// Rss, Pss and private memory from /proc/self/smaps_rollup.
        #[qinvokable]
        fn memory_report(self: &Hello) -> QString;
    }
}

use std::pin::Pin;

use basecraft_viewmodel::Frame;
use cxx_qt_lib::{QByteArray, QString};

#[derive(Default)]
pub struct HelloRust {
    frame: QByteArray,
    status: QString,
}

impl qobject::Hello {
    #[allow(clippy::many_single_char_names)]
    pub fn layout(mut self: Pin<&mut Self>, width: f64, height: f64) {
        #[allow(clippy::cast_possible_truncation)]
        let (w, h) = (width as f32, height as f32);
        let mut f = Frame::new();
        let colors = [
            [0.64, 0.18, 0.16, 1.0],
            [0.25, 0.42, 0.29, 1.0],
            [0.25, 0.33, 0.47, 1.0],
            [0.49, 0.35, 0.06, 1.0],
        ];
        let cell = 24.0;
        let mut i = 0;
        let mut y = 60.0;
        while y + cell < h {
            let mut x = 20.0;
            while x + cell < w - 20.0 {
                f.rect(x, y, cell - 2.0, cell - 2.0, colors[i % 4]);
                f.glyph(
                    x + 5.0,
                    y + 2.0,
                    cell - 6.0,
                    b"ACGT"[i % 4],
                    [1.0, 1.0, 1.0, 1.0],
                );
                i += 1;
                x += cell;
            }
            y += cell;
        }
        f.text(20.0, 16.0, 28.0, "Basecraft", [0.2, 0.16, 0.1, 1.0]);
        let bytes = f.to_bytes();
        self.as_mut()
            .set_status(QString::from(&format!("{} instances", f.len())));
        self.as_mut().set_frame(QByteArray::from(bytes.as_slice()));
    }

    pub fn memory_report(&self) -> QString {
        let text = std::fs::read_to_string("/proc/self/smaps_rollup").unwrap_or_default();
        let field = |name: &str| -> f64 {
            text.lines()
                .find(|l| l.starts_with(name))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
                .map_or(0.0, |kb| kb / 1024.0)
        };
        let private = field("Private_Clean:") + field("Private_Dirty:");
        QString::from(&format!(
            "rss {:.1} MB, pss {:.1} MB, private {:.1} MB (dirty {:.1}), anon {:.1} MB",
            field("Rss:"),
            field("Pss:"),
            private,
            field("Private_Dirty:"),
            field("Anonymous:")
        ))
    }

    pub fn rss_mb(&self) -> f64 {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("VmRSS:"))
                    .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
            })
            .map_or(0.0, |kb| kb / 1024.0)
    }
}
