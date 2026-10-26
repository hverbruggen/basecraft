"""Animated ASCII art for the About dialog: a chromatogram trace, a cladogram with species
brackets, an alignment, a sequence loom, a branching B and a paper shuttle.

    python3 generate.py play <name>     play in the terminal (Ctrl-C to stop)
    python3 generate.py list            list the animation names
    python3 generate.py export          regenerate ../../qml/AsciiArt.js

A frame is a list of (chars, styles) row pairs of equal length. Each style character colours
the cell above it: '.' plain, 'd' dim, 'm' moss, 'w' bright, 'A' 'C' 'G' 'T' base colours, and the lower
case of any of these (plus 'x' for plain) inverts the cell.
"""
import json
import math
import random
import sys
import time
from pathlib import Path

# Palette from the dark wordmark: its background, letter ink, sage stem and amber fold.
# C and T are added colours chosen to sit with those.
COLOURS = {".": "#81978f", "d": "#4e5e60", "w": "#f3eee4",
           "m": "#669f8c",   # moss, from the stem's darker gradient stops
           "A": "#93c7ae",   # sage, from the stem
           "C": "#6e9cc4",   # slate blue, added
           "G": "#f0c279",   # amber, from the fold
           "T": "#d97a5b"}   # terracotta, added
INVERSE = {"a": "A", "c": "C", "g": "G", "t": "T", "x": "."}
BG = "#1c2630"
# The same roles for light backgrounds, after the light wordmark: bright becomes dark ink.
COLOURS_LIGHT = {".": "#6f857c", "d": "#b3bcb4", "w": "#1c2630", "m": "#467e75",
                 "A": "#3f8a6e", "C": "#3f6f9e", "G": "#c98a2e", "T": "#c0593f"}
BG_LIGHT = "#f3eee4"
BASES = "ACGT"


class Canvas:
    def __init__(self, width, height):
        self.w, self.h = width, height
        self.chars = [[" "] * width for _ in range(height)]
        self.styles = [["."] * width for _ in range(height)]

    def put(self, x, y, text, style="."):
        for i, ch in enumerate(text):
            if 0 <= x + i < self.w and 0 <= y < self.h:
                self.chars[y][x + i] = ch
                self.styles[y][x + i] = style if len(style) == 1 else style[i]

    def restyle(self, x, y, style):
        if 0 <= x < self.w and 0 <= y < self.h:
            self.styles[y][x] = style

    def frame(self):
        return [("".join(c), "".join(s)) for c, s in zip(self.chars, self.styles)]


class Braille:
    """A dot canvas drawn with braille characters, 2x4 dots per character cell.
    Each dot carries a style key; a cell takes the most common key, preferring keys in prefer."""
    BITS = [[0x1, 0x2, 0x4, 0x40], [0x8, 0x10, 0x20, 0x80]]

    def __init__(self, width, height):
        self.w, self.h = width, height
        self.bits = [[0] * width for _ in range(height)]
        self.keys = [[{} for _ in range(width)] for _ in range(height)]

    def dot(self, x, y, key):
        x, y = int(round(x)), int(round(y))
        if 0 <= x < self.w * 2 and 0 <= y < self.h * 4:
            cx, cy = x // 2, y // 4
            self.bits[cy][cx] |= self.BITS[x % 2][y % 4]
            self.keys[cy][cx][key] = self.keys[cy][cx].get(key, 0) + 1

    def line(self, x0, y0, x1, y1, key, brush=1):
        n = max(int(max(abs(x1 - x0), abs(y1 - y0)) * 2), 1)
        for i in range(n + 1):
            x, y = x0 + (x1 - x0) * i / n, y0 + (y1 - y0) * i / n
            for bx in range(brush):
                for by in range(brush):
                    self.dot(x + bx, y + by, key)

    def polygon(self, pts, key):
        ys = [y for _, y in pts]
        for y in range(int(min(ys)), int(max(ys)) + 1):
            xs = []
            for (xa, ya), (xb, yb) in zip(pts, pts[1:] + pts[:1]):
                if (ya <= y < yb) or (yb <= y < ya):
                    xs.append(xa + (y - ya) * (xb - xa) / (yb - ya))
            xs.sort()
            for xa, xb in zip(xs[::2], xs[1::2]):
                for x in range(int(math.ceil(xa)), int(math.floor(xb)) + 1):
                    self.dot(x, y, key)

    def blit(self, cv, ox=0, oy=0, prefer=(), quiet=("d",)):
        for cy in range(self.h):
            for cx in range(self.w):
                if not self.bits[cy][cx]:
                    continue
                keys = self.keys[cy][cx]
                style = next((k for k in prefer if k in keys), None)
                if style is None:
                    loud = {k: v for k, v in keys.items() if k not in quiet}
                    style = max(loud, key=loud.get) if loud else next(iter(keys))
                cv.put(ox + cx, oy + cy, chr(0x2800 + self.bits[cy][cx]), style)


