//! Phase 3 step 3a: interior floor plans (data + PNG preview, no 3D yet).
//!
//! "Rooms in a row": the usable area inside the wall faces is split into
//! rectangular wings that each run along an exterior wall, and every wing
//! is cut into slices spanning it across. A slice therefore always
//! touches an exterior (or courtyard) wall and shares a full-depth wall
//! with its neighbours; short wings stack up to three rooms inside one
//! slice. Interior walls that meet a wall with openings land on piers,
//! and doors form a spanning tree over the shared walls.

use mb_schema::{Footprint, HouseSpec, Luxury};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RoomKind {
    Living,
    Kitchen,
    Washroom,
    Bedroom,
    Pooja,
    Store,
    Stair,
}

#[derive(Clone, Debug)]
pub struct Room {
    pub id: u32,
    pub kind: RoomKind,
    /// [x0,z0,x1,z1] mm, axis-aligned.
    pub rect: [i64; 4],
}

#[derive(Clone, Debug)]
pub struct Door {
    /// Room ids; u32::MAX = outside/court.
    pub a: u32,
    pub b: u32,
    /// Door span endpoints on the shared wall line (900 mm apart).
    pub seg: [[i64; 2]; 2],
}

#[derive(Clone, Debug)]
pub struct Floor {
    pub storey: u32,
    pub rooms: Vec<Room>,
    pub doors: Vec<Door>,
    pub fixtures: Vec<(Luxury, u32)>,
    /// Facade openings on this storey as wall-band rects; true = door.
    pub openings: Vec<([i64; 4], bool)>,
}

#[derive(Clone, Debug)]
pub struct FloorPlan {
    pub floors: Vec<Floor>,
    /// Stair rect [x0,z0,x1,z1] if floors >= 2 (same on every storey).
    pub stair: Option<[i64; 4]>,
    /// Wing rects the rooms are laid out in (their union is the usable
    /// area). Wing corners are fixed by the footprint, not chosen cuts.
    pub wings: Vec<[i64; 4]>,
}

type R = [i64; 4];

const DOOR_W: i64 = 900;
const WIN_MARGIN: i64 = 100;
const FRONT_MARGIN: i64 = 300;
const MIN_STACK: i64 = 1200;
const M2: i64 = 1_000_000;

fn area(r: R) -> i64 {
    (r[2] - r[0]) * (r[3] - r[1])
}

fn overlap1(a0: i64, a1: i64, b0: i64, b1: i64) -> i64 {
    (a1.min(b1) - a0.max(b0)).max(0)
}

/// A facade opening on the room-side face of a wall.
#[derive(Clone, Copy, Debug)]
struct Op {
    /// Wall runs along x (line is a z coordinate).
    horiz: bool,
    line: i64,
    lo: i64,
    hi: i64,
    /// Other face of the wall (for drawing the wall band).
    band: i64,
    door: bool,
    court: bool,
}

impl Op {
    fn margin(&self) -> i64 {
        if self.door { FRONT_MARGIN } else { WIN_MARGIN }
    }
}

fn wall_ops(spec: &HouseSpec, seed: u64, storey: u32) -> Vec<Op> {
    let t = crate::mm(spec.wall_thickness_m);
    let (outer, court) = crate::plan_pub(spec);
    let co = court.as_ref().map(|c| crate::inward_offset_pub(c, -t));
    let mut out = Vec::new();
    for o in crate::openings(spec, seed) {
        if o.storey != storey {
            continue;
        }
        let (ring, face) = match (o.court, &co, &court) {
            (true, Some(co), Some(c)) => (co, c),
            (true, ..) => continue,
            (false, ..) => (&outer, &outer),
        };
        let n = ring.len();
        let (a, b) = (ring[o.piece], ring[(o.piece + 1) % n]);
        let (u0, u1) = (crate::mm(o.u0), crate::mm(o.u1));
        let op = if a[1] == b[1] {
            let s = (b[0] - a[0]).signum();
            let (x0, x1) = (a[0] + s * u0, a[0] + s * u1);
            Op {
                horiz: true,
                line: if o.court { a[1] } else { a[1] + s * t },
                lo: x0.min(x1),
                hi: x0.max(x1),
                band: face[o.piece][1],
                door: o.kind == "door",
                court: o.court,
            }
        } else {
            let s = (b[1] - a[1]).signum();
            let (z0, z1) = (a[1] + s * u0, a[1] + s * u1);
            Op {
                horiz: false,
                line: if o.court { a[0] } else { a[0] - s * t },
                lo: z0.min(z1),
                hi: z0.max(z1),
                band: face[o.piece][0],
                door: o.kind == "door",
                court: o.court,
            }
        };
        out.push(op);
    }
    out
}

/// Room-side wall faces: (runs along x, line, lo, hi).
fn wall_lines(spec: &HouseSpec) -> Vec<(bool, i64, i64, i64)> {
    let t = crate::mm(spec.wall_thickness_m);
    let (outer, court) = crate::plan_pub(spec);
    let mut rings = vec![crate::inward_offset_pub(&outer, t)];
    if let Some(c) = &court {
        rings.push(crate::inward_offset_pub(c, -t));
    }
    let mut out = Vec::new();
    for r in &rings {
        for i in 0..r.len() {
            let (p, q) = (r[i], r[(i + 1) % r.len()]);
            if p[1] == q[1] {
                out.push((true, p[1], p[0].min(q[0]), p[0].max(q[0])));
            } else {
                out.push((false, p[0], p[1].min(q[1]), p[1].max(q[1])));
            }
        }
    }
    out
}

/// An interior wall at `c` spanning [s0,s1]; vertical (x = c) when `vx`.
/// Bad when an end abuts a wall where it would sit inside an opening.
fn bad(ops: &[Op], vx: bool, c: i64, s0: i64, s1: i64) -> bool {
    ops.iter().any(|o| {
        o.horiz == vx
            && (o.line == s0 || o.line == s1)
            && c > o.lo - o.margin()
            && c < o.hi + o.margin()
    })
}

