//! Zoom shared by the editors: a scale along one axis, zoomed about a point
//! on screen, and the zoom level as the zoom field shows it.

/// One axis of a view: `start` units at the axis origin and `per_px` units
/// per screen pixel, such as bases, alignment columns or trace samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub start: f64,
    pub per_px: f64,
}

impl Axis {
    /// The unit at `x` pixels from the axis origin.
    #[must_use]
    pub fn at(&self, x: f64) -> f64 {
        self.start + x * self.per_px
    }

    /// Zoom by `factor` (> 1 zooms in), keeping the unit at `x` there, with
    /// the scale held within `min..=max` units per pixel.
    pub fn zoom_at(&mut self, x: f64, factor: f64, min: f64, max: f64) {
        let anchor = self.at(x);
        self.per_px = (self.per_px / factor).clamp(min, max);
        self.start = anchor - x * self.per_px;
    }
}

/// A zoom level for the zoom field: whole percents from 10 %, two
/// significant digits below (2.5 %, 0.16 %, 0.0016 %).
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn percent_text(percent: f64) -> String {
    if !percent.is_finite() || percent <= 0.0 {
        return "0 %".into();
    }
    if percent >= 10.0 {
        format!("{percent:.0} %")
    } else {
        let digits = (1.0 - percent.log10().floor()).max(1.0) as usize;
        format!("{percent:.digits$} %")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zooming_keeps_the_unit_under_the_pointer() {
        let mut axis = Axis {
            start: 100.0,
            per_px: 2.0,
        };
        let before = axis.at(50.0);
        axis.zoom_at(50.0, 4.0, 0.1, 10.0);
        assert!((axis.per_px - 0.5).abs() < 1e-12);
        assert!((axis.at(50.0) - before).abs() < 1e-9);
        // Held within its range.
        axis.zoom_at(50.0, 1e9, 0.1, 10.0);
        assert!((axis.per_px - 0.1).abs() < 1e-12);
        assert!((axis.at(50.0) - before).abs() < 1e-9);
    }

    #[test]
    fn percentages_read_alike_everywhere() {
        assert_eq!(percent_text(100.0), "100 %");
        assert_eq!(percent_text(12.4), "12 %");
        assert_eq!(percent_text(0.163), "0.16 %");
        assert_eq!(percent_text(0.001_63), "0.0016 %");
    }
}