# --- trace -------------------------------------------------------------------------------------

def trace_frames():
    """A Sanger chromatogram scrolls past a read head; called bases collect in a read below."""
    width, height = 60, 10                 # trace area in characters (braille: 2x4 dots each)
    dw, dh = width * 2, height * 4
    spacing, n = 12, 24                    # dot columns per base; bases in the looping read
    period = spacing * n
    rng = random.Random(7)
    seq = "".join(rng.choice(BASES) for _ in range(n))
    peaks = [(seq[k], k * spacing + rng.uniform(-1.2, 1.2), rng.uniform(0.55, 1.0)) for k in range(n)]
    peaks += [(rng.choice(BASES), rng.uniform(0, period), rng.uniform(0.06, 0.16)) for _ in range(n)]
    cursor = width // 2

    def signal(base, g):
        v = 0.0
        for b, x, h in peaks:
            if b == base:
                d = (g - x + period / 2) % period - period / 2
                v += h * math.exp(-d * d / (2 * 2.7 ** 2))
        return v

    samples = {b: [signal(b, g) for g in range(period)] for b in BASES}
    frames = []
    for f in range(period):
        cv = Canvas(width, height + 3)
        br = Braille(width, height)
        for x in range(dw):
            br.dot(x, dh - 1, "d")
        for b in BASES:
            prev = None
            for x in range(dw):
                v = samples[b][(x + f) % period]
                y = round((1 - min(v, 1.0)) * (dh - 2))
                if v > 0.03:
                    lo, hi = (y, y) if prev is None else (min(prev, y), max(prev, y))
                    for yy in range(lo, hi + 1):
                        br.dot(x, yy, b)
                    prev = y
                else:
                    prev = None
        cv.put(cursor, 0, "▼", "w")
        br.blit(cv, 0, 1)
        for k, b in enumerate(seq):
            for wrap in (0, period, -period):
                col = round(k * spacing + wrap - f) // 2
                if 0 <= col < width:
                    cv.put(col, height + 1, b, b.lower() if col == cursor else b)
        # bases that have passed the read head, most recent on the right
        since = lambda k: (cursor * 2 - (k * spacing - f)) % period
        read = [seq[k] for k in sorted(range(n), key=since)][:20][::-1]
        line = "read " + "".join(read)
        cv.put(0, height + 2, line, "ddddd" + "".join(read))
        frames.append(cv.frame())
    return {"fps": 24, "frames": frames}


# --- cladogram ---------------------------------------------------------------------------------

BOX = {frozenset("lr"): "─", frozenset("ud"): "│", frozenset("rd"): "┌", frozenset("ru"): "└",
       frozenset("lud"): "┤", frozenset("rud"): "├", frozenset("lrd"): "┬", frozenset("lru"): "┴",
       frozenset("lrud"): "┼", frozenset("ld"): "┐", frozenset("lu"): "┘", frozenset("l"): "─",
       frozenset("r"): "─", frozenset("u"): "│", frozenset("d"): "│"}