/// Nearest position to `ideal` in [lo,hi] accepted by `ok`, trying the
/// ideal, the range ends and every opening's pier boundaries.
fn snap(ideal: i64, lo: i64, hi: i64, ops: &[Op], ok: impl Fn(i64) -> bool) -> Option<i64> {
    if lo > hi {
        return None;
    }
    let mut c = vec![ideal.clamp(lo, hi), lo, hi];
    for o in ops {
        c.push(o.lo - o.margin());
        c.push(o.hi + o.margin());
    }
    c.into_iter()
        .filter(|&x| x >= lo && x <= hi && ok(x))
        .min_by_key(|&x| ((x - ideal).abs(), x))
}

/// Length of exterior/court wall face on the span end `s` of a slice
/// covering [a0,a1] along `ax`.
fn lit_len(lines: &[(bool, i64, i64, i64)], ax: bool, s: i64, a0: i64, a1: i64) -> i64 {
    lines
        .iter()
        .filter(|l| l.0 == ax && l.1 == s)
        .map(|l| overlap1(l.2, l.3, a0, a1))
        .sum()
}

#[derive(Clone, Copy, Debug)]
struct Wing {
    r: R,
    /// Along-axis is x: slices are cut by lines x = c and span z.
    ax: bool,
}

impl Wing {
    fn new(r: R) -> Self {
        Wing {
            r,
            ax: r[2] - r[0] >= r[3] - r[1],
        }
    }
    fn with_ax(r: R, ax: bool) -> Self {
        Wing { r, ax }
    }
    fn along(&self) -> (i64, i64) {
        if self.ax {
            (self.r[0], self.r[2])
        } else {
            (self.r[1], self.r[3])
        }
    }
    fn span(&self) -> (i64, i64) {
        if self.ax {
            (self.r[1], self.r[3])
        } else {
            (self.r[0], self.r[2])
        }
    }
    fn rect(&self, a0: i64, a1: i64, s0: i64, s1: i64) -> R {
        if self.ax {
            [a0, s0, a1, s1]
        } else {
            [s0, a0, s1, a1]
        }
    }
}

fn bbox(p: &[[i64; 2]]) -> R {
    [
        p.iter().map(|q| q[0]).min().unwrap(),
        p.iter().map(|q| q[1]).min().unwrap(),
        p.iter().map(|q| q[0]).max().unwrap(),
        p.iter().map(|q| q[1]).max().unwrap(),
    ]
}

/// Wing corners that sit inside an opening span (fixed by the footprint).
fn corner_clashes(wings: &[Wing], ops: &[Op]) -> usize {
    let mut n = 0;
    for w in wings {
        for (x, z) in [
            (w.r[0], w.r[1]),
            (w.r[2], w.r[1]),
            (w.r[0], w.r[3]),
            (w.r[2], w.r[3]),
        ] {
            // A corner on the front door would split it between wings.
            n += ops
                .iter()
                .filter(|o| {
                    let (l, c) = if o.horiz { (z, x) } else { (x, z) };
                    o.line == l && c > o.lo - o.margin() && c < o.hi + o.margin()
                })
                .map(|o| if o.door { 1000 } else { 1 })
                .sum::<usize>();
        }
    }
    n
}

fn make_wings(spec: &HouseSpec, ops: &[Op]) -> Vec<Wing> {
    let t = crate::mm(spec.wall_thickness_m);
    let (outer, court) = crate::plan_pub(spec);
    let inner = crate::inward_offset_pub(&outer, t);
    let [ix0, iz0, ix1, iz1] = bbox(&inner);
    let pick = |opts: Vec<Vec<Wing>>| -> Vec<Wing> {
        let mut best = 0;
        let mut bn = usize::MAX;
        for (i, o) in opts.iter().enumerate() {
            let n = corner_clashes(o, ops);
            if n < bn {
                bn = n;
                best = i;
            }
        }
        opts[best]
            .iter()
            .copied()
            .filter(|w| area(w.r) > 0)
            .collect()
    };
    match spec.footprint {
        Footprint::Rect { .. } => {
            let (w, d) = (ix1 - ix0, iz1 - iz0);
            if w.min(d) <= 8500 {
                return vec![Wing::new([ix0, iz0, ix1, iz1])];
            }
            // Two strips along the long side; the split wall is a chosen
            // cut, so it lands on piers of the two end walls.
            if w >= d {
                let c = snap(
                    (iz0 + iz1) / 2,
                    iz0 + d * 3 / 10,
                    iz1 - d * 3 / 10,
                    ops,
                    |c| !bad(ops, false, c, ix0, ix1),
                );
                match c {
                    Some(c) => vec![
                        Wing::with_ax([ix0, iz0, ix1, c], true),
                        Wing::with_ax([ix0, c, ix1, iz1], true),
                    ],
                    None => vec![Wing::new([ix0, iz0, ix1, iz1])],
                }
            } else {
                let c = snap(
                    (ix0 + ix1) / 2,
                    ix0 + w * 3 / 10,
                    ix1 - w * 3 / 10,
                    ops,
                    |c| !bad(ops, true, c, iz0, iz1),
                );
                match c {
                    Some(c) => vec![
                        Wing::with_ax([ix0, iz0, c, iz1], false),
                        Wing::with_ax([c, iz0, ix1, iz1], false),
                    ],
                    None => vec![Wing::new([ix0, iz0, ix1, iz1])],
                }
            }
        }
        Footprint::L { .. } => {
            // Inner vertex 3 is the reflex corner of the notch.
            let (nx, nz) = (inner[3][0], inner[3][1]);
            pick(vec![
                vec![
                    Wing::new([ix0, iz0, ix1, nz]),
                    Wing::new([ix0, nz, nx, iz1]),
                ],
                vec![
                    Wing::new([ix0, iz0, nx, iz1]),
                    Wing::new([nx, iz0, ix1, nz]),
                ],
            ])
        }
        Footprint::Courtyard { .. } => {
            let co = crate::inward_offset_pub(court.as_ref().unwrap(), -t);
            let [cx0, cz0, cx1, cz1] = bbox(&co);
            pick(vec![
                vec![
                    Wing::with_ax([ix0, iz0, ix1, cz0], true),
                    Wing::with_ax([ix0, cz1, ix1, iz1], true),
                    Wing::with_ax([ix0, cz0, cx0, cz1], false),
                    Wing::with_ax([cx1, cz0, ix1, cz1], false),
                ],
                vec![
                    Wing::with_ax([ix0, iz0, cx0, iz1], false),
                    Wing::with_ax([cx1, iz0, ix1, iz1], false),
                    Wing::with_ax([cx0, iz0, cx1, cz0], true),
                    Wing::with_ax([cx0, cz1, cx1, iz1], true),
                ],
            ])
        }
    }
}

