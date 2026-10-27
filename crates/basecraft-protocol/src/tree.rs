//! Phylogenetic tree annotations and drawing style, as tree operations
//! carry them and the tree's sidecar file stores them.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A clade, named by the tips whose most recent common ancestor it is:
/// one tip, or two.
pub type CladeRef = Vec<String>;

/// A highlighted clade: a name on its stem branch, a bar along its tips, a
/// field of colour behind it, or any mix of these.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CladeAnnotation {
    pub clade: CladeRef,
    #[serde(default)]
    pub label: String,
    /// Write the label on the clade's stem branch.
    #[serde(default)]
    pub on_branch: bool,
    /// Draw a bar along the clade's tips, labelled beside it.
    #[serde(default)]
    pub bar: bool,
    /// Bar thickness in pixels.
    #[serde(default = "default_bar_width")]
    pub bar_width: u32,
    /// Fill the area the clade covers.
    #[serde(default)]
    pub field: bool,
    /// `#rrggbb`, for the bar and the field.
    #[serde(default = "default_colour")]
    pub colour: String,
    /// The field's opacity at its left and right edges, in percent. Two
    /// different values make a gradient.
    #[serde(default = "default_opacity")]
    pub opacity_left: u8,
    #[serde(default = "default_opacity")]
    pub opacity_right: u8,
}

fn default_bar_width() -> u32 {
    3
}

fn default_colour() -> String {
    "#808080".into()
}

fn default_opacity() -> u8 {
    30
}

impl CladeAnnotation {
    pub fn new(clade: CladeRef, label: impl Into<String>) -> Self {
        Self {
            clade,
            label: label.into(),
            on_branch: false,
            bar: true,
            bar_width: default_bar_width(),
            field: false,
            colour: default_colour(),
            opacity_left: default_opacity(),
            opacity_right: default_opacity(),
        }
    }
}

/// Where support values sit relative to their branch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportPlacement {
    /// Above branches that go up from their parent, below those that go
    /// down.
    #[default]
    Natural,
    Above,
    Below,
}

/// Support shown as filled circles at nodes: dark at or above `high`
/// percent, grey at or above `low`, white below, as in many published
/// trees. Proportions count as percentages, as for `support_min`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SupportSymbols {
    /// The support set the circles show.
    pub set: String,
    pub high: u32,
    pub low: u32,
}

/// Scientific support semantics supplied by a producer, never guessed from its name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportKind {
    #[default]
    Unknown,
    Bootstrap,
    Probability,
    /// Percentage support from a method other than ordinary bootstrap.
    Percentage,
}

/// How a tree is drawn. Sizes are pixels at 100 % zoom.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct TreeStyle {
    pub support_placement: SupportPlacement,
    /// Support sets shown at nodes, in order, joined with "/". Empty shows
    /// every set.
    pub support_shown: Vec<String>,
    /// Hide support values below this percentage. Known producer metadata sets
    /// the scale; ambiguous imported sets with all finite values in 0–1 use
    /// proportions. Legacy `support_proportions` overrides only Unknown imports. 0 shows all.
    pub support_min: u32,
    /// Per-set thresholds in percent, falling back to `support_min`.
    pub support_minimums: BTreeMap<String, u32>,
    /// Display conversion only: true renders fractions, false percentages.
    pub support_fraction: BTreeMap<String, bool>,
    /// Show an ordered legend when more than one support set is displayed.
    pub support_legend: bool,
    /// Decimal places per set; missing entries use the support semantics. Maximum 10.
    pub support_decimals: BTreeMap<String, u8>,
    /// Legacy source-unit interpretation for Unknown imports only (true means 0–1).
    pub support_proportions: BTreeMap<String, bool>,
    /// Preserve settings written by newer versions.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
    pub symbols: Option<SupportSymbols>,
    pub tip_font: u32,
    pub support_font: u32,
    pub branch_width: u32,
    /// Write clade labels beside bars top to bottom, rather than across.
    pub vertical_bar_labels: bool,
    pub scale_bar: bool,
}

impl Default for TreeStyle {
    fn default() -> Self {
        Self {
            support_placement: SupportPlacement::Natural,
            support_shown: Vec::new(),
            support_min: 0,
            support_minimums: BTreeMap::new(),
            support_fraction: BTreeMap::new(),
            support_legend: true,
            support_decimals: BTreeMap::new(),
            support_proportions: BTreeMap::new(),
            extra: BTreeMap::new(),
            symbols: None,
            tip_font: 13,
            support_font: 10,
            branch_width: 1,
            vertical_bar_labels: true,
            scale_bar: true,
        }
    }
}
