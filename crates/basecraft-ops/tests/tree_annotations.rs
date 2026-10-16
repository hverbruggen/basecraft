//! Viewer names remain annotations: save, history and links preserve scientific identity.
use basecraft_ops::{Session, files, links::AlignmentLink, replay};
use basecraft_protocol::{CladeAnnotation, Op};

#[test]
fn display_names_preserve_support_tip_identity_links_and_replay() {
    for extension in ["nwk", "nex"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let path = root.join(format!("tree.{extension}"));
        let newick = "((A:1,B:2)95/0.987654321:3,C:4);";
        let text = if extension == "nex" {
            format!("#NEXUS\nbegin trees; tree original = {newick} end;")
        } else {
            newick.into()
        };
        std::fs::write(&path, text).unwrap();
        let source = root.join("source.afa");
        std::fs::write(&source, ">A\nAC\n>B\nAT\n>C\nAG\n").unwrap();
        let mut session = Session::new(root);
        let doc = session.open(&path).unwrap();
        AlignmentLink::associate(&path, session.tree(doc), &source).unwrap();
        let link = std::fs::read(basecraft_ops::links::sidecar(&path)).unwrap();
        let snapshot = AlignmentLink::read(&path)
            .unwrap()
            .checked_snapshot(&path, session.tree(doc))
            .unwrap();
        let snapshot_bytes = std::fs::read(&snapshot).unwrap();
        let scientific = session.tree(doc).to_newick();
        let supports = session.tree(doc).supports().to_vec();
        for (id, clade, label) in [
            (1, vec!["A".into(), "B".into()], "95 / named branch"),
            (2, vec!["A".into()], "Tip display name"),
        ] {
            let mut annotation = CladeAnnotation::new(clade, label);
            annotation.on_branch = true;
            annotation.bar = false;
            session
                .apply(
                    doc,
                    Op::TreeAnnotate {
                        id,
                        annotation: Some(annotation),
                    },
                )
                .unwrap();
        }
        let annotations = session.tree(doc).annotations().clone();
        let mut edited = annotations[&1].clone();
        edited.label = "Numeric display: 0.999".into();
        session
            .apply(
                doc,
                Op::TreeAnnotate {
                    id: 1,
                    annotation: Some(edited),
                },
            )
            .unwrap();
        session.undo(doc).unwrap();
        assert_eq!(session.tree(doc).annotations(), &annotations);
        session.redo(doc).unwrap();
        assert_eq!(
            session.tree(doc).annotations()[&1].label,
            "Numeric display: 0.999"
        );
        assert_eq!(session.tree(doc).to_newick(), scientific);
        assert_eq!(session.tree(doc).supports(), supports);
        session.save(doc, None).unwrap();
        let reopened = files::load_tree(&path).unwrap().doc;
        assert_eq!(reopened.annotations(), session.tree(doc).annotations());
        assert_eq!(reopened.to_newick(), scientific);
        assert_eq!(reopened.supports(), supports);
        assert!(reopened.find_tip("A").is_some());
        assert!(reopened.find_tip("Tip display name").is_none());
        assert_eq!(
            std::fs::read(basecraft_ops::links::sidecar(&path)).unwrap(),
            link
        );
        assert_eq!(std::fs::read(snapshot).unwrap(), snapshot_bytes);
        session.flush().unwrap();
        let report = replay::replay(root).unwrap();
        assert!(report.ok(), "{:#?}", report.problems);
    }
}
