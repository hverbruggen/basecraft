//! Newick and NEXUS tree files.
//!
//! Reads Newick (one or more trees, each ending in `;`) and the TREES block
//! of a NEXUS file, with its TRANSLATE table. Node names are kept as
//! written, underscores included; quoted names lose their quotes. Comments
//! starting with `&`, such as `MrBayes`' `[&prob=0.99]`, are kept with the
//! node they follow; other comments are dropped.

use crate::Result;
use crate::error::parse_error;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedNode {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// A tip's name, or an internal node's label (often a support value).
    pub name: String,
    pub length: Option<f64>,
    /// `&` comments, without brackets, joined with `,`.
    pub comment: String,
}

/// A tree as read: node 0 is the root.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedTree {
    pub name: String,
    /// Explicit rooting declaration; None means the exchange made no claim.
    pub rooted: Option<bool>,
    pub nodes: Vec<ParsedNode>,
}

/// Write a parsed member without converting support comments into editor data.
/// Untouched forest members retain their node annotations and exchange rooting.
pub fn write_parsed(tree: &ParsedTree, w: &mut dyn std::io::Write) -> std::io::Result<()> {
    enum Part {
        Node(usize),
        Label(usize),
        Text(&'static str),
    }
    let prefix = match tree.rooted {
        Some(true) => "[&R] ",
        Some(false) => "[&U] ",
        None => "",
    };
    w.write_all(prefix.as_bytes())?;
    let mut pending = vec![Part::Node(0)];
    let mut visited = 0;
    while let Some(part) = pending.pop() {
        match part {
            Part::Text(text) => w.write_all(text.as_bytes())?,
            Part::Node(id) => {
                visited += 1;
                if visited > tree.nodes.len() {
                    return Err(std::io::Error::other("invalid parsed tree cycle"));
                }
                let node = tree
                    .nodes
                    .get(id)
                    .ok_or_else(|| std::io::Error::other("invalid parsed tree node"))?;
                pending.push(Part::Label(id));
                if !node.children.is_empty() {
                    pending.push(Part::Text(")"));
                    for (index, child) in node.children.iter().enumerate().rev() {
                        pending.push(Part::Node(*child));
                        if index > 0 {
                            pending.push(Part::Text(","));
                        }
                    }
                    pending.push(Part::Text("("));
                }
            }
            Part::Label(id) => {
                let node = tree
                    .nodes
                    .get(id)
                    .ok_or_else(|| std::io::Error::other("invalid parsed tree node"))?;
                w.write_all(quote_name(&node.name).as_bytes())?;
                if let Some(length) = node.length {
                    write!(w, ":{length}")?;
                }
                if !node.comment.is_empty() {
                    write!(w, "[&{}]", node.comment)?;
                }
            }
        }
    }
    w.write_all(b";")
}

/// Whether `text` looks like a NEXUS file.
pub fn is_nexus(text: &str) -> bool {
    text.trim_start()
        .get(..6)
        .is_some_and(|s| s.eq_ignore_ascii_case("#nexus"))
}

/// Every tree in a Newick or NEXUS file.
pub fn parse_trees(text: &str) -> Result<Vec<ParsedTree>> {
    if is_nexus(text) {
        parse_nexus(text)
    } else {
        let mut trees = Vec::new();
        let mut p = Parser::new(text, 0);
        loop {
            let before_space = p.pos;
            p.skip_space();
            if p.at_end() {
                break;
            }
            p.pos = before_space;
            let mut t = p.tree()?;
            t.name = format!("tree{}", trees.len() + 1);
            trees.push(t);
        }
        if trees.is_empty() {
            return Err(parse_error("Newick", 1, "no tree in the file"));
        }
        Ok(trees)
    }
}

/// One Newick tree, `;` optional.
pub fn parse_newick(text: &str) -> Result<ParsedTree> {
    Parser::new(text, 0).tree()
}

fn parse_nexus(text: &str) -> Result<Vec<ParsedTree>> {
    // Commands end at semicolons outside quotes and nested comments. In
    // particular, a quoted taxon or comment containing "end;" is not a boundary.
    let header_end = text.len() - text.trim_start().len() + "#NEXUS".len();
    let mut commands = Parser::new(text, header_end);
    let mut translate: Vec<(String, String)> = Vec::new();
    let mut trees = Vec::new();
    let mut block = String::new();
    let mut found = false;
    while let Some((start, end)) = commands.nexus_command()? {
        let mut p = Parser::new(&text[..end], start);
        p.skip_space();
        let word = p.word().to_ascii_lowercase();
        if block.is_empty() {
            if word == "begin" {
                p.skip_space();
                block = p.word().to_ascii_lowercase();
                p.skip_space();
                if p.next() != Some(b';') {
                    return Err(p.error("expected ; after NEXUS block name"));
                }
                found |= block == "trees";
            }
            continue;
        }
        if word == "end" || word == "endblock" {
            p.skip_space();
            if p.next() != Some(b';') {
                return Err(p.error("expected ; after NEXUS block end"));
            }
            if block == "trees" {
                break;
            }
            block.clear();
            continue;
        }
        if block != "trees" {
            continue;
        }
        match word.as_str() {
            "translate" => loop {
                p.skip_space();
                let key = p.name()?;
                p.skip_space();
                let value = p.name()?;
                translate.push((key, value));
                p.skip_space();
                match p.next() {
                    Some(b',') => {}
                    Some(b';') => break,
                    _ => return Err(p.error("expected , or ; in TRANSLATE")),
                }
            },
            "tree" | "utree" => {
                p.skip_space();
                if p.peek() == Some(b'*') {
                    p.next();
                    p.skip_space();
                }
                let name = p.name()?;
                p.skip_space();
                if p.next() != Some(b'=') {
                    return Err(p.error("expected = after the tree's name"));
                }
                let mut t = p.tree()?;
                if word == "utree" {
                    if t.rooted == Some(true) {
                        return Err(p.error("UTREE contradicts an explicit rooted declaration"));
                    }
                    t.rooted = Some(false);
                }
                t.name = name;
                for n in &mut t.nodes {
                    if n.children.is_empty()
                        && let Some((_, v)) = translate.iter().find(|(k, _)| *k == n.name)
                    {
                        n.name.clone_from(v);
                    }
                }
                trees.push(t);
            }
            _ => {} // Other commands were already scanned as a complete unit.
        }
    }
    if !found {
        return Err(parse_error("NEXUS", 1, "no TREES block"));
    }
    if trees.is_empty() {
        return Err(parse_error("NEXUS", 1, "no tree in the TREES block"));
    }
    Ok(trees)
}

struct Parser<'a> {
    text: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str, pos: usize) -> Self {
        Self {
            text: text.as_bytes(),
            pos,
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.text.len()
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    fn line(&self) -> usize {
        1 + memchr::memchr_iter(b'\n', &self.text[..self.pos.min(self.text.len())]).count()
    }

    fn error(&self, message: &str) -> crate::Error {
        parse_error("Newick", self.line(), message)
    }

    /// Skip whitespace and plain comments; return `&` comments met.
    fn skip_space_keep(&mut self) -> Vec<String> {
        let mut kept = Vec::new();
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() {
                self.pos += 1;
            } else if b == b'[' {
                let start = self.pos + 1;
                let mut depth = 0;
                while let Some(c) = self.next() {
                    match c {
                        b'[' => depth += 1,
                        b']' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let body = &self.text[start..self.pos.saturating_sub(1).max(start)];
                if body.first() == Some(&b'&') {
                    kept.push(String::from_utf8_lossy(&body[1..]).into_owned());
                }
            } else {
                break;
            }
        }
        kept
    }

    fn skip_space(&mut self) {
        self.skip_space_keep();
    }

    /// Return a complete NEXUS command's byte range, including its terminator.
    /// No allocation of command bodies or quoted names is needed for scanning.
    fn nexus_command(&mut self) -> Result<Option<(usize, usize)>> {
        let start = self.pos;
        let mut quote = None;
        let mut depth = 0usize;
        let mut content = false;
        while let Some(b) = self.next() {
            if depth > 0 {
                if b == b'[' {
                    depth += 1;
                } else if b == b']' {
                    depth -= 1;
                }
            } else if let Some(q) = quote {
                if b == q {
                    if self.peek() == Some(q) {
                        self.pos += 1;
                    } else {
                        quote = None;
                    }
                }
            } else {
                match b {
                    b'[' => depth = 1,
                    b'\'' | b'"' => {
                        quote = Some(b);
                        content = true;
                    }
                    b';' => return Ok(Some((start, self.pos))),
                    _ => content |= !b.is_ascii_whitespace(),
                }
            }
        }
        if quote.is_some() || depth > 0 || content {
            return Err(self.error("unterminated NEXUS command, quote or comment"));
        }
        Ok(None)
    }

    /// A bare word (letters, digits, `_`, `.`), for NEXUS commands.
    fn word(&mut self) -> String {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        {
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.text[start..self.pos]).into_owned()
    }

    /// A quoted or unquoted name; may be empty.
    fn name(&mut self) -> Result<String> {
        if self.peek() == Some(b'\'') || self.peek() == Some(b'"') {
            let quote = self.next().unwrap_or(b'\'');
            let mut out = Vec::new();
            loop {
                match self.next() {
                    None => return Err(self.error("unterminated quoted name")),
                    Some(b) if b == quote => {
                        // A doubled quote stands for one.
                        if self.peek() == Some(quote) {
                            self.pos += 1;
                            out.push(quote);
                        } else {
                            break;
                        }
                    }
                    Some(b) => out.push(b),
                }
            }
            return Ok(String::from_utf8_lossy(&out).into_owned());
        }
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b"(),:;[]=".contains(&b) || b.is_ascii_whitespace() {
                break;
            }
            self.pos += 1;
        }
        Ok(String::from_utf8_lossy(&self.text[start..self.pos]).into_owned())
    }

    /// One tree up to and including its `;` (or the end of the text).
    fn tree(&mut self) -> Result<ParsedTree> {
        let mut nodes: Vec<ParsedNode> = vec![ParsedNode::default()];
        // Nodes whose ( is still open.
        let mut open: Vec<usize> = Vec::new();
        let mut started = false;
        let mut current = 0usize;
        let mut expecting_node = true;
        let mut comments = self.skip_space_keep();
        // Rooting is tree-level exchange meaning, not a node annotation.
        let mut rooted = None;
        for comment in &comments {
            let value = if comment.eq_ignore_ascii_case("r") {
                Some(true)
            } else if comment.eq_ignore_ascii_case("u") {
                Some(false)
            } else {
                None
            };
            if let Some(value) = value {
                if rooted.is_some_and(|previous| previous != value) {
                    return Err(self.error("contradictory tree rooting declarations"));
                }
                rooted = Some(value);
            }
        }
        comments.retain(|c| !c.eq_ignore_ascii_case("r") && !c.eq_ignore_ascii_case("u"));
        nodes[0].comment = comments.join(",");
        loop {
            for k in self.skip_space_keep() {
                let c = &mut nodes[current].comment;
                if !c.is_empty() {
                    c.push(',');
                }
                c.push_str(&k);
            }
            match self.peek() {
                None | Some(b';') => {
                    if self.peek().is_some() {
                        self.pos += 1;
                    }
                    if !open.is_empty() {
                        return Err(self.error("the tree ends before its last )"));
                    }
                    if !started {
                        return Err(self.error("empty tree"));
                    }
                    break;
                }
                Some(b'(') => {
                    self.pos += 1;
                    if !expecting_node {
                        return Err(self.error("unexpected ("));
                    }
                    let id = match open.last() {
                        None if !started => 0,
                        None => return Err(self.error("text after the tree's last )")),
                        Some(&parent) => add_child(&mut nodes, parent, String::new()),
                    };
                    started = true;
                    open.push(id);
                    current = id;
                }
                Some(b',') => {
                    self.pos += 1;
                    if open.is_empty() {
                        return Err(self.error(", outside parentheses"));
                    }
                    expecting_node = true;
                }
                Some(b')') => {
                    self.pos += 1;
                    current = open.pop().ok_or_else(|| self.error("unbalanced )"))?;
                    // Its label and length follow.
                    nodes[current].name = self.name()?;
                    expecting_node = false;
                }
                Some(b':') => {
                    self.pos += 1;
                    self.skip_space();
                    let start = self.pos;
                    while self
                        .peek()
                        .is_some_and(|b| b.is_ascii_digit() || b"+-.eE".contains(&b))
                    {
                        self.pos += 1;
                    }
                    let s = std::str::from_utf8(&self.text[start..self.pos]).unwrap_or("");
                    let len: f64 = s
                        .parse()
                        .map_err(|_| self.error(&format!("bad branch length {s:?}")))?;
                    nodes[current].length = Some(len);
                }
                Some(_) => {
                    if !expecting_node {
                        return Err(self.error("unexpected text after a node"));
                    }
                    let name = self.name()?;
                    if name.is_empty() {
                        return Err(self.error("unexpected character"));
                    }
                    current = match open.last() {
                        // A tree of one tip.
                        None if !started => {
                            nodes[0].name = name;
                            0
                        }
                        None => return Err(self.error("text after the tree's last )")),
                        Some(&parent) => add_child(&mut nodes, parent, name),
                    };
                    started = true;
                    expecting_node = false;
                }
            }
        }
        Ok(ParsedTree {
            name: String::new(),
            rooted,
            nodes,
        })
    }
}

fn add_child(nodes: &mut Vec<ParsedNode>, parent: usize, name: String) -> usize {
    let id = nodes.len();
    nodes.push(ParsedNode {
        parent: Some(parent),
        name,
        ..ParsedNode::default()
    });
    nodes[parent].children.push(id);
    id
}

/// A name as Newick writes it: quoted when it holds characters Newick
/// reserves, or spaces.
pub fn quote_name(name: &str) -> String {
    let plain = !name.is_empty()
        && !name
            .bytes()
            .any(|b| b"()[]',:;=".contains(&b) || b.is_ascii_whitespace());
    if plain {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// A branch length, as short as it reads back exactly.
pub fn format_length(len: f64) -> String {
    format!("{len}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tips(t: &ParsedTree) -> Vec<&str> {
        t.nodes
            .iter()
            .filter(|n| n.children.is_empty())
            .map(|n| n.name.as_str())
            .collect()
    }

    #[test]
    fn reads_lengths_labels_and_comments() {
        let t = parse_newick("((A:0.1,'B c':0.2)95:0.05[&prob=0.99],(C,D)80/1:0.3)root;").unwrap();
        assert_eq!(tips(&t), ["A", "B c", "C", "D"]);
        assert_eq!(t.nodes[0].name, "root");
        assert_eq!(t.nodes[0].children.len(), 2);
        let ab = t.nodes[0].children[0];
        assert_eq!(t.nodes[ab].name, "95");
        assert_eq!(t.nodes[ab].length, Some(0.05));
        assert_eq!(t.nodes[ab].comment, "prob=0.99");
        let cd = t.nodes[0].children[1];
        assert_eq!(t.nodes[cd].name, "80/1");
        let a = t.nodes[ab].children[0];
        assert_eq!(t.nodes[a].parent, Some(ab));
        assert_eq!(t.nodes[a].length, Some(0.1));
    }

    #[test]
    fn reads_several_trees_and_one_tip() {
        let trees = parse_trees("(A,B);\n((A,B),C);\n").unwrap();
        assert_eq!(trees.len(), 2);
        assert_eq!(tips(&trees[1]), ["A", "B", "C"]);
        assert_eq!(tips(&parse_newick("A;").unwrap()), ["A"]);
        assert!(parse_newick("((A,B);").is_err());
        assert!(parse_newick("(A,B));").is_err());
    }

    #[test]
    fn reads_nexus_with_translate() {
        let text = "#NEXUS\n[a comment]\nbegin taxa;\n dimensions ntax=3;\nend;\n\
            begin trees;\n translate\n 1 Alpha,\n 2 'Beta gamma',\n 3 Delta;\n\
            tree con_50 = [&U] ((1:0.1,2:0.2)[&prob=1.0]:0.3,3:0.4);\nend;\n";
        let trees = parse_trees(text).unwrap();
        assert_eq!(trees.len(), 1);
        assert_eq!(trees[0].name, "con_50");
        assert_eq!(tips(&trees[0]), ["Alpha", "Beta gamma", "Delta"]);
        let inner = trees[0].nodes[0].children[0];
        assert_eq!(trees[0].nodes[inner].comment, "prob=1.0");
        assert_eq!(trees[0].nodes[0].comment, "");
    }

    #[test]
    fn nexus_boundaries_ignore_quoted_names_and_nested_comments() {
        let text = r#"#NEXUS
            [begin trees; TREE fake = (wrong,names); end;]
            BEGIN TAXA;
              TAXLABELS 'begin trees; end;' "endblock;";
            END;
            BeGiN [a comment] TrEeS ;
              [end; [nested endblock;] 'quoted' text]
              TRANSLATE 1 'weekend;sample', 2 'it''s endblock;', 3 "begin trees;";
              TITLE 'a title; end;';
              TREE 'named end; tree' = [&R] (1:0.1[&note='end;'],2:0.2,3:0.3);
              [endblock;]
              UTREE next = ('friend;sample':0.4,'x[end;]':0.5);
            EnDbLoCk [end;] ;
            BEGIN TREES; TREE ignored = (other,taxa); END;
        "#;
        let trees = parse_trees(text).unwrap();
        assert_eq!(trees.len(), 2);
        assert_eq!(trees[0].name, "named end; tree");
        assert_eq!(
            tips(&trees[0]),
            ["weekend;sample", "it's endblock;", "begin trees;"]
        );
        assert_eq!(trees[0].nodes[1].length, Some(0.1));
        assert_eq!(trees[0].nodes[1].comment, "note='end;'");
        assert_eq!(tips(&trees[1]), ["friend;sample", "x[end;]"]);
    }

    #[test]
    fn nexus_requires_real_commands_and_preserves_eof_compatibility() {
        assert!(parse_trees("#NEXUS [begin trees; TREE fake = (a,b); end;]").is_err());
        assert!(parse_trees("#NEXUS BEGIN TAXA; TITLE 'begin trees;'; END;").is_err());
        // Existing files may omit the closing END after a complete tree command.
        assert_eq!(
            parse_trees("#NEXUS BEGIN\nTREES; TREE t=(a,b);")
                .unwrap()
                .len(),
            1
        );
        for text in [
            "#NEXUS BEGIN TREES; TITLE 'end;",
            "#NEXUS BEGIN TREES; [unterminated end;",
            "#NEXUS BEGIN TREES; TREE t=('end;sample,b);",
        ] {
            assert!(parse_trees(text).is_err(), "{text}");
        }
    }

    #[test]
    fn quoting() {
        assert_eq!(quote_name("Codium_fragile"), "Codium_fragile");
        assert_eq!(quote_name("Udotea sp. 1"), "'Udotea sp. 1'");
        assert_eq!(quote_name("it's"), "'it''s'");
        let t = parse_newick(&format!("({},B);", quote_name("it's (x)"))).unwrap();
        assert_eq!(tips(&t)[0], "it's (x)");
    }
}
