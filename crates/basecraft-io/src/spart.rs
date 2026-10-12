//! Matricial SPART and SPART-XML interchange, independent of workspace metadata.
//! Text names use reversible UTF-8 hex encoding when SPART delimiters disallow them.
use crate::{Result, error::parse_error};
use basecraft_protocol::delimitation::{
    DelimitationView, DelimitationWorkspace, Group, Hypothesis, HypothesisOrigin, Provenance,
    Score, Specimen,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

fn error(message: impl Into<String>) -> crate::Error {
    parse_error("SPART", 1, message)
}
fn empty(name: &str) -> DelimitationWorkspace {
    DelimitationWorkspace {
        version: 1,
        name: name.into(),
        specimens: vec![],
        hypotheses: vec![],
        view: DelimitationView::default(),
        exchange_fields: BTreeMap::new(),
    }
}
fn hypothesis(id: String, name: String) -> Hypothesis {
    Hypothesis {
        id,
        name,
        origin: HypothesisOrigin::Imported,
        parent: None,
        groups: vec![],
        unassigned: BTreeSet::new(),
        scores: vec![],
        provenance: Provenance {
            method: "SPART import".into(),
            ..Provenance::default()
        },
        attributes: BTreeMap::new(),
    }
}
fn group(id: String, label: String) -> Group {
    Group {
        id,
        label,
        color: String::new(),
        members: BTreeSet::new(),
        scores: vec![],
        individual_scores: BTreeMap::new(),
        attributes: BTreeMap::new(),
    }
}
fn score(value: &str, scale: &str) -> Result<Option<Score>> {
    if value.trim() == "?" || value.trim().is_empty() {
        return Ok(None);
    }
    let value = value
        .trim()
        .parse::<f64>()
        .map_err(|_| error("invalid score"))?;
    if !value.is_finite() {
        return Err(error("nonfinite score"));
    }
    Ok(Some(Score {
        value,
        scale: scale.into(),
        method: "SPART".into(),
        interpretation: "Imported score; see source method".into(),
    }))
}

/// SPART textual identifiers contain only letters, numbers and underscore.
pub fn encode_name(s: &str) -> String {
    if !s.is_empty()
        && !s.starts_with("basecraft_utf8_")
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        s.into()
    } else {
        let mut out = "basecraft_utf8_".to_string();
        for b in s.bytes() {
            write!(out, "{b:02x}").unwrap();
        }
        out
    }
}
fn decode_name(s: &str) -> Result<String> {
    let s = s.trim();
    let Some(hex) = s.strip_prefix("basecraft_utf8_") else {
        return Ok(s.into());
    };
    if hex.len() % 2 != 0 {
        return Err(error("invalid encoded name"));
    }
    let bytes: std::result::Result<Vec<_>, _> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect();
    String::from_utf8(bytes.map_err(|_| error("invalid encoded name"))?)
        .map_err(|_| error("invalid name UTF-8"))
}