fn min_w(k: RoomKind) -> i64 {
    match k {
        RoomKind::Living => 2000,
        RoomKind::Kitchen => 1800,
        RoomKind::Washroom => 1500,
        RoomKind::Bedroom => 2400,
        RoomKind::Pooja | RoomKind::Store => 1200,
        RoomKind::Stair => 2400,
    }
}

fn slice_min(s: &[(RoomKind, i64)]) -> i64 {
    if s.len() == 1 { min_w(s[0].0) } else { 1500 }
}

fn needs_light(k: RoomKind) -> bool {
    matches!(k, RoomKind::Living | RoomKind::Bedroom)
}

/// Where the front door sits relative to a wing.
#[derive(Clone, Copy)]
enum DoorPos {
    /// On a wall at a span end: every slice touches it; Living must
    /// cover [lo,hi] along the wing.
    Span { hi: bool, lo: i64, hi_c: i64 },
    /// On the wall closing the wing's along-end.
    Along { hi: bool },
}

fn locate_door(wings: &[Wing], d: &Op) -> Option<(usize, DoorPos)> {
    for (i, w) in wings.iter().enumerate() {
        let ((a0, a1), (s0, s1)) = (w.along(), w.span());
        if d.horiz == w.ax {
            if (d.line == s0 || d.line == s1) && a0 <= d.lo && d.hi <= a1 {
                return Some((
                    i,
                    DoorPos::Span {
                        hi: d.line == s1,
                        lo: d.lo,
                        hi_c: d.hi,
                    },
                ));
            }
        } else if (d.line == a0 || d.line == a1) && s0 <= d.lo && d.hi <= s1 {
            return Some((i, DoorPos::Along { hi: d.line == a1 }));
        }
    }
    None
}

struct Slot {
    wing: usize,
    /// Slot range along the wing; `hi_end`: the slot is at the wing's
    /// high along-end.
    a0: i64,
    a1: i64,
    hi_end: bool,
    stair: R,
    /// Room beside the stair in the same slot (stair takes the far span
    /// end); None when the stair fills the slot.
    front: Option<R>,
    /// The front room holds the front door.
    front_door: bool,
}

