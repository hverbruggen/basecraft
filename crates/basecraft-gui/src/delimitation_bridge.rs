//! A partition workspace owns its edits; `TreeController` remains a presentation canvas.
#[cxx_qt::bridge]
pub mod qobject {
    impl cxx_qt::Threading for DelimitationController {}
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, workspace_json)]
        #[qproperty(QString, selection_json)]
        #[qproperty(QString, preview_json)]
        #[qproperty(QString, statistics_json)]
        #[qproperty(QString, display_path)]
        #[qproperty(QString, active_hypothesis)]
        #[qproperty(QString, linked_alignment)]
        #[qproperty(QString, file_path)]
        #[qproperty(QString, status)]
        #[qproperty(QString, message)]
        #[qproperty(QString, message_kind)]
        #[qproperty(QString, zoom_text)]
        #[qproperty(bool, dirty)]
        #[qproperty(bool, busy)]
        #[qproperty(bool, can_cut)]
        #[qproperty(f64, root_height)]
        #[qproperty(f64, threshold)]
        type DelimitationController = super::DelimitationRust;
        #[qinvokable]
        fn open_file(self: Pin<&mut DelimitationController>, path: &QString) -> bool;
        #[qinvokable]
        fn attach_tree(self: Pin<&mut DelimitationController>, path: &QString, rooted: bool);
        #[qinvokable]
        fn activate(self: Pin<&mut DelimitationController>, id: &QString);
        #[qinvokable]
        fn action(self: Pin<&mut DelimitationController>, name: &QString, args: &QString);
        #[qinvokable]
        fn select_group(
            self: Pin<&mut DelimitationController>,
            hypothesis: &QString,
            group: &QString,
            toggle: bool,
        );
        #[qinvokable]
        fn highlight_taxa(self: Pin<&mut DelimitationController>, names: &QString);
        #[qinvokable]
        fn tree_taxa(self: Pin<&mut DelimitationController>, names: &QString);
        #[qinvokable]
        fn coverage_json(self: &DelimitationController, geometry: &QString) -> QString;
        #[qsignal]
        #[cxx_name = "taxaSelected"]
        fn taxa_selected(self: Pin<&mut DelimitationController>, names: QString);
        #[qsignal]
        #[cxx_name = "displayTaxaSelected"]
        fn display_taxa_selected(self: Pin<&mut DelimitationController>, names: QString);
        #[qinvokable]
        fn bars_json(
            self: &DelimitationController,
            geometry: &QString,
            left: f64,
            height: f64,
        ) -> QString;
        #[qinvokable]
        fn preview_cut(self: Pin<&mut DelimitationController>, threshold: f64);
        #[qinvokable]
        fn preview_position(self: Pin<&mut DelimitationController>, geometry: &QString, x: f64);
        #[qinvokable]
        fn cancel_cut(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn save_cut(self: Pin<&mut DelimitationController>, name: &QString);
        #[qinvokable]
        fn undo(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn redo(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn save(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn save_as(self: Pin<&mut DelimitationController>, path: &QString) -> bool;
        #[qinvokable]
        fn export_svg(self: Pin<&mut DelimitationController>, path: &QString);
        #[qinvokable]
        fn run_analysis(
            self: Pin<&mut DelimitationController>,
            method: &QString,
            input: &QString,
            rooted: bool,
            settings: &QString,
        );
        #[qinvokable]
        fn cancel_job(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn poll_job(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn flush(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn check_theme(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn memory_report(self: &DelimitationController) -> QString;
    }
}
use basecraft_docs::TreeDoc;
use basecraft_ops::{DocId, Session};
use basecraft_protocol::{
    Op,
    delimitation::{DelimitationEdit, Group, Hypothesis, HypothesisOrigin, Provenance},
};
use basecraft_viewmodel::delimitation::{self, Selection};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    pin::Pin,
};

pub struct DelimitationRust {
    workspace_json: QString,
    selection_json: QString,
    preview_json: QString,
    statistics_json: QString,
    display_path: QString,
    active_hypothesis: QString,
    linked_alignment: QString,
    file_path: QString,
    status: QString,
    message: QString,
    message_kind: QString,
    zoom_text: QString,
    dirty: bool,
    busy: bool,
    can_cut: bool,
    root_height: f64,
    threshold: f64,
    session: Option<Session>,
    doc: Option<DocId>,
    selection: Selection,
    display_tree: Option<TreeDoc>,
    time_tree: Option<(TreeDoc, basecraft_delimitation::time_tree::TimeTree, String)>,
    preview: Vec<BTreeSet<String>>,
    job_paths: Option<(PathBuf, PathBuf, String)>,
}
impl Default for DelimitationRust {
    fn default() -> Self {
        Self {
            workspace_json: QString::from("{}"),
            selection_json: QString::from("[]"),
            preview_json: QString::from("[]"),
            statistics_json: QString::from("{}"),
            display_path: QString::default(),
            active_hypothesis: QString::default(),
            linked_alignment: QString::default(),
            file_path: QString::default(),
            status: QString::default(),
            message: QString::default(),
            message_kind: QString::default(),
            zoom_text: QString::default(),
            dirty: false,
            busy: false,
            can_cut: false,
            root_height: 0.0,
            threshold: 0.0,
            session: None,
            doc: None,
            selection: Selection::default(),
            display_tree: None,
            time_tree: None,
            preview: vec![],
            job_paths: None,
        }
    }
}
fn rows(geometry: &str) -> Vec<(String, f32, f32)> {
    serde_json::from_str::<Value>(geometry)
        .ok()
        .and_then(|v| v.get("rows").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| {
            Some((
                v.get("name")?.as_str()?.into(),
                number(v.get("y")?.as_f64()?),
                number(v.get("height")?.as_f64()?),
            ))
        })
        .collect()
}
#[allow(clippy::cast_possible_truncation)]
fn number(v: f64) -> f32 {
    v as f32
}
fn group(members: BTreeSet<String>, label: String) -> Group {
    Group {
        id: ulid::Ulid::generate().to_string(),
        label,
        color: String::new(),
        members,
        scores: vec![],
        individual_scores: BTreeMap::new(),
        attributes: BTreeMap::new(),
    }
}
impl qobject::DelimitationController {
    fn fail(mut self: Pin<&mut Self>, text: &str) {
        self.as_mut().set_message_kind(QString::from("error"));
        self.set_message(QString::from(text));
    }
    fn refresh(mut self: Pin<&mut Self>) {
        let Some((data, status)) = ({
            let r = self.rust();
            r.session.as_ref().zip(r.doc).map(|(s, d)| {
                let w = s.delimitation(d);
                (
                    serde_json::to_string(w).unwrap(),
                    format!(
                        "{} specimens · {} hypotheses · {} selected",
                        w.specimens.len(),
                        w.hypotheses.len(),
                        r.selection.specimens.len()
                    ),
                )
            })
        }) else {
            return;
        };
        self.as_mut().set_workspace_json(QString::from(&data));
        self.set_status(QString::from(&status));
    }
    fn edit(mut self: Pin<&mut Self>, edit: DelimitationEdit) -> Result<(), String> {
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let s = r.session.as_mut().ok_or("No workspace open")?;
            s.apply(
                r.doc.ok_or("No workspace open")?,
                Op::DelimitationEdit {
                    edit: Box::new(edit),
                },
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        };
        match &result {
            Ok(()) => {
                self.as_mut().set_statistics_json(QString::from("{}"));
                self.as_mut().set_dirty(true);
                self.as_mut().set_message(QString::default());
                self.refresh();
            }
            Err(e) => self.fail(e),
        }
        result
    }
    #[allow(clippy::many_single_char_names)]
    pub fn open_file(mut self: Pin<&mut Self>, path: &QString) -> bool {
        let p = PathBuf::from(path.to_string());
        let result = (|| -> basecraft_ops::Result<_> {
            let mut s = Session::new(crate::bridge::project_root(&p));
            let d = s.open(&p)?;
            if !s.is_delimitation(d) {
                return Err(basecraft_ops::Error::Invalid(
                    "This file is not a species workspace".into(),
                ));
            }
            Ok((s, d))
        })();
        match result {
            Ok((s, d)) => {
                let w = s.delimitation(d);
                let display = w.view.display_tree.clone();
                let linked = w
                    .specimens
                    .first()
                    .and_then(|first| {
                        first.sources.keys().find(|path| {
                            Path::new(path).extension().is_some_and(|ext| {
                                ["fa", "fasta", "afa", "fas", "fna"]
                                    .iter()
                                    .any(|e| ext.eq_ignore_ascii_case(e))
                            }) && w
                                .specimens
                                .iter()
                                .all(|specimen| specimen.sources.contains_key(*path))
                        })
                    })
                    .cloned()
                    .unwrap_or_default();
                self.as_mut().set_linked_alignment(QString::from(linked));
                let active = w
                    .hypotheses
                    .first()
                    .map(|h| h.id.clone())
                    .unwrap_or_default();
                {
                    let mut r = self.as_mut().rust_mut();
                    r.session = Some(s);
                    r.doc = Some(d);
                }
                self.as_mut().set_file_path(path.clone());
                self.as_mut().set_active_hypothesis(QString::from(&active));
                self.as_mut().refresh();
                if let Some(display) = display {
                    self.as_mut().attach_tree(&QString::from(&display), false);
                }
                true
            }
            Err(e) => {
                self.fail(&e.to_string());
                false
            }
        }
    }
    pub fn attach_tree(mut self: Pin<&mut Self>, path: &QString, rooted: bool) {
        let result = (|| -> Result<_, Box<dyn std::error::Error>> {
            let p = PathBuf::from(path.to_string());
            let bytes = std::fs::read(&p)?;
            let trees = basecraft_io::newick::parse_trees(std::str::from_utf8(&bytes)?)?;
            if trees.len() != 1 {
                return Err("Choose a file containing exactly one display tree".into());
            }
            let tree = TreeDoc::from_parsed(&trees[0].name, &trees[0]);
            let time = basecraft_delimitation::time_tree::validate(&tree, rooted, 1e-8, true);
            let hash = basecraft_io::hash_bytes(&bytes);
            Ok((p, tree, time, hash, bytes))
        })();
        match result {
            Ok((_source, tree, time, hash, bytes)) => {
                // Render the exact retained tree buffer; later source edits cannot
                // alter cut previews or exported scientific coordinates.
                let retained = (|| -> Result<_, Box<dyn std::error::Error>> {
                    let r = self.rust();
                    let session = r.session.as_ref().ok_or("No workspace open")?;
                    let digest = session.snapshot_bytes(&bytes)?;
                    let display = session
                        .root()
                        .join(".basecraft/displays")
                        .join(format!("{}.nwk", digest.trim_start_matches("b3:")));
                    std::fs::create_dir_all(display.parent().unwrap())?;
                    if !display.exists() {
                        basecraft_io::write_atomic(&display, |w| w.write_all(&bytes))?;
                    }
                    Ok(display)
                })();
                let retained = match retained {
                    Ok(path) => path,
                    Err(e) => {
                        self.fail(&e.to_string());
                        return;
                    }
                };
                self.as_mut().rust_mut().display_tree = Some(tree.clone());
                let warning = time.as_ref().err().map(ToString::to_string);
                if let Ok(time) = time {
                    let height = time.root_height;
                    self.as_mut().rust_mut().time_tree = Some((tree, time, hash));
                    self.as_mut().set_can_cut(true);
                    self.as_mut().set_root_height(height);
                } else {
                    self.as_mut().rust_mut().time_tree = None;
                    self.as_mut().set_can_cut(false);
                }
                self.as_mut()
                    .set_display_path(QString::from(retained.to_string_lossy().as_ref()));
                let mut view = {
                    let r = self.rust();
                    r.session
                        .as_ref()
                        .unwrap()
                        .delimitation(r.doc.unwrap())
                        .view
                        .clone()
                };
                let new_display = Some(retained.to_string_lossy().into_owned());
                if view.display_tree != new_display {
                    view.display_tree = new_display;
                    let _ = self.as_mut().edit(DelimitationEdit::SetView { view });
                }
                if rooted {
                    if let Some(warning) = warning {
                        self.fail(&warning);
                    }
                }
            }
            Err(e) => self.fail(&format!("Cannot attach tree: {e}")),
        }
    }
    pub fn activate(mut self: Pin<&mut Self>, id: &QString) {
        self.as_mut().set_active_hypothesis(id.clone());
        self.refresh();
    }
    pub fn action(self: Pin<&mut Self>, name: &QString, args: &QString) {
        let result = (|| -> Result<DelimitationEdit, String> {
            let r = self.rust();
            let s = r.session.as_ref().ok_or("No workspace open")?;
            let w = s.delimitation(r.doc.unwrap());
            let args: Value = serde_json::from_str(&args.to_string()).map_err(|e| e.to_string())?;
            let text = |key: &str| {
                args.get(key)
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            };
            let id = r.active_hypothesis.to_string();
            let members = r.selection.specimens.clone();
            Ok(match name.to_string().as_str() {
                "create" => DelimitationEdit::Add {
                    hypothesis: Hypothesis {
                        id: ulid::Ulid::generate().to_string(),
                        name: text("name"),
                        origin: HypothesisOrigin::Manual,
                        parent: None,
                        groups: vec![],
                        unassigned: w.specimens.iter().map(|s| s.id.clone()).collect(),
                        scores: vec![],
                        provenance: Provenance::default(),
                        attributes: BTreeMap::new(),
                    },
                },
                "derive" => DelimitationEdit::Derive {
                    source: id,
                    id: ulid::Ulid::generate().to_string(),
                    name: text("name"),
                },
                "rename" => DelimitationEdit::Rename {
                    hypothesis: id,
                    name: text("name"),
                },
                "rename_group" => DelimitationEdit::RenameGroup {
                    hypothesis: id,
                    group: text("group"),
                    label: text("label"),
                },
                "group" => {
                    if members.is_empty() {
                        return Err("Select specimens first".into());
                    }
                    DelimitationEdit::Split {
                        hypothesis: id,
                        group: group(members, text("label")),
                    }
                }
                "merge" => DelimitationEdit::Merge {
                    hypothesis: id,
                    groups: serde_json::from_value(
                        args.get("groups").cloned().unwrap_or(json!([])),
                    )
                    .map_err(|e| e.to_string())?,
                    id: ulid::Ulid::generate().to_string(),
                    label: text("label"),
                },
                "reassign" => DelimitationEdit::Reassign {
                    hypothesis: id,
                    specimens: members,
                    group: Some(text("group")),
                },
                "unassign" => DelimitationEdit::Reassign {
                    hypothesis: id,
                    specimens: members,
                    group: None,
                },
                "visible" => {
                    let mut view = w.view.clone();
                    view.visible =
                        serde_json::from_value(args.get("ids").cloned().unwrap_or(json!([])))
                            .map_err(|e| e.to_string())?;
                    DelimitationEdit::SetView { view }
                }
                _ => return Err("Unknown grouping action".into()),
            })
        })();
        match result {
            Ok(edit) => {
                let _ = self.edit(edit);
            }
            Err(e) => self.fail(&e),
        }
    }
    fn emit_selection(mut self: Pin<&mut Self>) {
        let text = serde_json::to_string(&self.rust().selection.specimens).unwrap();
        self.as_mut().set_selection_json(QString::from(&text));
        self.as_mut().taxa_selected(QString::from(&text));
        let display = (|| -> Option<_> {
            let state = self.rust();
            let (session, doc) = state.session.as_ref().zip(state.doc)?;
            let tree = state.display_tree.as_ref()?;
            let mapping = delimitation::tree_mapping(
                session.delimitation(doc),
                &tree
                    .tips_in_order()
                    .iter()
                    .map(|&i| tree.node(i).name.clone())
                    .collect(),
            );
            Some(
                mapping
                    .iter()
                    .filter(|(_, members)| !members.is_disjoint(&state.selection.specimens))
                    .map(|(tip, _)| tip.clone())
                    .collect::<Vec<_>>(),
            )
        })();
        if let Some(display) = display {
            self.as_mut()
                .display_taxa_selected(QString::from(serde_json::to_string(&display).unwrap()));
        }
        self.refresh();
    }
    pub fn select_group(
        mut self: Pin<&mut Self>,
        hypothesis: &QString,
        group: &QString,
        toggle: bool,
    ) {
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let h = r.session.as_ref().zip(r.doc).and_then(|(s, d)| {
                s.delimitation(d)
                    .hypotheses
                    .iter()
                    .find(|h| h.id == hypothesis.to_string())
            });
            h.ok_or_else(|| "Unknown hypothesis".to_string())
                .and_then(|h| r.selection.group(h, &group.to_string(), toggle))
        };
        match result {
            Ok(()) => self.emit_selection(),
            Err(e) => self.fail(&e),
        }
    }
    pub fn tree_taxa(mut self: Pin<&mut Self>, names: &QString) {
        let mapped = (|| -> Option<_> {
            let r = self.rust();
            let (session, doc) = r.session.as_ref().zip(r.doc)?;
            let display = r.display_tree.as_ref()?;
            let mapping = delimitation::tree_mapping(
                session.delimitation(doc),
                &display
                    .tips_in_order()
                    .iter()
                    .map(|&i| display.node(i).name.clone())
                    .collect(),
            );
            let names: BTreeSet<String> = serde_json::from_str(&names.to_string()).ok()?;
            Some(
                names
                    .iter()
                    .flat_map(|name| mapping.get(name).into_iter().flatten().cloned())
                    .collect(),
            )
        })();
        if let Some(mapped) = mapped {
            if self.as_mut().rust_mut().selection.receive(mapped, "tree") {
                self.emit_selection();
            }
        }
    }
    pub fn coverage_json(&self, geometry: &QString) -> QString {
        let r = self.rust();
        let Some((session, doc)) = r.session.as_ref().zip(r.doc) else {
            return QString::from("{}");
        };
        let work = session.delimitation(doc);
        let names = rows(&geometry.to_string())
            .into_iter()
            .map(|(name, _, _)| name)
            .collect();
        let mapping = delimitation::tree_mapping(work, &names);
        let represented: BTreeSet<_> = mapping.values().flatten().cloned().collect();
        QString::from(json!({"unmatched_tips":names.difference(&mapping.keys().cloned().collect()).collect::<Vec<_>>(),"absent_specimens":work.specimens.iter().filter(|s|!represented.contains(&s.id)).map(|s|s.id.clone()).collect::<Vec<_>>()}).to_string())
    }
    pub fn highlight_taxa(mut self: Pin<&mut Self>, names: &QString) {
        let ids = serde_json::from_str::<BTreeSet<String>>(&names.to_string());
        if let Ok(names) = ids {
            let mapped = {
                let r = self.rust();
                let Some((s, d)) = r.session.as_ref().zip(r.doc) else {
                    return;
                };
                names
                    .into_iter()
                    .filter(|name| {
                        s.delimitation(d)
                            .specimens
                            .iter()
                            .any(|specimen| &specimen.id == name)
                    })
                    .collect()
            };
            if self.as_mut().rust_mut().selection.receive(mapped, "linked") {
                self.emit_selection();
            }
        }
    }
    #[allow(clippy::many_single_char_names)]
    pub fn bars_json(&self, geometry: &QString, left: f64, height: f64) -> QString {
        let r = self.rust();
        let Some((s, d)) = r.session.as_ref().zip(r.doc) else {
            return QString::from("[]");
        };
        let mut work = s.delimitation(d).clone();
        let mapping = delimitation::tree_mapping(
            &work,
            &rows(&geometry.to_string())
                .into_iter()
                .map(|(name, _, _)| name)
                .collect(),
        );
        if !r.preview.is_empty() {
            let mut h = Hypothesis {
                id: "cut-preview".into(),
                name: "Cut preview".into(),
                origin: HypothesisOrigin::ManualCut,
                parent: None,
                groups: vec![],
                unassigned: BTreeSet::new(),
                scores: vec![],
                provenance: Provenance::default(),
                attributes: BTreeMap::new(),
            };
            for (i, members) in r.preview.iter().enumerate() {
                let mut g = group(
                    members
                        .iter()
                        .flat_map(|id| mapping.get(id).into_iter().flatten().cloned())
                        .collect(),
                    format!("Preview {}", i + 1),
                );
                g.color = ["#377eb8", "#e41a1c", "#4daf4a", "#984ea3"][i % 4].into();
                h.groups.push(g);
            }
            work.hypotheses.push(h);
            work.view.visible.push("cut-preview".into());
        }
        let (bars, _) = delimitation::bars(
            &work,
            &rows(&geometry.to_string()),
            number(left),
            36.0,
            Some((0.0, number(height))),
        );
        QString::from(&serde_json::to_string(&bars.iter().map(|b|json!({"hypothesis":b.hypothesis,"group":b.group,"x":b.x,"y":b.y,"width":b.width,"height":b.height,"color":b.color,"label":b.label})).collect::<Vec<_>>()).unwrap())
    }
    pub fn preview_cut(mut self: Pin<&mut Self>, threshold: f64) {
        let result = {
            let r = self.rust();
            r.time_tree
                .as_ref()
                .ok_or("Validate a rooted time tree first".into())
                .and_then(|(tree, time, _)| {
                    basecraft_delimitation::time_tree::cut(tree, time, threshold)
                        .map_err(|e| e.to_string())
                })
        };
        match result {
            Ok(groups) => {
                let json = serde_json::to_string(&groups).unwrap();
                self.as_mut().rust_mut().preview = groups;
                self.as_mut().set_threshold(threshold);
                self.set_preview_json(QString::from(&json));
            }
            Err(e) => self.fail(&e),
        }
    }
    pub fn preview_position(self: Pin<&mut Self>, geometry: &QString, x: f64) {
        let value = serde_json::from_str::<Value>(&geometry.to_string()).ok();
        if let Some(value) = value {
            if let Some((root, tips)) = value["root"][0].as_f64().zip(value["tips_x"].as_f64()) {
                if tips > root {
                    let threshold =
                        self.rust().root_height * ((tips - x) / (tips - root)).clamp(0.0, 1.0);
                    self.preview_cut(threshold);
                }
            }
        }
    }
    pub fn cancel_cut(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().preview.clear();
        self.set_preview_json(QString::from("[]"));
    }
    pub fn save_cut(mut self: Pin<&mut Self>, name: &QString) {
        let edit = {
            let r = self.rust();
            if r.preview.is_empty() {
                return;
            }
            let (tree, _, hash) = r.time_tree.as_ref().unwrap();
            let w = r.session.as_ref().unwrap().delimitation(r.doc.unwrap());
            let mapping = delimitation::tree_mapping(
                w,
                &tree
                    .tips_in_order()
                    .iter()
                    .map(|&i| tree.node(i).name.clone())
                    .collect(),
            );
            let groups: Vec<_> = r
                .preview
                .iter()
                .enumerate()
                .map(|(i, g)| {
                    group(
                        g.iter()
                            .flat_map(|name| mapping.get(name).into_iter().flatten().cloned())
                            .collect(),
                        (i + 1).to_string(),
                    )
                })
                .collect();
            let assigned: BTreeSet<_> = groups
                .iter()
                .flat_map(|g| g.members.iter().cloned())
                .collect();
            let h = Hypothesis {
                id: ulid::Ulid::generate().to_string(),
                name: name.to_string(),
                origin: HypothesisOrigin::ManualCut,
                parent: None,
                groups,
                unassigned: w
                    .specimens
                    .iter()
                    .map(|s| s.id.clone())
                    .filter(|id| !assigned.contains(id))
                    .collect(),
                scores: vec![],
                provenance: Provenance {
                    method: "manual_time_cut".into(),
                    analysis_tree: Some(hash.clone()),
                    parameters: [
                        ("tree_name".into(), json!(tree.name)),
                        ("rooted".into(), json!(true)),
                    ]
                    .into(),
                    ..Provenance::default()
                },
                attributes: BTreeMap::new(),
            };
            DelimitationEdit::SaveCut {
                hypothesis: h,
                threshold: r.threshold,
                boundary: "node_age_le_threshold".into(),
            }
        };
        if self.as_mut().edit(edit).is_ok() {
            self.cancel_cut();
        }
    }
    fn history(mut self: Pin<&mut Self>, redo: bool) {
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            r.session
                .as_mut()
                .zip(r.doc)
                .map(|(s, d)| if redo { s.redo(d) } else { s.undo(d) })
        };
        if let Some(Err(e)) = result {
            self.fail(&e.to_string());
        } else {
            self.as_mut().set_statistics_json(QString::from("{}"));
            self.as_mut().set_message(QString::default());
            self.as_mut().set_dirty(true);
            self.as_mut().cancel_cut();
            self.refresh();
        }
    }
    pub fn undo(self: Pin<&mut Self>) {
        self.history(false);
    }
    pub fn redo(self: Pin<&mut Self>) {
        self.history(true);
    }
    pub fn save(mut self: Pin<&mut Self>) {
        let path = self.rust().file_path.clone();
        self.as_mut().save_as(&path);
    }
    pub fn save_as(mut self: Pin<&mut Self>, path: &QString) -> bool {
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            r.session
                .as_mut()
                .zip(r.doc)
                .map(|(s, d)| s.save(d, Some(Path::new(&path.to_string()))))
        };
        match result {
            Some(Ok(())) => {
                self.as_mut().set_file_path(path.clone());
                self.as_mut().set_dirty(false);
                self.as_mut().set_message_kind(QString::from("ok"));
                self.set_message(QString::from("Saved"));
                true
            }
            Some(Err(e)) => {
                self.fail(&e.to_string());
                false
            }
            None => false,
        }
    }
    pub fn export_svg(mut self: Pin<&mut Self>, path: &QString) {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (s, d) = r.session.as_mut().zip(r.doc).ok_or("No workspace open")?;
            let tree = r
                .display_tree
                .as_ref()
                .ok_or("Attach a display tree first")?;
            let theme = crate::bridge::load_theme()
                .map(|(theme, _)| theme)
                .unwrap_or_default();
            let tree_svg = basecraft_viewmodel::tree::to_svg_with_style(
                tree,
                900.0,
                20.0,
                &theme,
                &crate::bridge::monospace_font(),
            );
            let (rows, width, height) = basecraft_viewmodel::tree::figure_rows(tree, 900.0, 20.0);
            let svg = delimitation::comparison_svg(
                &tree_svg,
                s.delimitation(d),
                &rows,
                width,
                height,
                &crate::bridge::monospace_font(),
            );
            s.export(d, Path::new(&path.to_string()), "svg", |w| {
                w.write_all(svg.as_bytes())
            })?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.as_mut().set_message_kind(QString::from("ok"));
                self.set_message(QString::from("Exported comparison"));
            }
            Err(e) => self.fail(&e.to_string()),
        }
    }
    pub fn run_analysis(
        mut self: Pin<&mut Self>,
        method: &QString,
        input: &QString,
        rooted: bool,
        settings: &QString,
    ) {
        if self.rust().busy {
            return;
        }
        let result = (|| -> Result<_, String> {
            let r = self.rust();
            let (session, doc) = r.session.as_ref().zip(r.doc).ok_or("No workspace open")?;
            let method = method.to_string();
            if ![
                "gmyc_single",
                "gmyc_multiple",
                "ptp",
                "mptp",
                "asap",
                "relative_time",
                "haplotypes",
                "group_statistics",
            ]
            .contains(&method.as_str())
            {
                return Err("Unknown analysis".into());
            }
            let output = session
                .root()
                .join(format!("analysis-{}.json", ulid::Ulid::generate()));
            let mut job = basecraft_plugin_host::FileJob::from_file(
                session,
                doc,
                Path::new(&input.to_string()),
                (&crate::bridge::plugin_dir("delimitation"), &method),
                output.clone(),
            )
            .map_err(|e| e.to_string())?;
            let progress = output.with_extension("progress.json");
            let cancel = output.with_extension("cancel");
            job.params.extend([
                ("rooted".into(), json!(rooted)),
                ("generation".into(), json!(session.generation(doc))),
                ("workers".into(), json!(1)),
                ("seed".into(), json!(1)),
                ("progress".into(), json!(progress)),
                ("cancel_file".into(), json!(cancel)),
            ]);
            let settings: serde_json::Map<String, Value> =
                serde_json::from_str(&settings.to_string()).map_err(|e| e.to_string())?;
            for (key, value) in settings {
                if ["cache", "mapping", "workers", "seed", "replicates", "model"]
                    .contains(&key.as_str())
                    && !value.as_str().is_some_and(str::is_empty)
                {
                    job.params.insert(key, value);
                }
            }
            if method == "group_statistics" {
                let h = session
                    .delimitation(doc)
                    .hypotheses
                    .iter()
                    .find(|h| h.id == r.active_hypothesis.to_string())
                    .ok_or("Choose a hypothesis")?;
                job.params.insert(
                    "groups_data".into(),
                    json!(
                        h.groups
                            .iter()
                            .flat_map(|g| g
                                .members
                                .iter()
                                .map(move |id| (id.clone(), g.id.clone())))
                            .collect::<BTreeMap<_, _>>()
                    ),
                );
            }
            Ok((job, session.generation(doc), progress, cancel))
        })();
        let (job, generation, progress, cancel) = match result {
            Ok(job) => job,
            Err(e) => {
                self.fail(&e);
                return;
            }
        };
        let id = job.params["job_id"].as_str().unwrap().to_string();
        self.as_mut().rust_mut().job_paths = Some((progress, cancel, id));
        self.as_mut().set_busy(true);
        self.as_mut()
            .set_message(QString::from("Running analysis…"));
        let thread = self.qt_thread();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let result = job.run();
            let elapsed = started.elapsed().as_secs_f64();
            let _ = thread.queue(move |mut ctl| {
                let applied = (|| -> Result<Value, String> {
                    let mut state = ctl.as_mut().rust_mut();
                    let state = &mut *state;
                    let (session, doc) = state.session.as_mut().zip(state.doc).ok_or("Workspace closed")?;
                    job.record_terminal(session, doc, &result, elapsed).map_err(|e| e.to_string())?;
                    let result = result.map_err(|e| e.to_string())?;
                    job.record(session, doc, &result).map_err(|e| e.to_string())?;
                    if session.generation(doc) != generation {
                        return Err("Analysis completed for its saved input; workspace changed, so result remains an artifact".into());
                    }
                    let value: Value = serde_json::from_slice(&std::fs::read(&job.output).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
                    if value.get("workspace").is_some_and(|v|!v.is_null()) {
                        let work: basecraft_protocol::delimitation::DelimitationWorkspace = serde_json::from_value(value["workspace"].clone()).map_err(|e|e.to_string())?;
                        let roster: BTreeSet<_> = session.delimitation(doc).specimens.iter().map(|s|s.id.clone()).collect();
                        if roster != work.specimens.iter().map(|s|s.id.clone()).collect() {
                            return Err("Analysis specimen mapping differs; result saved separately".into());
                        }
                        for hypothesis in work.hypotheses {
                            session.apply(doc, Op::DelimitationEdit { edit: Box::new(DelimitationEdit::Add {hypothesis}) }).map_err(|e|e.to_string())?;
                        }
                    }
                    Ok(value)
                })();
                ctl.as_mut().set_busy(false);
                ctl.as_mut().rust_mut().job_paths = None;
                match applied {
                    Ok(value) => {
                        if value.get("groups").is_some() {ctl.as_mut().set_statistics_json(QString::from(value.to_string()));}
                        else {ctl.as_mut().set_dirty(true);}
                        ctl.as_mut().set_message_kind(QString::from("ok"));
                        ctl.as_mut().set_message(QString::from("Analysis completed"));
                        ctl.refresh();
                    }
                    Err(error) => ctl.fail(&error),
                }
            });
        });
    }
    pub fn cancel_job(self: Pin<&mut Self>) {
        let result = self
            .rust()
            .job_paths
            .as_ref()
            .map(|(_, cancel, _)| std::fs::write(cancel, b"cancel\n"));
        if let Some(Err(e)) = result {
            self.fail(&e.to_string());
        }
    }
    pub fn poll_job(self: Pin<&mut Self>) {
        let progress = self.rust().job_paths.as_ref().and_then(|(path, _, id)| {
            std::fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .filter(|v| v["job_id"].as_str() == Some(id))
                .map(|v| {
                    format!(
                        "{} · {} / {}",
                        v["phase"].as_str().unwrap_or("Running"),
                        v["completed"],
                        v["total"]
                    )
                })
        });
        if let Some(text) = progress {
            self.set_message(QString::from(&text));
        }
    }
    pub fn flush(mut self: Pin<&mut Self>) {
        let result = self
            .as_mut()
            .rust_mut()
            .session
            .as_mut()
            .map(Session::flush);
        if let Some(Err(e)) = result {
            self.fail(&e.to_string());
        }
    }
    pub fn check_theme(self: Pin<&mut Self>) {}
    pub fn memory_report(&self) -> QString {
        QString::from(&crate::bridge::memory_text())
    }
}

/// Entry actions use the same public snapshot/plugin/artifact boundary as CLI.
pub(crate) fn create_workspace(session: &mut Session, doc: DocId) -> Result<String, String> {
    use basecraft_plugin_host::{FileJob, file_tool::AlignmentInput};
    let output = session
        .root()
        .join(format!("species-{}.json", ulid::Ulid::generate()));
    let plugin = crate::bridge::plugin_dir("delimitation");
    let job = if session.is_tree(doc) {
        FileJob::from_tree(session, doc, (&plugin, "workspace_tree"), output)
    } else {
        FileJob::from_alignment(
            session,
            doc,
            (&plugin, "workspace_alignment"),
            AlignmentInput::Aligned,
            output,
        )
    }
    .map_err(|e| e.to_string())?;
    let started = std::time::Instant::now();
    let result = job.run();
    job.record_terminal(session, doc, &result, started.elapsed().as_secs_f64())
        .map_err(|e| e.to_string())?;
    let result = result.map_err(|e| e.to_string())?;
    job.record(session, doc, &result)
        .map_err(|e| e.to_string())?;
    result
        .outputs
        .into_iter()
        .find(|p| {
            Path::new(p)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("spart"))
        })
        .ok_or_else(|| "No workspace artifact returned".into())
}