pub fn parse_text(text: &str) -> Result<DelimitationWorkspace> {
    let mut clean = String::new();
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => clean.push(c),
            _ => {}
        }
    }
    if depth > 0 {
        return Err(error("unterminated comment"));
    }
    let mut fields = BTreeMap::new();
    let mut begun = false;
    let mut ended = false;
    for statement in clean.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        if statement.eq_ignore_ascii_case("begin spart") {
            begun = true;
            continue;
        }
        if statement.eq_ignore_ascii_case("end") {
            ended = true;
            continue;
        }
        let (key, value) = statement
            .split_once('=')
            .ok_or_else(|| error("expected field = value"))?;
        if fields
            .insert(key.trim().to_ascii_lowercase(), value.trim().to_string())
            .is_some()
        {
            return Err(error("duplicate SPART field"));
        }
    }
    if !begun || !ended {
        return Err(error("missing SPART block"));
    }
    let get = |key: &str| {
        fields
            .get(key)
            .ok_or_else(|| error(format!("missing {key}")))
    };
    let mut work = empty(&decode_name(get("project_name")?)?);
    let (count, names) = get("n_spartitions")?
        .split_once(':')
        .ok_or_else(|| error("invalid partition header"))?;
    let count: usize = count
        .trim()
        .parse()
        .map_err(|_| error("invalid partition count"))?;
    let names: Vec<_> = if names.trim().is_empty() {
        vec![]
    } else {
        names.split('/').map(str::trim).collect()
    };
    if names.len() != count {
        return Err(error("partition count mismatch"));
    }
    let subsets: Vec<_> = if count == 0 {
        vec![]
    } else {
        get("n_subsets")?.split('/').collect()
    };
    if subsets.len() != count {
        return Err(error("subset count mismatch"));
    }
    let types = |key: &str, i: usize| {
        fields
            .get(key)
            .and_then(|s| s.split('/').nth(i))
            .map_or("unspecified", str::trim)
    };
    for (i, name) in names.iter().enumerate() {
        let (name, value) = name.split_once(',').unwrap_or((name, "?"));
        let mut h = hypothesis(format!("partition-{}", i + 1), decode_name(name)?);
        if let Some(s) = score(value, types("spartition_score_type", i))? {
            h.scores.push(s);
        }
        let (n, values) = subsets[i].split_once(':').unwrap_or((subsets[i], ""));
        let n: usize = n
            .trim()
            .parse()
            .map_err(|_| error("invalid subset count"))?;
        let values: Vec<_> = values.split(',').collect();
        for j in 0..n {
            let mut g = group(format!("group-{}", j + 1), (j + 1).to_string());
            if let Some(value) = values.get(j) {
                if let Some(s) = score(value, types("subset_score_type", i))? {
                    g.scores.push(s);
                }
            }
            h.groups.push(g);
        }
        work.hypotheses.push(h);
    }
    let mut ids = BTreeSet::new();
    for line in get("individual_assignment")?
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let (name, assignments) = line
            .split_once(':')
            .ok_or_else(|| error("invalid assignment row"))?;
        let id = decode_name(name)?;
        if !ids.insert(id.clone()) {
            return Err(error("duplicate specimen name"));
        }
        work.specimens.push(Specimen {
            label: id.clone(),
            id: id.clone(),
            sources: BTreeMap::new(),
            attributes: BTreeMap::new(),
        });
        let assignments: Vec<_> = assignments.split('/').map(str::trim).collect();
        if assignments.len() != count {
            return Err(error("assignment column count mismatch"));
        }
        for (h, assignment) in work.hypotheses.iter_mut().zip(assignments) {
            if assignment == "?" {
                h.unassigned.insert(id.clone());
            } else {
                let k: usize = assignment
                    .parse()
                    .map_err(|_| error("invalid group number"))?;
                let g = k
                    .checked_sub(1)
                    .and_then(|k| h.groups.get_mut(k))
                    .ok_or_else(|| error("unknown group number"))?;
                g.members.insert(id.clone());
            }
        }
    }
    if let Some(rows) = fields.get("individual_score") {
        for line in rows.lines().map(str::trim).filter(|s| !s.is_empty()) {
            let (name, values) = line
                .split_once(':')
                .ok_or_else(|| error("invalid individual score row"))?;
            let id = decode_name(name)?;
            if !ids.contains(&id) {
                return Err(error("unknown individual score reference"));
            }
            let values: Vec<_> = values.split('/').collect();
            if values.len() != count {
                return Err(error("individual score count mismatch"));
            }
            for (i, h) in work.hypotheses.iter_mut().enumerate() {
                if let Some(s) = score(values[i], types("individual_score_type", i))? {
                    let g = h
                        .groups
                        .iter_mut()
                        .find(|g| g.members.contains(&id))
                        .ok_or_else(|| error("score for unassigned specimen"))?;
                    g.individual_scores.insert(id.clone(), vec![s]);
                }
            }
        }
    }
    let known = [
        "project_name",
        "n_spartitions",
        "n_subsets",
        "individual_assignment",
        "individual_score",
        "n_individuals",
        "spartition_score_type",
        "subset_score_type",
        "individual_score_type",
    ];
    work.exchange_fields = fields
        .into_iter()
        .filter(|(k, _)| !known.contains(&k.as_str()))
        .collect();
    Ok(work)
}

