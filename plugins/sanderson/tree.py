"""Strict, bounded Newick handling for native Sanderson PL transport.

The parser accepts one explicitly bifurcating rooted tree. It never changes a
branch length, label, or topology to accommodate an engine limitation.
"""

import math
import re
from dataclasses import dataclass, field

NUMBER = re.compile(r"(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?")
MAX_BYTES = 1_000_000
MAX_NODES = 4095


@dataclass
class Node:
    label: str
    length_text: str | None
    children: list["Node"] = field(default_factory=list)
    key: str = ""

    @property
    def length(self):
        return None if self.length_text is None else float(self.length_text)

    def walk(self):
        yield self
        for child in self.children:
            yield from child.walk()


class Parser:
    def __init__(self, source):
        if not isinstance(source, str) or len(source.encode("utf-8")) > MAX_BYTES:
            raise ValueError("Expected one Newick tree of at most 1 MB")
        self.source = source
        self.pos = 0
        self.nodes = 0

    def space(self):
        while self.pos < len(self.source) and self.source[self.pos].isspace():
            self.pos += 1

    def label(self):
        self.space()
        s = self.source
        if self.pos < len(s) and s[self.pos] == "'":
            self.pos += 1
            parts = []
            while self.pos < len(s):
                if s[self.pos] == "'":
                    self.pos += 1
                    if self.pos < len(s) and s[self.pos] == "'":
                        parts.append("'")
                        self.pos += 1
                    else:
                        return "".join(parts)
                else:
                    parts.append(s[self.pos])
                    self.pos += 1
            raise ValueError("Unterminated quoted Newick label")
        start = self.pos
        while self.pos < len(s) and s[self.pos] not in "(),:; \t\r\n":
            if s[self.pos] in "[]'":
                raise ValueError("Newick comments and unsupported quoting are not accepted")
            self.pos += 1
        return s[start:self.pos]

    def node(self, depth=0):
        if depth > 256 or self.nodes >= MAX_NODES:
            raise ValueError("Tree exceeds node or nesting limit")
        self.nodes += 1
        self.space()
        children = []
        if self.pos < len(self.source) and self.source[self.pos] == "(":
            self.pos += 1
            while True:
                children.append(self.node(depth + 1))
                self.space()
                if self.pos >= len(self.source):
                    raise ValueError("Unterminated Newick clade")
                token = self.source[self.pos]
                self.pos += 1
                if token == ")":
                    break
                if token != ",":
                    raise ValueError("Expected comma or closing parenthesis")
            if len(children) != 2:
                raise ValueError("Only rooted bifurcating trees are supported")
        label = self.label()
        if not children and not label:
            raise ValueError("Every tip needs a nonempty label")
        self.space()
        length = None
        if self.pos < len(self.source) and self.source[self.pos] == ":":
            self.pos += 1
            self.space()
            match = NUMBER.match(self.source, self.pos)
            if match is None:
                raise ValueError("Missing or invalid branch length")
            length = match.group()
            self.pos = match.end()
            value = float(length)
            if not math.isfinite(value) or value <= 0:
                raise ValueError("Every branch length must be positive and finite")
        return Node(label, length, children)

    def parse(self):
        root = self.node()
        self.space()
        if self.pos >= len(self.source) or self.source[self.pos] != ";":
            raise ValueError("Newick tree must end with a semicolon")
        self.pos += 1
        self.space()
        if self.pos != len(self.source):
            raise ValueError("Only one Newick tree is accepted")
        if not root.children or root.length_text is not None:
            raise ValueError("Root must be explicit, bifurcating and have no stem length")
        tips = [node for node in root.walk() if not node.children]
        if len(tips) < 3 or len({node.label for node in tips}) != len(tips):
            raise ValueError("At least three uniquely named tips are required")
        if any(node.length_text is None for node in root.walk() if node is not root):
            raise ValueError("Every nonroot branch needs a length")
        tip_no = internal_no = 0
        for node in root.walk():
            if node.children:
                internal_no += 1
                node.key = f"N{internal_no:06d}"
            else:
                tip_no += 1
                node.key = f"T{tip_no:06d}"
        return root


def parse(source):
    return Parser(source).parse()


def labeled_newick(node, durations):
    if node.children:
        body = "(" + ",".join(labeled_newick(child, durations) for child in node.children) + ")"
    else:
        body = ""
    label = "'" + node.label.replace("'", "''") + "'" if node.label else ""
    return body + label + (f":{durations[node.key]:.17g}" if node.key in durations else "")