def cladogram_frames():
    """A tree grows from the root, its tips light up, then species clusters are bracketed one by one."""
    # (name or children, branch length in columns)
    tree = ([([([("a1", 6), ("a2", 6)], 5), ("a3", 11)], 6),
             ([([([("b1", 4), ("b2", 4)], 4), ("b3", 8)], 5),
               ([("c1", 9), ("d1", 9)], 7)], 4)], 3)
    nodes, tips = [], []

    def layout(node, x0):
        sub, length = node
        x = x0 + length
        rec = {"x": x, "parent_x": x0, "kids": [], "name": None}
        nodes.append(rec)
        if isinstance(sub, str):
            rec["name"], rec["row"] = sub, 2 * len(tips)
            tips.append(rec)
        else:
            rec["kids"] = [layout(k, x) for k in sub]
            rec["row"] = (rec["kids"][0]["row"] + rec["kids"][-1]["row"]) // 2
        return rec

    root = layout(tree, 1)
    for rec in nodes:
        rec["clade"] = set()

    def collect(rec):
        rec["clade"] = {id(rec)} | set().union(*(collect(k) for k in rec["kids"])) if rec["kids"] else {id(rec)}
        return rec["clade"]

    collect(root)
    cells = {}

    def link(x, y, d, tag):
        cells.setdefault((x, y), [set(), tag])[0].add(d)
        cells[(x, y)][1] = tag

    for rec in nodes:
        for x in range(rec["parent_x"], rec["x"] + 1):
            if x > rec["parent_x"] or rec is root:
                link(x, rec["row"], "l", ("h", id(rec)))
            if x < rec["x"]:
                link(x, rec["row"], "r", ("h", id(rec)))
        if rec["kids"]:
            r0, r1 = rec["kids"][0]["row"], rec["kids"][-1]["row"]
            for y in range(r0, r1 + 1):
                if y > r0:
                    link(rec["x"], y, "u", ("v", id(rec)))
                if y < r1:
                    link(rec["x"], y, "d", ("v", id(rec)))
    for rec in tips:
        cells[(rec["x"], rec["row"])][0].discard("r")
    link(0, root["row"], "r", ("h", id(root)))

    tip_x = max(t["x"] for t in tips)
    label_x = tip_x + 3
    bracket_x = label_x + 4
    clusters = [(["a1", "a2", "a3"], "A"), (["b1", "b2", "b3"], "C"), (["c1"], "G"), (["d1"], "T")]
    by_name = {t["name"]: t for t in tips}

    def mrca(names):
        ids = {id(by_name[n]) for n in names}
        best = None
        for rec in nodes:
            if ids <= rec["clade"] and (best is None or len(rec["clade"]) < len(best["clade"])):
                best = rec
        return best

    width, height = bracket_x + 8, 2 * len(tips) - 1

    def draw(reveal, n_tips, n_clusters):
        cv = Canvas(width, height)
        coloured = {}
        for names, col in clusters[:n_clusters]:
            top = mrca(names)
            for rec in nodes:
                if id(rec) in top["clade"]:
                    coloured[("v", id(rec))] = col
                    if rec is not top or not top["kids"]:   # a one-tip species keeps its own branch
                        coloured[("h", id(rec))] = col
        for (x, y), (dirs, tag) in cells.items():
            if x <= reveal:
                cv.put(x, y, BOX[frozenset(dirs)], coloured.get(tag, "d" if n_clusters else "."))
        for i, t in enumerate(tips[:n_tips]):
            col = next((c for names, c in clusters[:n_clusters] if t["name"] in names), "w")
            cv.put(t["x"] + 1, t["row"], "●", col)
            cv.put(label_x, t["row"], t["name"], "d")
        for k, (names, col) in enumerate(clusters[:n_clusters]):
            r0, r1 = by_name[names[0]]["row"], by_name[names[-1]]["row"]
            if r0 == r1:
                cv.put(bracket_x, r0, "]", col)
            else:
                cv.put(bracket_x, r0, "┐", col)
                for y in range(r0 + 1, r1):
                    cv.put(bracket_x, y, "│", col)
                cv.put(bracket_x, r1, "┘", col)
            cv.put(bracket_x + 2, (r0 + r1) // 2, f"sp{k + 1}", col)
        return cv.frame()

    frames = []
    for t in range(tip_x + 2):
        frames.append(draw(t, 0, 0))
    for n in range(1, len(tips) + 1):
        frames += [draw(tip_x + 1, n, 0)] * 3
    frames += [draw(tip_x + 1, len(tips), 0)] * 12
    for k in range(1, len(clusters) + 1):
        frames += [draw(tip_x + 1, len(tips), k)] * 14
    frames += [draw(tip_x + 1, len(tips), len(clusters))] * 40
    for t in range(tip_x + 1, -2, -1):
        frames.append(draw(t, 0, 0) if t < 0 else _fade(draw(tip_x + 1, len(tips), len(clusters)), t))
    frames += [Canvas(width, height).frame()] * 10
    return {"fps": 20, "frames": frames}


def _fade(frame, cut):
    """Blank every cell right of column cut."""
    return [(c[:cut + 1] + " " * (len(c) - cut - 1), s[:cut + 1] + "." * (len(s) - cut - 1)) for c, s in frame]


# --- alignment ---------------------------------------------------------------------------------

def alignment_frames():
    """Unaligned sequences slide apart as gaps open, conserved columns are marked, a cursor sweeps."""
    rng = random.Random(11)
    length, nseq = 44, 6
    ref = [rng.choice(BASES) for _ in range(length)]
    rows = []
    for _ in range(nseq):
        s = [b if rng.random() > 0.12 else rng.choice(BASES) for b in ref]
        for _ in range(rng.randint(1, 3)):
            start, run = rng.randrange(4, length - 6), rng.randint(1, 3)
            for i in range(start, start + run):
                s[i] = "-"
        rows.append("".join(s))
    cons = ""
    for col in zip(*rows):
        res = [c for c in col if c != "-"]
        top = max(set(res), key=res.count) if res else "-"
        same = res.count(top)
        cons += "*" if same == nseq else ":" if same >= nseq - 2 else " "
    names = [f"s{i + 1:02d}" for i in range(nseq)]
    pad = 5
    width, height = pad + length, nseq + 3
    gaps_total = max(r.count("-") for r in rows)

    def state(row, k):
        out, seen = [], 0
        for ch in row:
            if ch == "-":
                seen += 1
                if seen <= k:
                    out.append("-")
            else:
                out.append(ch)
        return "".join(out).ljust(length)

    def draw(k, cons_n=0, cursor=None):
        cv = Canvas(width, height)
        ruler = "".join("|" if (i + 1) % 10 == 0 else "·" if (i + 1) % 5 == 0 else " " for i in range(length))
        cv.put(pad, 0, ruler, "d")
        for r, (name, row) in enumerate(zip(names, rows)):
            s = state(row, k)
            cv.put(0, r + 1, name, "d")
            cv.put(pad, r + 1, s, "".join(c if c in BASES else "d" for c in s))
        cv.put(pad, nseq + 2, cons[:cons_n], "w")
        if cursor is not None:
            for y in range(1, nseq + 1):
                st = cv.styles[y][pad + cursor]
                cv.restyle(pad + cursor, y, {"A": "a", "C": "c", "G": "g", "T": "t"}.get(st, "x"))
            cv.restyle(pad + cursor, nseq + 2, "w" if cons[cursor] != " " else ".")
            cv.put(pad + cursor, 0, "▼", "w")
        return cv.frame()

    frames = [draw(0)] * 20
    for k in range(1, gaps_total + 1):
        frames += [draw(k)] * 3
    frames += [draw(gaps_total)] * 8
    for n in range(1, length + 1):
        frames.append(draw(gaps_total, n))
    for c in range(length):
        frames += [draw(gaps_total, length, c)] * 2
    frames += [draw(gaps_total, length)] * 30
    for k in range(gaps_total, -1, -1):
        frames += [draw(k)] * 2
    return {"fps": 20, "frames": frames}


# --- sequence loom -----------------------------------------------------------------------------

def loom_frames():
    """A shuttle carries sequences back and forth through the warp; the woven cloth advances down."""
    width, height = 50, 16
    x0, x1 = 1, 48                          # cloth edges
    warps = list(range(3, x1, 7))
    reed, fell = 0, 4                       # beam row and the row being woven
    rng = random.Random(5)
    ref = [rng.choice(BASES) for _ in range(x1 - x0 + 1)]
    seqs = ["".join(b if rng.random() > 0.15 else rng.choice(BASES) for b in ref) for _ in range(6)]
    n = len(seqs)
    speed, settle, rest = 2, 2, 3

    def over(i, p):                         # warp i lies over the weft of pass p
        return (i + p) % 2 == 0

    def woven(cv, y, p, upto=None, rightward=True, style=None):
        seq = seqs[p % n]
        for x in range(x0, x1 + 1):
            if upto is not None and (x > upto if rightward else x < upto):
                continue
            if x in warps and over(warps.index(x), p):
                cv.put(x, y, "┃", "m")
            else:
                ch = seq[x - x0]
                cv.put(x, y, ch, style or ch)

    def draw(p, head=None, shift=0):
        cv = Canvas(width, height)
        cv.put(x0, reed, "═" * (x1 - x0 + 1), "d")
        for i, x in enumerate(warps):
            cv.put(x, reed, "╤", "d")
            for y in range(reed + 1, height):
                cv.put(x, y, "│", "m" if y > fell or over(i, p) else "d")
        rightward = p % 2 == 0
        if head is not None:
            woven(cv, fell, p, head, rightward)
            cv.put(head, fell, "▶" if rightward else "◀", "w")
        # finished rows; during settle the newest slides down from the fell
        done = range(p - 1, p - 1 - (height - fell) // 2 - 1, -1) if head is not None else range(p, p - (height - fell) // 2 - 1, -1)
        for j, q in enumerate(done):
            y = fell + 2 * (j + 1) if head is not None else fell + 2 * j + shift
            if fell < y < height or (head is None and y == fell):
                woven(cv, y, q, style=None if j < 3 else "d")
        return cv.frame()

    frames = []
    for p in range(n):
        rightward = p % 2 == 0
        xs = range(x0, x1 + 1, speed) if rightward else range(x1, x0 - 1, -speed)
        frames += [draw(p, x) for x in xs]
        frames += [draw(p)] * rest
        frames += [draw(p, shift=k) for k in range(1, settle + 1)]
    return {"fps": 20, "frames": frames}


# --- branching B -------------------------------------------------------------------------------

def branching_b_frames():
    """A B drawn as a tree grows from its root; tips light up and signals run out to them."""
    width, height = 32, 16
    sx = lambda x: 8 + (x - 137) * 0.19      # 512 px concept-icon coordinates to dots
    sy = lambda y: 2 + (y - 100) * 0.19
    pts = {"root": (152, 395), "inL": (152, 326), "mid_l": (152, 255), "inU": (152, 185),
           "top_l": (152, 115), "top_r": (262, 115), "N1": (352, 178), "mid": (262, 255),
           "N2": (372, 330), "bot_r": (272, 395), "inU_r": (228, 185), "n1": (264, 166),
           "inL_r": (240, 326), "n2": (280, 346)}
    pts = {k: (sx(x), sy(y)) for k, (x, y) in pts.items()}
    edges = [("root", "inL"), ("inL", "mid_l"), ("mid_l", "inU"), ("inU", "top_l"), ("top_l", "top_r"),
             ("top_r", "N1"), ("N1", "mid"), ("mid", "mid_l"), ("mid", "N2"), ("N2", "bot_r"),
             ("bot_r", "root"), ("inU", "inU_r"), ("inU_r", "n1"), ("inL", "inL_r"), ("inL_r", "n2")]
    nodes = [("N1", "●", "G"), ("N2", "●", "G"), ("n1", "•", "A"), ("n2", "•", "A")]
    seglen = {e: math.dist(pts[e[0]], pts[e[1]]) for e in edges}
    dist = {k: math.inf for k in pts}
    dist["root"] = 0.0
    todo = set(pts)
    while todo:                              # Dijkstra from the root
        u = min(todo, key=dist.get)
        todo.remove(u)
        for a, b in edges:
            for v, w in ((a, b), (b, a)):
                if v == u:
                    dist[w] = min(dist[w], dist[u] + seglen[(a, b)])
    samples = []                              # (x, y, distance from root) along every edge
    for a, b in edges:
        (xa, ya), (xb, yb), ln = pts[a], pts[b], seglen[(a, b)]
        steps = max(int(ln * 2), 1)
        for i in range(steps + 1):
            u = i / steps
            samples.append((xa + (xb - xa) * u, ya + (yb - ya) * u,
                            min(dist[a] + u * ln, dist[b] + (1 - u) * ln)))
    far = max(d for *_, d in samples)

    def draw(grown, pulses=(), popped=None):
        cv = Canvas(width, height)
        br = Braille(width, height)
        for x, y, d in samples:
            if d <= grown:
                hot = any(p - 7 < d <= p for p in pulses)
                for bx in (0, 1):
                    for by in (0, 1):
                        br.dot(x + bx, y + by, "w" if hot else ".")
        br.blit(cv, prefer=("w",))
        for name, ch, col in nodes:
            x, y = pts[name]
            if dist[name] <= grown:
                fresh = popped is not None and 0 <= grown - dist[name] < 6
                lit = any(0 <= p - dist[name] < 10 for p in pulses)
                cv.put(int((x + 0.5) // 2), int((y + 0.5) // 4), "◉" if fresh or lit else ch, "w" if fresh or lit else col)
        return cv.frame()

    frames = [draw(g, popped=True) for g in [i * 2.0 for i in range(int(far / 2) + 4)]]
    frames += [draw(far + 8)] * 10
    for p in [i * 3.0 for i in range(int(far / 3) + 14)]:
        frames.append(draw(far + 8, pulses=(p, p - far * 0.55)))
    frames += [draw(far + 8)] * 24
    frames += [draw(g) for g in [far - i * 3.0 for i in range(int(far / 3) + 2)]]
    frames += [Canvas(width, height).frame()] * 8
    return {"fps": 20, "frames": frames}


# --- shuttle -----------------------------------------------------------------------------------

def shuttle_frames():
    """A paper shuttle flies through drifting bases, shedding small blocks from its tail."""
    width, height, period = 60, 16, 240
    sx = lambda x: 30 + (x - 120) * 0.18     # 512 px concept-icon coordinates to dots
    sy = lambda y: 6 + (y - 107) * 0.18
    P = lambda *xy: [(sx(x), sy(y)) for x, y in zip(xy[::2], xy[1::2])]
    body = P(362, 107, 405, 151, 246, 356, 231, 288, 157, 267)
    belly = P(231, 288, 383, 130, 246, 356)
    fin = P(120, 242, 228, 207, 157, 267)
    flame = P(165, 292, 212, 338, 133, 370)
    hx, hy = 248 / math.hypot(248, -116), -116 / math.hypot(248, -116)   # heading
    nx, ny = -hy, hx                                                     # its normal
    rng = random.Random(3)
    stars = [(rng.uniform(0, width), rng.uniform(0, height), rng.choice([0.25, 0.5]), rng.choice(BASES))
             for _ in range(24)]
    every, life = 5, 30
    sparks = [(rng.uniform(-3, 3), rng.uniform(0.7, 1.1)) for _ in range(period // every)]
    tail = (sx(150), sy(338))

    frames = []
    for f in range(period):
        cv = Canvas(width, height)
        for x, y, v, b in stars:
            cx = (x - v * f) % width
            cy = (y + v * 0.25 * f) % height
            cv.put(int(cx), int(cy), b, "m" if v > 0.3 else "d")
        bob = 1.5 * math.sin(2 * math.pi * f / 60)
        ox, oy = nx * bob, ny * bob
        shift = lambda poly: [(x + ox, y + oy) for x, y in poly]
        for k in range(-(life // every) - 1, period // every):
            age = (f - k * every) % period
            if age >= life:
                continue
            jit, sp = sparks[k % len(sparks)]
            px = tail[0] + ox - hx * age * sp + nx * jit
            py = tail[1] + oy - hy * age * sp + ny * jit
            ch, st = ("■", "A") if age < 8 else ("▪", "m") if age < 18 else ("·", "d")
            cv.put(int(px // 2), int(py // 4), ch, st)
        br = Braille(width, height)
        flick = 0.75 + 0.25 * ((f // 2) % 2)
        base = ((flame[0][0] + flame[1][0]) / 2, (flame[0][1] + flame[1][1]) / 2)
        br.polygon(shift([flame[0], flame[1], (base[0] + (flame[2][0] - base[0]) * flick,
                                                base[1] + (flame[2][1] - base[1]) * flick)]), "G")
        br.polygon(shift(fin), "T")
        br.polygon(shift(body), "w")
        br.polygon(shift(belly), "C")
        br.blit(cv, prefer=("C", "T"))
        bx, by = sx(258) + ox, sy(210) + oy
        cv.put(int(bx // 2) - 1, int(by // 4), "◆◆", "d")
        frames.append(cv.frame())
    return {"fps": 20, "frames": frames}


ANIMS = {"trace": trace_frames, "cladogram": cladogram_frames, "alignment": alignment_frames,
         "loom": loom_frames, "branching-b": branching_b_frames, "shuttle": shuttle_frames}


def _ansi(style):
    def rgb(hexcol):
        return tuple(int(hexcol[i:i + 2], 16) for i in (1, 3, 5))
    if style in INVERSE:
        r, g, b = rgb(COLOURS[INVERSE[style]])
        k = rgb(BG)
        return f"\x1b[38;2;{k[0]};{k[1]};{k[2]};48;2;{r};{g};{b}m"
    r, g, b = rgb(COLOURS[style])
    return f"\x1b[0;38;2;{r};{g};{b}m"


def play(name):
    anim = ANIMS[name]()
    delay = 1 / anim["fps"]
    sys.stdout.write("\x1b[?25l\x1b[2J")
    try:
        while True:
            for frame in anim["frames"]:
                out = ["\x1b[H"]
                for chars, styles in frame:
                    for ch, st in zip(chars, styles):
                        out.append(_ansi(st) + ch)
                    out.append("\x1b[0m\n")
                sys.stdout.write("".join(out))
                sys.stdout.flush()
                time.sleep(delay)
    except KeyboardInterrupt:
        pass
    finally:
        sys.stdout.write("\x1b[0m\x1b[?25h\n")


def export():
    """Write the frames as a QML JavaScript library, storing each distinct frame once."""
    animations = []
    for name, fn in ANIMS.items():
        anim = fn()
        unique, seen, order = [], {}, []
        for frame in anim["frames"]:
            key = ("\n".join(c for c, _ in frame), "\n".join(s for _, s in frame))
            if key not in seen:
                seen[key] = len(unique)
                unique.append(list(key))
            order.append(seen[key])
        first = anim["frames"][0]
        animations.append({"name": name, "fps": anim["fps"], "columns": len(first[0][0]), "rows": len(first),
                           "frames": order, "unique": unique})
        print(f"{name}: {len(order)} frames ({len(unique)} distinct), {len(first[0][0])}x{len(first)}")
    dump = lambda value: json.dumps(value, ensure_ascii=False, separators=(",", ":"))
    out = Path(__file__).resolve().parents[2] / "qml" / "AsciiArt.js"
    out.write_text(".pragma library\n"
                   "// Generated by branding/ascii/generate.py export; edit that script instead.\n"
                   f"var palettes = {dump({'dark': {'background': BG, 'colours': COLOURS},
                                           'light': {'background': BG_LIGHT, 'colours': COLOURS_LIGHT}})}\n"
                   f"var inverse = {dump(INVERSE)}\n"
                   f"var animations = {dump(animations)}\n")


if __name__ == "__main__":
    if len(sys.argv) >= 2 and sys.argv[1] == "play":
        play(sys.argv[2])
    elif len(sys.argv) >= 2 and sys.argv[1] == "list":
        print(" ".join(ANIMS))
    elif len(sys.argv) >= 2 and sys.argv[1] == "export":
        export()
    else:
        print(__doc__)