/// Loss notices are separate from the portable assignments. Full metadata belongs in the sidecar.
pub fn write_text(work: &DelimitationWorkspace) -> (String, Vec<String>) {
    let mut out = format!(
        "begin spart;\nproject_name = {};\ndate = ?;\nn_spartitions = {} : ",
        encode_name(&work.name),
        work.hypotheses.len()
    );
    out.push_str(
        &work
            .hypotheses
            .iter()
            .map(|h| format!("{}, {}", encode_name(&h.name), score_text(h.scores.first())))
            .collect::<Vec<_>>()
            .join(" / "),
    );
    if let Some(date) = work.exchange_fields.get("date") {
        out = out.replace("date = ?;", &format!("date = {date};"));
    }
    out.push_str(";\nn_individuals = ");
    out.push_str(
        &work
            .hypotheses
            .iter()
            .map(|h| (work.specimens.len() - h.unassigned.len()).to_string())
            .collect::<Vec<_>>()
            .join(" / "),
    );
    out.push_str(";\nn_subsets = ");
    out.push_str(
        &work
            .hypotheses
            .iter()
            .map(|h| {
                format!(
                    "{}: {}",
                    h.groups.len(),
                    h.groups
                        .iter()
                        .map(|g| score_text(g.scores.first()))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join(" / "),
    );
    for (field, which) in [("spartition_score_type", 0), ("subset_score_type", 1)] {
        write!(out, ";\n{field} = ").unwrap();
        out.push_str(
            &work
                .hypotheses
                .iter()
                .map(|h| {
                    let score = match which {
                        0 => h.scores.first(),
                        1 => h.groups.iter().find_map(|g| g.scores.first()),
                        _ => h
                            .groups
                            .iter()
                            .find_map(|g| g.individual_scores.values().find_map(|s| s.first())),
                    };
                    score.map_or_else(|| "?".into(), |s| encode_name(&s.scale))
                })
                .collect::<Vec<_>>()
                .join(" / "),
        );
    }
    out.push_str(";\nindividual_assignment =\n");
    for specimen in &work.specimens {
        writeln!(
            out,
            "{} : {}",
            encode_name(&specimen.id),
            work.hypotheses
                .iter()
                .map(|h| h
                    .groups
                    .iter()
                    .position(|g| g.members.contains(&specimen.id))
                    .map_or_else(|| "?".into(), |i| (i + 1).to_string()))
                .collect::<Vec<_>>()
                .join(" / ")
        )
        .unwrap();
    }
    out.push_str(";\nindividual_score =\n");
    for specimen in &work.specimens {
        writeln!(
            out,
            "{} : {}",
            encode_name(&specimen.id),
            work.hypotheses
                .iter()
                .map(|h| score_text(h.groups.iter().find_map(|g| {
                    g.individual_scores
                        .get(&specimen.id)
                        .and_then(|s| s.first())
                })))
                .collect::<Vec<_>>()
                .join(" / ")
        )
        .unwrap();
    }
    out.push_str(";\nindividual_score_type = ");
    out.push_str(
        &work
            .hypotheses
            .iter()
            .map(|h| {
                h.groups
                    .iter()
                    .find_map(|g| g.individual_scores.values().find_map(|s| s.first()))
                    .map_or_else(|| "?".into(), |s| encode_name(&s.scale))
            })
            .collect::<Vec<_>>()
            .join(" / "),
    );
    out.push_str(";\nend;\n");
    let mut notices = vec!["SPART text stores assignments and one score per level; stable IDs, labels, lineage, colors and complete provenance require the Basecraft sidecar".into()];
    if work.hypotheses.iter().any(|h| !h.unassigned.is_empty()) {
        notices.push("Unassigned specimens use '?'; SpartParser revision 2026-10-12 does not accept this field in its matricial reader; use SPART-XML for independent partial-coverage exchange".into());
    }
    (out, notices)
}
fn score_text(score: Option<&Score>) -> String {
    score.map_or_else(|| "?".into(), |s| s.value.to_string())
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn write_xml(work: &DelimitationWorkspace) -> String {
    let mut out = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root>\n<individuals>\n".to_string();
    for s in &work.specimens {
        write!(out, "<individual id=\"{}\"", escape(&s.id)).unwrap();
        for (key, value) in &s.attributes {
            if xml_name(key) && key != "id" {
                write!(out, " {key}=\"{}\"", escape(value)).unwrap();
            }
        }
        out.push_str("/>\n");
    }
    out.push_str("</individuals>\n<spartitions>\n");
    for h in &work.hypotheses {
        write!(out, "<spartition label=\"{}\"", escape(&h.name)).unwrap();
        if let Some(s) = h.scores.first() {
            write!(
                out,
                " spartitionScore=\"{}\" spartitionScoreType=\"{}\"",
                s.value,
                escape(&s.scale)
            )
            .unwrap();
        }
        if let Some(s) = h.groups.iter().find_map(|g| g.scores.first()) {
            write!(out, " subsetScoreType=\"{}\"", escape(&s.scale)).unwrap();
        }
        if let Some(s) = h
            .groups
            .iter()
            .find_map(|g| g.individual_scores.values().find_map(|s| s.first()))
        {
            write!(out, " individualScoreType=\"{}\"", escape(&s.scale)).unwrap();
        }
        for (key, value) in &h.attributes {
            if xml_name(key)
                && ![
                    "label",
                    "spartitionScore",
                    "spartitionScoreType",
                    "subsetScoreType",
                    "individualScoreType",
                ]
                .contains(&key.as_str())
            {
                write!(out, " {key}=\"{}\"", escape(value)).unwrap();
            }
        }
        out.push_str("><subsets>\n");
        for g in &h.groups {
            write!(out, "<subset label=\"{}\"", escape(&g.label)).unwrap();
            if let Some(s) = g.scores.first() {
                write!(out, " score=\"{}\"", s.value).unwrap();
            }
            for (key, value) in &g.attributes {
                if xml_name(key) && !["label", "score"].contains(&key.as_str()) {
                    write!(out, " {key}=\"{}\"", escape(value)).unwrap();
                }
            }
            out.push_str(">\n");
            for id in &g.members {
                write!(out, "<individual ref=\"{}\"", escape(id)).unwrap();
                if let Some(s) = g.individual_scores.get(id).and_then(|s| s.first()) {
                    write!(out, " score=\"{}\"", s.value).unwrap();
                }
                out.push_str("/>\n");
            }
            out.push_str("</subset>\n");
        }
        out.push_str("</subsets></spartition>\n");
    }
    out.push_str("</spartitions>\n");
    for (key, value) in &work.exchange_fields {
        if key.starts_with("xml:") {
            out.push_str(value);
            out.push('\n');
        }
    }
    out.push_str("</root>\n");
    out
}
fn xml_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && name.as_bytes()[0].is_ascii_alphabetic()
}

pub fn parse_xml(text: &str) -> Result<DelimitationWorkspace> {
    // DTDs/entities disabled by the parser defaults; bounded input/node budget.
    if text.len() > 100 * 1024 * 1024 {
        return Err(error("SPART-XML exceeds 100 MiB limit"));
    }
    let xml = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            nodes_limit: 2_000_000,
            ..Default::default()
        },
    )
    .map_err(|e| error(e.to_string()))?;
    let root = xml.root_element();
    if root.tag_name().name() != "root" {
        return Err(error("expected SPART-XML root"));
    }
    let mut work = empty("SPART-XML");
    let individuals = root
        .children()
        .find(|n| n.has_tag_name("individuals"))
        .ok_or_else(|| error("missing individuals"))?;
    let mut ids = BTreeSet::new();
    for n in individuals
        .children()
        .filter(|n| n.has_tag_name("individual"))
    {
        let id = n
            .attribute("id")
            .ok_or_else(|| error("missing individual ID"))?
            .to_string();
        if !ids.insert(id.clone()) {
            return Err(error("duplicate individual ID"));
        }
        work.specimens.push(Specimen {
            label: id.clone(),
            id,
            sources: BTreeMap::new(),
            attributes: n
                .attributes()
                .filter(|a| a.name() != "id")
                .map(|a| (a.name().into(), a.value().into()))
                .collect(),
        });
    }
    let spartitions = root
        .children()
        .find(|n| n.has_tag_name("spartitions"))
        .ok_or_else(|| error("missing spartitions"))?;
    for (i, n) in spartitions
        .children()
        .filter(|n| n.has_tag_name("spartition"))
        .enumerate()
    {
        let mut h = hypothesis(
            format!("partition-{}", i + 1),
            n.attribute("label")
                .ok_or_else(|| error("missing partition label"))?
                .into(),
        );
        h.attributes = n
            .attributes()
            .filter(|a| !["label", "spartitionScore", "spartitionScoreType"].contains(&a.name()))
            .map(|a| (a.name().into(), a.value().into()))
            .collect();
        if let Some(value) = n.attribute("spartitionScore") {
            if let Some(s) = score(
                value,
                n.attribute("spartitionScoreType").unwrap_or("unspecified"),
            )? {
                h.scores.push(s);
            }
        }
        let subsets = n
            .children()
            .find(|n| n.has_tag_name("subsets"))
            .ok_or_else(|| error("missing subsets"))?;
        let mut assigned = BTreeSet::new();
        for (j, s) in subsets
            .children()
            .filter(|n| n.has_tag_name("subset"))
            .enumerate()
        {
            let mut g = group(
                format!("group-{}", j + 1),
                s.attribute("label")
                    .ok_or_else(|| error("missing subset label"))?
                    .into(),
            );
            g.attributes = s
                .attributes()
                .filter(|a| !["label", "score"].contains(&a.name()))
                .map(|a| (a.name().into(), a.value().into()))
                .collect();
            if let Some(value) = s.attribute("score") {
                if let Some(sc) = score(
                    value,
                    n.attribute("subsetScoreType").unwrap_or("unspecified"),
                )? {
                    g.scores.push(sc);
                }
            }
            for m in s.children().filter(|n| n.has_tag_name("individual")) {
                let id = m
                    .attribute("ref")
                    .ok_or_else(|| error("missing individual reference"))?
                    .to_string();
                if !ids.contains(&id) || !assigned.insert(id.clone()) {
                    return Err(error("unknown or overlapping individual reference"));
                }
                g.members.insert(id.clone());
                if let Some(value) = m.attribute("score") {
                    if let Some(sc) = score(
                        value,
                        n.attribute("individualScoreType").unwrap_or("unspecified"),
                    )? {
                        g.individual_scores.insert(id, vec![sc]);
                    }
                }
            }
            h.groups.push(g);
        }
        h.unassigned = ids.difference(&assigned).cloned().collect();
        work.hypotheses.push(h);
    }
    for n in root.children().filter(roxmltree::Node::is_element) {
        if !["individuals", "spartitions"].contains(&n.tag_name().name()) {
            work.exchange_fields.insert(
                format!("xml:{}", n.tag_name().name()),
                text[n.range()].into(),
            );
        }
    }
    Ok(work)
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &str = include_str!("../../../testdata/fixtures/delimitation/manual.spart");
    #[test]
    fn text_and_xml_keep_partial_disjoint_groups_and_scores() {
        let work = parse_text(FIXTURE).unwrap();
        assert_eq!(
            work.hypotheses[0].groups[0].members,
            ["a".into(), "c".into()].into()
        );
        assert_eq!(work.hypotheses[0].scores[0].value, -12.5);
        for round in [
            parse_text(&write_text(&work).0).unwrap(),
            parse_xml(&write_xml(&work)).unwrap(),
        ] {
            for (h, g) in work.hypotheses.iter().zip(round.hypotheses) {
                assert_eq!(
                    h.groups.iter().map(|g| &g.members).collect::<Vec<_>>(),
                    g.groups.iter().map(|g| &g.members).collect::<Vec<_>>()
                );
                assert_eq!(h.unassigned, g.unassigned);
                assert_eq!(
                    h.scores.first().map(|s| s.value),
                    g.scores.first().map(|s| s.value)
                );
            }
        }
    }
    #[test]
    fn independent_parser_generated_xml_is_readable() {
        let complete = parse_xml(include_str!(
            "../../../testdata/fixtures/delimitation/spartparser-complete.xml"
        ))
        .unwrap();
        let partial = parse_xml(include_str!(
            "../../../testdata/fixtures/delimitation/spartparser-partial.xml"
        ))
        .unwrap();
        assert_eq!(complete.hypotheses.len(), 2);
        assert_eq!(partial.hypotheses.len(), 3);
        assert_eq!(
            partial.hypotheses[2].unassigned,
            ["b".into(), "d".into()].into()
        );
    }
    #[test]
    fn names_roundtrip_and_dtd_is_rejected() {
        let mut work = parse_text(FIXTURE).unwrap();
        work.name = "Study: α & β".into();
        work.hypotheses[0].name = "A/B < C".into();
        work.specimens[0].id = "é & <'\"".into();
        work.specimens[0].label = work.specimens[0].id.clone();
        for h in &mut work.hypotheses {
            for g in &mut h.groups {
                if g.members.remove("a") {
                    g.members.insert(work.specimens[0].id.clone());
                }
            }
        }
        let text = parse_text(&write_text(&work).0).unwrap();
        assert_eq!(text.name, work.name);
        assert_eq!(text.specimens[0].id, work.specimens[0].id);
        let xml = parse_xml(&write_xml(&work)).unwrap();
        assert_eq!(xml.specimens[0].id, work.specimens[0].id);
        assert_eq!(xml.hypotheses[0].name, work.hypotheses[0].name);
        assert!(
            parse_xml("<!DOCTYPE root [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><root>&x;</root>")
                .is_err()
        );
        assert!(parse_text(&FIXTURE.replace("a : 1 / 1 / 1", "a : 99 / 1 / 1")).is_err());
        assert!(parse_text(&FIXTURE.replace("b : 2 / 1 / ?", "a : 2 / 1 / ?")).is_err());
    }
}
