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
    pub nodes: Vec<ParsedNode>,
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
            p.skip_space();
            if p.at_end() {
                break;
            }
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
    let lower = text.to_ascii_lowercase();
    let Some(start) = lower.find("begin trees;") else {
        return Err(parse_error("NEXUS", 1, "no TREES block"));
    };
    let body_start = start + "begin trees;".len();
    let end = lower[body_start..]
        .find("end;")
        .or_else(|| lower[body_start..].find("endblock;"))
        .map_or(text.len(), |e| body_start + e);
    let mut translate: Vec<(String, String)> = Vec::new();
    let mut trees = Vec::new();
    let mut p = Parser::new(&text[..end], body_start);
    loop {
        p.skip_space();
        if p.at_end() {
            break;
        }
        let word = p.word().to_ascii_lowercase();
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
            "" => {
                p.next();
            }
            _ => {
                // Another command: skip to its semicolon.
                while let Some(b) = p.next() {
                    if b == b';' {
                        break;
                    }
                }
            }
        }
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
        // A rooting comment such as [&R] before the tree isn't the root's.
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
    fn quoting() {
        assert_eq!(quote_name("Codium_fragile"), "Codium_fragile");
        assert_eq!(quote_name("Udotea sp. 1"), "'Udotea sp. 1'");
        assert_eq!(quote_name("it's"), "'it''s'");
        let t = parse_newick(&format!("({},B);", quote_name("it's (x)"))).unwrap();
        assert_eq!(tips(&t)[0], "it's (x)");
    }
}