fn place_stair(
    wings: &[Wing],
    all_ops: &[Op],
    lines: &[(bool, i64, i64, i64)],
    door: Option<(&Op, usize, DoorPos)>,
) -> Option<Slot> {
    let mut order: Vec<usize> = (0..wings.len()).collect();
    order.sort_by_key(|&i| (-area(wings[i].r), i));
    // Pass 0 keeps the front door out of the stair slot, pass 1 lets the
    // slot's front room hold it, pass 2 ignores piers (never no stair).
    for pass in 0..3 {
        for &wi in &order {
            for flip in [false, true] {
                if let Some(s) = try_stair(wings, wi, all_ops, lines, door, pass, flip) {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn try_stair(
    wings: &[Wing],
    wi: usize,
    all_ops: &[Op],
    lines: &[(bool, i64, i64, i64)],
    door: Option<(&Op, usize, DoorPos)>,
    pass: u32,
    flip: bool,
) -> Option<Slot> {
    let w = wings[wi];
    let ((a0, a1), (s0, s1)) = (w.along(), w.span());
    let (len, span) = (a1 - a0, s1 - s0);
    if len < 2400 + 1800 && pass < 2 {
        return None;
    }
    let ops: &[Op] = if pass == 2 { &[] } else { all_ops };
    let in_wing = door.filter(|d| d.1 == wi);
    let dc = door.map(|(d, ..)| {
        let (x, z) = if d.horiz {
            ((d.lo + d.hi) / 2, d.line)
        } else {
            (d.line, (d.lo + d.hi) / 2)
        };
        if w.ax { x } else { z }
    });
    let hi_end = match in_wing {
        Some((_, _, DoorPos::Along { .. })) if flip => return None,
        Some((_, _, DoorPos::Along { hi })) => !hi,
        _ => dc.is_none_or(|c| a1 - c >= c - a0) != flip,
    };
    let lit0 = lit_len(lines, w.ax, s0, a0, a1) >= 1000;
    let lit1 = lit_len(lines, w.ax, s1, a0, a1) >= 1000;
    let stair_hi = match in_wing {
        Some((_, _, DoorPos::Span { hi, .. })) => !hi,
        _ => !(!lit0 && lit1),
    };
    let partial = span >= 4500;
    let door_span = match in_wing {
        Some((_, _, DoorPos::Span { lo, hi_c, .. })) => Some((lo, hi_c)),
        _ => None,
    };
    let wmax = (len - 1800).clamp(2400, 4500);
    let contains = |c: i64| match door_span {
        Some((lo, hi)) => {
            if hi_end {
                lo >= c
            } else {
                hi <= c
            }
        }
        None => false,
    };
    let ok = |c: i64| {
        !bad(ops, w.ax, c, s0, s1)
            && if pass == 0 {
                !contains(c)
            } else {
                partial || !contains(c)
            }
    };
    let c = if hi_end {
        snap(a1 - 2600, a1 - wmax, a1 - 2400, ops, ok)
    } else {
        snap(a0 + 2600, a0 + 2400, a0 + wmax, ops, ok)
    }?;
    let (sa0, sa1) = if hi_end { (c, a1) } else { (a0, c) };
    let mut front = None;
    let mut stair = w.rect(sa0, sa1, s0, s1);
    if partial {
        let depth = (span - 1500).min(4000);
        let okd = |c2: i64| !bad(ops, !w.ax, c2, sa0, sa1);
        let c2 = if stair_hi {
            snap(s1 - depth, s0 + 1500, s1 - 3000, ops, okd)
        } else {
            snap(s0 + depth, s0 + 3000, s1 - 1500, ops, okd)
        };
        match c2 {
            Some(c2) if stair_hi => {
                stair = w.rect(sa0, sa1, c2, s1);
                front = Some(w.rect(sa0, sa1, s0, c2));
            }
            Some(c2) => {
                stair = w.rect(sa0, sa1, s0, c2);
                front = Some(w.rect(sa0, sa1, c2, s1));
            }
            None if contains(c) => return None,
            None => {}
        }
    }
    Some(Slot {
        wing: wi,
        a0: sa0,
        a1: sa1,
        hi_end,
        stair,
        front,
        front_door: front.is_some() && contains(c),
    })
}

/// A free along-range of a wing to fill with rooms; `rev`: the near end
/// (next to Living or the stair) is the high end.
struct Part {
    wing: usize,
    a0: i64,
    a1: i64,
    rev: bool,
}

fn wing_order(wings: &[Wing], start: usize) -> Vec<usize> {
    let touch = |a: R, b: R| {
        ((a[2] == b[0] || b[2] == a[0]) && overlap1(a[1], a[3], b[1], b[3]) > 0)
            || ((a[3] == b[1] || b[3] == a[1]) && overlap1(a[0], a[2], b[0], b[2]) > 0)
    };
    let mut out = vec![start];
    let mut i = 0;
    while i < out.len() {
        let cur = out[i];
        for j in 0..wings.len() {
            if !out.contains(&j) && touch(wings[cur].r, wings[j].r) {
                out.push(j);
            }
        }
        i += 1;
    }
    for j in 0..wings.len() {
        if !out.contains(&j) {
            out.push(j);
        }
    }
    out
}

/// Shared wall segment between two rooms, at least `min` long.
fn shared(a: R, b: R, min: i64) -> Option<[[i64; 2]; 2]> {
    if a[2] == b[0] || b[2] == a[0] {
        let x = if a[2] == b[0] { a[2] } else { b[2] };
        let (z0, z1) = (a[1].max(b[1]), a[3].min(b[3]));
        if z1 - z0 >= min {
            return Some([[x, z0], [x, z1]]);
        }
    }
    if a[3] == b[1] || b[3] == a[1] {
        let z = if a[3] == b[1] { a[3] } else { b[3] };
        let (x0, x1) = (a[0].max(b[0]), a[2].min(b[2]));
        if x1 - x0 >= min {
            return Some([[x0, z], [x1, z]]);
        }
    }
    None
}

fn door_seg(seg: [[i64; 2]; 2]) -> [[i64; 2]; 2] {
    let c = [(seg[0][0] + seg[1][0]) / 2, (seg[0][1] + seg[1][1]) / 2];
    if seg[0][1] == seg[1][1] {
        [[c[0] - DOOR_W / 2, c[1]], [c[0] + DOOR_W / 2, c[1]]]
    } else {
        [[c[0], c[1] - DOOR_W / 2], [c[0], c[1] + DOOR_W / 2]]
    }
}

/// Deterministic Kruskal over room adjacency: through Living/Stair first,
/// never through a washroom when avoidable, longer walls first.
fn tree_doors(rooms: &[Room]) -> Vec<Door> {
    let mut edges = Vec::new();
    for (i, a) in rooms.iter().enumerate() {
        for b in &rooms[i + 1..] {
            if let Some(seg) = shared(a.rect, b.rect, 1200) {
                let ks = [a.kind, b.kind];
                let w = if ks.contains(&RoomKind::Washroom) {
                    2
                } else if ks.contains(&RoomKind::Living) || ks.contains(&RoomKind::Stair) {
                    0
                } else {
                    1
                };
                let len = (seg[1][0] - seg[0][0]) + (seg[1][1] - seg[0][1]);
                edges.push((w, -len, a.id, b.id, seg));
            }
        }
    }
    edges.sort();
    let mut uf: Vec<usize> = (0..rooms.len()).collect();
    fn find(uf: &mut [usize], mut x: usize) -> usize {
        while uf[x] != x {
            uf[x] = uf[uf[x]];
            x = uf[x];
        }
        x
    }
    let mut out = Vec::new();
    for (_, _, a, b, seg) in edges {
        let (ra, rb) = (find(&mut uf, a as usize), find(&mut uf, b as usize));
        if ra != rb {
            uf[ra] = rb;
            out.push(Door {
                a,
                b,
                seg: door_seg(seg),
            });
        }
    }
    out
}

struct Ctx<'a> {
    lines: &'a [(bool, i64, i64, i64)],
    ops: &'a [Op],
    rng: mb_seed::Rng,
    rooms: Vec<Room>,
}

impl Ctx<'_> {
    fn push(&mut self, kind: RoomKind, rect: R) -> u32 {
        let id = self.rooms.len() as u32;
        self.rooms.push(Room { id, kind, rect });
        id
    }

    /// Cut [a0,a1] of wing `w` into slices for `kinds` (in along order),
    /// stacking rooms when the slices would get too narrow.
    /// `pin`: the front door spans [lo,hi] on span end s1 (true) or s0;
    /// the slice around it becomes the Living one, Living at that end.
    fn fill(
        &mut self,
        w: Wing,
        a0: i64,
        a1: i64,
        kinds: &[(RoomKind, i64)],
        pin: Option<(i64, i64, bool)>,
    ) {
        let (s0, s1) = w.span();
        let span = s1 - s0;
        let len = a1 - a0;
        // Two daylit rooms may share a slice only if both span ends are
        // exterior along the whole part (wherever the cuts end up).
        let lit_any = lit_len(self.lines, w.ax, s0, a0, a1) >= len
            && lit_len(self.lines, w.ax, s1, a0, a1) >= len;
        let mut slices: Vec<Vec<(RoomKind, i64)>> = kinds.iter().map(|k| vec![*k]).collect();
        let can_merge = |s: &[Vec<(RoomKind, i64)>], i: usize| {
            let n = s[i].len() + s[i + 1].len();
            let lights = s[i]
                .iter()
                .chain(&s[i + 1])
                .filter(|k| needs_light(k.0))
                .count();
            n <= 3 && span >= MIN_STACK * n as i64 && lights <= if lit_any { 2 } else { 1 }
        };
        let merge = |s: &mut Vec<Vec<(RoomKind, i64)>>, i: usize| {
            let nxt = s.remove(i + 1);
            s[i].extend(nxt);
        };
        while slices.len() > 1 && slices.iter().map(|s| slice_min(s)).sum::<i64>() > len {
            let best = (0..slices.len() - 1)
                .filter(|&i| can_merge(&slices, i))
                .min_by_key(|&i| {
                    (
                        slices[i]
                            .iter()
                            .chain(&slices[i + 1])
                            .map(|k| k.1)
                            .sum::<i64>(),
                        i,
                    )
                });
            let Some(i) = best else { break };
            merge(&mut slices, i);
        }
        // Cut; a cut with no pier-valid position stacks its two slices
        // where that works, else stays put (see `cuts`).
        let cuts = loop {
            match self.cuts(w, a0, a1, &slices, false) {
                Ok(c) => break c,
                Err(i) if can_merge(&slices, i) => merge(&mut slices, i),
                Err(_) => break self.cuts(w, a0, a1, &slices, true).unwrap_or_default(),
            }
        };
        let bounds: Vec<(i64, i64)> = (0..slices.len())
            .map(|i| {
                (
                    if i == 0 { a0 } else { cuts[i - 1] },
                    cuts.get(i).copied().unwrap_or(a1),
                )
            })
            .collect();
        let mut at_door = None;
        if let Some((lo, hi, _)) = pin {
            let has_living = |s: &Vec<(RoomKind, i64)>| s.iter().any(|k| k.0 == RoomKind::Living);
            if let (Some(k), Some(j)) = (
                bounds.iter().position(|b| b.0 <= lo && hi <= b.1),
                slices.iter().position(has_living),
            ) {
                slices.swap(j, k);
                at_door = Some(k);
            }
        }
        for (i, s) in slices.iter().enumerate() {
            let p = if at_door == Some(i) {
                pin.map(|p| p.2)
            } else {
                None
            };
            self.stack(w, bounds[i].0, bounds[i].1, s, p);
        }
    }

    /// Pier-valid cut positions between `slices` over [a0,a1], or the
    /// index of the first cut that has none. With `clash`, a cut with no
    /// pier position stays at its ideal place: windows on two facades
    /// can leave no pier at all (the facade adapts to rooms in step 3b).
    fn cuts(
        &mut self,
        w: Wing,
        a0: i64,
        a1: i64,
        slices: &[Vec<(RoomKind, i64)>],
        clash: bool,
    ) -> Result<Vec<i64>, usize> {
        let (s0, s1) = w.span();
        let ops = self.ops;
        let tw: Vec<i64> = slices.iter().map(|s| s.iter().map(|k| k.1).sum()).collect();
        let mins: Vec<i64> = slices.iter().map(|s| slice_min(s)).collect();
        let mut out = Vec::new();
        let mut cur = a0;
        for i in 0..slices.len() - 1 {
            let rem_t: i64 = tw[i..].iter().sum();
            let jit = 1.0 + self.rng.range_f64(-0.1, 0.1);
            let ideal = cur + ((a1 - cur) as f64 * tw[i] as f64 / rem_t.max(1) as f64 * jit) as i64;
            let mut lo = cur + mins[i];
            let mut hi = a1 - mins[i + 1..].iter().sum::<i64>();
            if lo > hi {
                // Minimum widths can't all fit: share the room evenly.
                let n = (slices.len() - i) as i64;
                lo = cur + (a1 - cur) / (2 * n);
                hi = a1 - (a1 - cur) * (2 * n - 3).max(1) / (2 * n);
            }
            let c = match snap(ideal, lo, hi, ops, |c| !bad(ops, w.ax, c, s0, s1)) {
                Some(c) => c,
                None if clash => ideal.clamp(lo, hi),
                None => return Err(i),
            };
            out.push(c);
            cur = c;
        }
        Ok(out)
    }

    /// One slice [a0,a1] holding 1-3 rooms across the span; rooms that
    /// need daylight go to the lit span ends, and with `pin` Living goes
    /// to the door end (s1 when true).
    fn stack(&mut self, w: Wing, a0: i64, a1: i64, members: &[(RoomKind, i64)], pin: Option<bool>) {
        let (s0, s1) = w.span();
        if members.len() == 1 {
            self.push(members[0].0, w.rect(a0, a1, s0, s1));
            return;
        }
        let l0 = lit_len(self.lines, w.ax, s0, a0, a1) >= 1000;
        let l1 = lit_len(self.lines, w.ax, s1, a0, a1) >= 1000;
        let (lit, dark): (Vec<_>, Vec<_>) = members.iter().copied().partition(|k| needs_light(k.0));
        let mut seq: Vec<(RoomKind, i64)> = Vec::new();
        if let Some(at_s1) = pin {
            let liv: Vec<_> = members
                .iter()
                .copied()
                .filter(|k| k.0 == RoomKind::Living)
                .collect();
            let lit_o: Vec<_> = lit
                .iter()
                .copied()
                .filter(|k| k.0 != RoomKind::Living)
                .collect();
            if at_s1 {
                seq.extend(lit_o);
                seq.extend(dark.iter().copied());
                seq.extend(liv);
            } else {
                seq.extend(liv);
                seq.extend(dark.iter().copied());
                seq.extend(lit_o);
            }
        } else {
            match (l0, l1) {
                (true, true) => {
                    seq.extend(lit.first().copied());
                    seq.extend(dark.iter().copied());
                    seq.extend(lit.iter().skip(1).copied());
                }
                (false, true) => {
                    seq.extend(dark.iter().copied());
                    seq.extend(lit.iter().copied());
                }
                _ => {
                    seq.extend(lit.iter().copied());
                    seq.extend(dark.iter().copied());
                }
            }
        }
        let ops = self.ops;
        let tot: i64 = seq.iter().map(|k| k.1).sum();
        let mut cur = s0;
        for i in 0..seq.len() {
            let end = if i + 1 == seq.len() {
                s1
            } else {
                let rem: i64 = seq[i..].iter().map(|k| k.1).sum();
                let _ = tot;
                let ideal = cur + ((s1 - cur) as f64 * seq[i].1 as f64 / rem.max(1) as f64) as i64;
                let lo = cur + MIN_STACK;
                let hi = s1 - MIN_STACK * (seq.len() - i - 1) as i64;
                snap(ideal, lo, hi.max(lo), ops, |c| !bad(ops, !w.ax, c, a0, a1))
                    .unwrap_or(ideal.clamp(lo, hi.max(lo)))
            };
            self.push(seq[i].0, w.rect(a0, a1, cur, end));
            cur = end;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn layout_floor(
    spec: &HouseSpec,
    seed: u64,
    storey: u32,
    wings: &[Wing],
    lines: &[(bool, i64, i64, i64)],
    ops: &[Op],
    slot: Option<&Slot>,
    door: Option<(&Op, usize, DoorPos)>,
) -> Floor {
    let mut cx = Ctx {
        lines,
        ops,
        rng: mb_seed::Rng::new(mb_seed::derive(seed, "interior", &storey.to_string())),
        rooms: Vec::new(),
    };
    let ground = storey == 0;
    let usable: i64 = wings.iter().map(|w| area(w.r)).sum();
    let (mut living, mut wash) = (!ground, false);
    if let Some(s) = slot {
        cx.push(RoomKind::Stair, s.stair);
        if let Some(f) = s.front {
            if ground && s.front_door {
                cx.push(RoomKind::Living, f);
                living = true;
            } else {
                cx.push(RoomKind::Washroom, f);
                wash = true;
            }
        }
    }
    // Free parts of every wing, starting from the door (ground) or the
    // stair (upper floors).
    let start = match (ground, door, slot) {
        (true, Some(d), _) => d.1,
        (_, _, Some(s)) => s.wing,
        _ => 0,
    };
    let mut parts: Vec<Part> = Vec::new();
    for wi in wing_order(wings, start) {
        let (mut a0, mut a1) = wings[wi].along();
        let mut rev = false;
        if let Some(s) = slot.filter(|s| s.wing == wi) {
            if s.hi_end {
                a1 = s.a0;
                rev = true;
            } else {
                a0 = s.a1;
            }
        }
        if a1 > a0 {
            parts.push(Part {
                wing: wi,
                a0,
                a1,
                rev,
            });
        }
    }
    // The door wing's part comes first; its Living holds the front door.
    let door_part = if living {
        None
    } else {
        door.and_then(|(_, dw, pos)| parts.iter().position(|p| p.wing == dw).map(|i| (i, pos)))
    };
    let door_pos = door_part.map(|(i, pos)| {
        let p = parts.remove(i);
        parts.insert(0, p);
        pos
    });
    let caps: Vec<i64> = parts
        .iter()
        .map(|p| {
            let (s0, s1) = wings[p.wing].span();
            (p.a1 - p.a0) * (s1 - s0)
        })
        .collect();
    let free: i64 = caps.iter().sum();
    let mut queue: Vec<(RoomKind, i64)> = Vec::new();
    if ground {
        if !living {
            queue.push((RoomKind::Living, (usable * 30 / 100).max(9 * M2)));
        }
        queue.push((
            RoomKind::Kitchen,
            (usable * 12 / 100).clamp(5 * M2, 14 * M2),
        ));
        if !wash {
            queue.push((RoomKind::Washroom, 5 * M2));
        }
        if spec.luxuries.contains(&Luxury::PoojaCorner) {
            queue.push((RoomKind::Pooja, 3 * M2));
        }
        let used: i64 = queue.iter().map(|k| k.1).sum();
        let nb = ((free - used) / (14 * M2)).clamp(if spec.floors == 1 { 1 } else { 0 }, 4);
        queue.extend(std::iter::repeat_n(
            (RoomKind::Bedroom, 14 * M2),
            nb as usize,
        ));
    } else {
        if !wash {
            queue.push((RoomKind::Washroom, 5 * M2));
        }
        let used: i64 = queue.iter().map(|k| k.1).sum();
        let nb = ((free - used) / (14 * M2)).clamp(1, 8);
        queue.extend(std::iter::repeat_n(
            (RoomKind::Bedroom, 14 * M2),
            nb as usize,
        ));
    }
    // Spread the program over the parts in proportion to their area.
    let total_t: i64 = queue.iter().map(|k| k.1).sum::<i64>().max(1);
    let mut per: Vec<Vec<(RoomKind, i64)>> = vec![Vec::new(); parts.len()];
    let mut acc = 0i64;
    for &(k, t) in &queue {
        let mid = ((acc + t / 2) as i128 * free as i128 / total_t as i128) as i64;
        acc += t;
        let mut pi = parts.len().saturating_sub(1);
        let mut cum = 0;
        for (i, c) in caps.iter().enumerate() {
            cum += c;
            if mid < cum {
                pi = i;
                break;
            }
        }
        if k == RoomKind::Living && door_pos.is_some() {
            pi = 0;
        }
        if let Some(v) = per.get_mut(pi) {
            v.push((k, t));
        }
    }
    for (pi, p) in parts.iter().enumerate() {
        let w = wings[p.wing];
        let (s0, s1) = w.span();
        let cap = caps[pi];
        let mut take = std::mem::take(&mut per[pi]);
        if take.is_empty() {
            if cap >= 7 * M2 && p.a1 - p.a0 >= 2400 && s1 - s0 >= 2400 {
                let n = (cap / (14 * M2)).clamp(1, (p.a1 - p.a0) / 2400);
                take.extend(std::iter::repeat_n(
                    (RoomKind::Bedroom, cap / n),
                    n as usize,
                ));
            } else {
                take.push((RoomKind::Store, cap));
            }
        }
        let mut pin = None;
        if pi == 0
            && let Some(pos) = door_pos
        {
            let li = take
                .iter()
                .position(|k| k.0 == RoomKind::Living)
                .unwrap_or(0);
            let liv = take.remove(li);
            match pos {
                DoorPos::Along { hi } => {
                    if hi {
                        take.reverse();
                        take.push(liv);
                    } else {
                        take.insert(0, liv);
                    }
                }
                DoorPos::Span { hi, lo, hi_c } => {
                    // Living where the door is; the rest on whichever side
                    // has room, nearest first (so the kitchen adjoins it).
                    let tot: i64 = take.iter().map(|k| k.1).sum::<i64>() + liv.1;
                    let f = (((lo + hi_c) / 2 - p.a0) as f64 / (p.a1 - p.a0).max(1) as f64)
                        .clamp(0.0, 1.0);
                    let mut cap_lo = f * tot as f64 - liv.1 as f64 / 2.0;
                    let mut cap_hi = tot as f64 - cap_lo - liv.1 as f64;
                    let (mut before, mut after) = (Vec::new(), Vec::new());
                    for k in take.drain(..) {
                        if cap_lo >= cap_hi {
                            cap_lo -= k.1 as f64;
                            before.push(k);
                        } else {
                            cap_hi -= k.1 as f64;
                            after.push(k);
                        }
                    }
                    before.reverse();
                    take = before;
                    take.push(liv);
                    take.extend(after);
                    pin = Some((lo, hi_c, hi));
                }
            }
        } else if p.rev {
            take.reverse();
        }
        cx.fill(w, p.a0, p.a1, &take, pin);
    }
    let rooms = std::mem::take(&mut cx.rooms);
    let mut doors = tree_doors(&rooms);
    // Front door and the courtyard door.
    if ground && let Some((d, ..)) = door {
        let c = (d.lo + d.hi) / 2;
        let seg = if d.horiz {
            [[c - DOOR_W / 2, d.line], [c + DOOR_W / 2, d.line]]
        } else {
            [[d.line, c - DOOR_W / 2], [d.line, c + DOOR_W / 2]]
        };
        if let Some(r) = rooms.iter().find(|r| on_edge(r.rect, seg)) {
            doors.push(Door {
                a: r.id,
                b: u32::MAX,
                seg,
            });
        }
    }
    if ground && spec.footprint.court().is_some() {
        let court_lines: Vec<_> = {
            let t = crate::mm(spec.wall_thickness_m);
            let co = crate::inward_offset_pub(crate::plan_pub(spec).1.as_ref().unwrap(), -t);
            let r = bbox(&co);
            vec![
                (true, r[1], r[0], r[2]),
                (true, r[3], r[0], r[2]),
                (false, r[0], r[1], r[3]),
                (false, r[2], r[1], r[3]),
            ]
        };
        let cd = ops.iter().find(|o| o.court && o.door);
        let mut best: Option<(i64, u32, [[i64; 2]; 2])> = None;
        for r in &rooms {
            for &(h, l, lo, hi) in &court_lines {
                let (e0, e1, on) = if h {
                    (
                        r.rect[0].max(lo),
                        r.rect[2].min(hi),
                        r.rect[1] == l || r.rect[3] == l,
                    )
                } else {
                    (
                        r.rect[1].max(lo),
                        r.rect[3].min(hi),
                        r.rect[0] == l || r.rect[2] == l,
                    )
                };
                if !on || e1 - e0 < 1200 {
                    continue;
                }
                let c = match cd {
                    Some(o) if o.horiz == h && o.line == l => {
                        let c = (o.lo + o.hi) / 2;
                        if c - DOOR_W / 2 < e0 || c + DOOR_W / 2 > e1 {
                            continue;
                        }
                        c
                    }
                    Some(_) => continue,
                    None => (e0 + e1) / 2,
                };
                let score = (r.kind == RoomKind::Living) as i64 * 1_000_000 + (e1 - e0);
                let seg = if h {
                    [[c - DOOR_W / 2, l], [c + DOOR_W / 2, l]]
                } else {
                    [[l, c - DOOR_W / 2], [l, c + DOOR_W / 2]]
                };
                if best.is_none_or(|b| score > b.0) {
                    best = Some((score, r.id, seg));
                }
            }
        }
        if let Some((_, id, seg)) = best {
            doors.push(Door {
                a: id,
                b: u32::MAX,
                seg,
            });
        }
    }
    // Fixtures.
    let mut fixtures = Vec::new();
    let find = |k: RoomKind| rooms.iter().find(|r| r.kind == k).map(|r| r.id);
    for l in &spec.luxuries {
        match l {
            Luxury::Tv | Luxury::Jhoola if ground => {
                fixtures.extend(find(RoomKind::Living).map(|id| (*l, id)))
            }
            Luxury::Ac => {
                let beds: Vec<u32> = rooms
                    .iter()
                    .filter(|r| r.kind == RoomKind::Bedroom)
                    .map(|r| r.id)
                    .collect();
                if beds.is_empty() {
                    fixtures.extend(find(RoomKind::Living).map(|id| (*l, id)));
                }
                fixtures.extend(beds.into_iter().map(|id| (*l, id)));
            }
            Luxury::WashingMachine if ground => fixtures.extend(
                find(RoomKind::Washroom)
                    .or(find(RoomKind::Store))
                    .map(|id| (*l, id)),
            ),
            _ => {}
        }
    }
    let openings = ops
        .iter()
        .map(|o| {
            let (b0, b1) = (o.line.min(o.band), o.line.max(o.band));
            let r = if o.horiz {
                [o.lo, b0, o.hi, b1]
            } else {
                [b0, o.lo, b1, o.hi]
            };
            (r, o.door)
        })
        .collect();
    Floor {
        storey,
        rooms,
        doors,
        fixtures,
        openings,
    }
}

/// `seg` lies on an edge of `r`.
fn on_edge(r: R, seg: [[i64; 2]; 2]) -> bool {
    let [[x0, z0], [x1, z1]] = seg;
    if z0 == z1 {
        (r[1] == z0 || r[3] == z0) && r[0] <= x0.min(x1) && x0.max(x1) <= r[2]
    } else {
        (r[0] == x0 || r[2] == x0) && r[1] <= z0.min(z1) && z0.max(z1) <= r[3]
    }
}

/// Full multi-storey layout.
pub fn layout(spec: &HouseSpec, seed: u64) -> FloorPlan {
    let n = spec.floors.max(1);
    let ops: Vec<Vec<Op>> = (0..n).map(|s| wall_ops(spec, seed, s)).collect();
    let all: Vec<Op> = ops.concat();
    let lines = wall_lines(spec);
    let wings = make_wings(spec, &all);
    let door_op = ops[0].iter().find(|o| o.door && !o.court).copied();
    let door = door_op
        .as_ref()
        .and_then(|d| locate_door(&wings, d).map(|(i, p)| (d, i, p)));
    let slot = if n >= 2 {
        place_stair(&wings, &all, &lines, door)
    } else {
        None
    };
    let floors = (0..n)
        .map(|s| {
            layout_floor(
                spec,
                seed,
                s,
                &wings,
                &lines,
                &ops[s as usize],
                slot.as_ref(),
                door,
            )
        })
        .collect();
    FloorPlan {
        floors,
        stair: slot.map(|s| s.stair),
        wings: wings.iter().map(|w| w.r).collect(),
    }
}

// ---- PNG renderer ----

const COLS: [[u8; 3]; 7] = [
    [245, 220, 140], // Living warm yellow
    [235, 160, 80],  // Kitchen orange
    [160, 200, 230], // Washroom light blue
    [170, 215, 160], // Bedroom light green
    [240, 165, 60],  // Pooja saffron
    [180, 180, 180], // Store grey
    [200, 200, 200], // Stair (hatched)
];

/// Top-down floor-plan PNG at `px_per_m` resolution: dark walls, rooms
/// filled by kind, the stair hatched, door gaps white, windows blue.
pub fn floor_plan_png(spec: &HouseSpec, plan: &FloorPlan, storey: u32, px_per_m: u32) -> Vec<u8> {
    let (outer, court) = crate::plan_pub(spec);
    let [ox0, oz0, ox1, oz1] = bbox(&outer);
    let pad = 1000i64;
    let ppm = px_per_m as i64;
    let w = ((ox1 - ox0 + 2 * pad) * ppm / 1000).max(1) as usize;
    let h = ((oz1 - oz0 + 2 * pad) * ppm / 1000).max(1) as usize;
    let mut img = vec![250u8; w * h * 4];
    for i in (3..img.len()).step_by(4) {
        img[i] = 255;
    }
    // North (+z) up: image rows grow towards -z.
    let to_px = |x: i64, z: i64| -> (i64, i64) {
        ((x - ox0 + pad) * ppm / 1000, (oz1 + pad - z) * ppm / 1000)
    };
    let fill = |img: &mut [u8], r: R, c: [u8; 3]| {
        let (x0, y1) = to_px(r[0], r[1]);
        let (x1, y0) = to_px(r[2], r[3]);
        for y in y0.max(0)..y1.min(h as i64) {
            for x in x0.max(0)..x1.min(w as i64) {
                let i = (y as usize * w + x as usize) * 4;
                img[i..i + 3].copy_from_slice(&c);
            }
        }
    };
    // Wall mass: the whole footprint dark, the L notch and court cleared.
    fill(&mut img, [ox0, oz0, ox1, oz1], [40, 40, 40]);
    if let Footprint::L { .. } = spec.footprint {
        fill(
            &mut img,
            [outer[3][0], outer[3][1], ox1, oz1],
            [250, 250, 250],
        );
    }
    if let Some(c) = &court {
        fill(&mut img, bbox(c), [235, 240, 225]);
    }
    let Some(f) = plan.floors.iter().find(|f| f.storey == storey) else {
        return encode(&img, w, h);
    };
    let line = 2 * ppm / 30 + 1;
    for r in &f.rooms {
        // Inset by half a line so neighbouring rooms show a wall between.
        let i = line * 1000 / ppm / 2;
        fill(
            &mut img,
            [r.rect[0] + i, r.rect[1] + i, r.rect[2] - i, r.rect[3] - i],
            COLS[r.kind as usize],
        );
        if r.kind == RoomKind::Stair {
            let (x0, y1) = to_px(r.rect[0], r.rect[1]);
            let (x1, y0) = to_px(r.rect[2], r.rect[3]);
            for y in y0.max(0)..y1.min(h as i64) {
                for x in x0.max(0)..x1.min(w as i64) {
                    if (x + y) % 10 < 2 {
                        let i = (y as usize * w + x as usize) * 4;
                        img[i..i + 3].copy_from_slice(&[120, 120, 120]);
                    }
                }
            }
        }
    }
    for (r, door) in &f.openings {
        fill(
            &mut img,
            *r,
            if *door {
                [255, 255, 255]
            } else {
                [120, 175, 225]
            },
        );
    }
    for d in &f.doors {
        let [[x0, z0], [x1, z1]] = d.seg;
        let g = line * 1000 / ppm + 60;
        let r = if z0 == z1 {
            [x0.min(x1), z0 - g, x0.max(x1), z0 + g]
        } else {
            [x0 - g, z0.min(z1), x0 + g, z0.max(z1)]
        };
        fill(&mut img, r, [255, 255, 255]);
    }
    encode(&img, w, h)
}

fn encode(img: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().expect("png header");
    wr.write_image_data(img).expect("png data");
    drop(wr);
    out
}
