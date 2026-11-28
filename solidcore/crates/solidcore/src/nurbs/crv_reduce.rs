//! A clamped B-spline curve with knots taken out until it has at most a given number of control
//! points, within a budget of the kernel's certified bounds.
//!
//! Upstream: none (not upstream; solidcore-native).
//!
//! | Rust                 | Upstream |
//! |----------------------|----------|
//! | [`reduce_to_points`] | none     |
//! | [`Reduction`]        | none     |
//!
//! Knots are taken out greedily, the one whose removal moves the curve least first, as that is
//! judged by the removal's own bound, [`Curve::remove_knot_max_err_of`] (`N_CrvRemoveKnotMaxErr`):
//! the removal changes one control point of the curve with the knot put back, by that distance, and
//! the basis functions are non-negative and sum to one, so the curve moves nowhere by more. The move
//! is that distance times the peak of the basis function, which is less than one, so the bound
//! overstates it, and a gate on it would refuse a curve the budget allows. It orders the removals;
//! the budget is held by the one comparison that follows, of the result with the original by
//! [`certified_distance`]. That is the bound returned. When it is past the budget the removals are
//! taken back by bisection to the most that pass.
//!
//! Cost: the knots and points are a linked list, a removal is made on the dozen or so points
//! around the knot (so it is constant time), and it changes the bound only of the removals within
//! `3 degree + 4` places of it, which are found again and put back in a heap of the bounds. `n`
//! removals at most, each `O(log n)` for the heap: `O(n log n)`. The one certified comparison is
//! `O(n)`; the bisection (only when it is needed) replays the removals made, `O(n)` for each of its
//! `O(log n)` steps.

use crate::error::{Result, SolidError};
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::crv_deviation::certified_distance;
use crate::nurbs::knots::Side;
use crate::nurbs::math::CPoint;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

const OP: &str = "nurbs::crv_reduce::reduce_to_points";

/// The tangent angle (the sine of it, for the small angles that matter) past which the two sides
/// of a joint held `degree` times meet at a corner, not a smooth join a knot may be taken out of.
const CORNER: f64 = 1e-3;

/// What [`reduce_to_points`] came to.
#[derive(Clone, Debug)]
pub struct Reduction {
    /// The curve after the removals made.
    pub curve: Curve,
    /// How far `curve` is from the original, everywhere: the kernel's certified bound
    /// ([`certified_distance`]), never past the budget.
    pub bound: f64,
    /// Whether `curve` has at most the number of control points asked for.
    pub reached: bool,
}

const NONE: usize = usize::MAX;

/// The knots and control points of a clamped curve as a list of units, the `i`-th knot with the
/// `i`-th control point (the last `degree + 1` knots, equal to `last`, are not held).
struct Chain {
    degree: usize,
    first: f64,
    last: f64,
    knot: Vec<f64>,
    point: Vec<CPoint>,
    prev: Vec<usize>,
    next: Vec<usize>,
    alive: Vec<bool>,
    version: Vec<u32>,
}

/// The units around one, as a curve's own arrays: `r` is where the unit is in them.
struct Window {
    points: Vec<CPoint>,
    knots: Vec<f64>,
    r: usize,
}

/// A heap key: the bound, least first.
#[derive(PartialEq)]
struct Key(f64);

impl Eq for Key {}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Key {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

type Heap = BinaryHeap<(Reverse<Key>, Reverse<usize>, u32)>;

impl Chain {
    fn new(c: &Curve) -> Option<Self> {
        let (points, knots, degree) = (c.cpts(), c.knots(), c.degree());
        let units = points.len();
        let (first, last) = c.param_bounds();
        let tail = knots.get(units..)?;
        if degree == 0
            || knots.len() != units + degree + 1
            || tail.iter().any(|k| *k != last)
            || knots.get(..=degree)?.iter().any(|k| *k != first)
            || !knots.iter().all(|k| k.is_finite())
        {
            return None;
        }
        Some(Self {
            degree,
            first,
            last,
            knot: knots.get(..units)?.to_vec(),
            point: points.to_vec(),
            prev: (0..units)
                .map(|i| i.checked_sub(1).unwrap_or(NONE))
                .collect(),
            next: (0..units)
                .map(|i| if i + 1 < units { i + 1 } else { NONE })
                .collect(),
            alive: vec![true; units],
            version: vec![0; units],
        })
    }

    /// The units around `x`, `2 degree + 4` before it and as many after it.
    fn window(&self, x: usize) -> Option<Window> {
        let p = self.degree;
        let pad = 2 * p + 4;
        let (mut start, mut r) = (x, 0);
        while r < pad && *self.prev.get(start)? != NONE {
            start = *self.prev.get(start)?;
            r += 1;
        }
        let want = r + pad + 1 + p + 1;
        let mut units = Vec::with_capacity(want);
        let mut at = start;
        while at != NONE && units.len() < want {
            units.push(at);
            at = *self.next.get(at)?;
        }
        let (points, mut knots): (Vec<CPoint>, Vec<f64>) = if units.len() >= want {
            let keep = r + pad + 1;
            let pts = units.get(..keep)?;
            let ks = units.get(..keep + p + 1)?;
            (
                pts.iter()
                    .filter_map(|u| self.point.get(*u).copied())
                    .collect(),
                ks.iter()
                    .filter_map(|u| self.knot.get(*u).copied())
                    .collect(),
            )
        } else {
            (
                units
                    .iter()
                    .filter_map(|u| self.point.get(*u).copied())
                    .collect(),
                units
                    .iter()
                    .filter_map(|u| self.knot.get(*u).copied())
                    .collect(),
            )
        };
        knots.resize(points.len() + p + 1, self.last);
        Some(Window { points, knots, r })
    }

    /// The own bound of removing the knot of `x`, one copy; `None` when it is not one to take out:
    /// not the last copy of its value, not inside the domain, a corner, or a bound that is not a
    /// number.
    fn bound(&self, x: usize) -> Option<(f64, usize)> {
        let p = self.degree;
        let w = self.window(x)?;
        let (&t, &after) = (w.knots.get(w.r)?, w.knots.get(w.r + 1)?);
        if !(t > self.first && t < self.last && t < after) {
            return None;
        }
        let mut s = 1;
        while s <= p && w.r >= s && w.knots.get(w.r - s) == Some(&t) {
            s += 1;
        }
        if s > p || w.r < p + 1 || (s == p && is_corner(&w.points, w.r, p)) {
            return None;
        }
        let b = Curve::remove_knot_max_err_of(&w.points, &w.knots, p, w.r, s).ok()?;
        (!b.is_nan()).then_some((b, s))
    }

    /// Takes one copy of the knot of `x` out, on the units around it; the unit before it when that
    /// is done, `None` when the kernel removes nothing, or it would move the curve at a parameter of
    /// `held` by more than rounding (and a billionth of the budget).
    fn remove(&mut self, x: usize, s: usize, held: &[f64], budget: f64) -> Option<usize> {
        let p = self.degree;
        let w = self.window(x)?;
        let t = *w.knots.get(w.r)?;
        let win = Curve::new(w.points.clone(), p, w.knots.clone()).ok()?;
        let done = win.remove_knot(t, 1, f64::MAX).ok()?;
        if done.removed != 1 {
            return None;
        }
        let (a, b) = (w.r.checked_sub(p)?, w.r.checked_sub(s)?);
        let piece = done.curve.cpts().get(a..b)?.to_vec();
        if piece.len() + 1 != b + 1 - a {
            return None;
        }
        // the curves differ only between the first and last knot of the points moved
        let (lo, hi) = (*w.knots.get(a)?, *w.knots.get(b + p + 1)?);
        for &h in held.iter().filter(|h| **h >= lo && **h <= hi) {
            let (Ok(u), Ok(v)) = (win.eval(h, Side::Left), done.curve.eval(h, Side::Left)) else {
                return None;
            };
            let size = [u.x, u.y, u.z, v.x, v.y, v.z]
                .iter()
                .fold(0.0_f64, |m, c| m.max(c.abs()));
            let moved = norm([u.x - v.x, u.y - v.y, u.z - v.z]);
            if moved.is_nan() || moved > 1e-9 * budget + 64.0 * f64::EPSILON * size {
                return None;
            }
        }
        // the units from `r - degree` to the one before the knot's take the points that are left
        let mut units = vec![x];
        for _ in 0..p {
            units.push(*self.prev.get(*units.last()?)?);
        }
        units.reverse();
        for (i, unit) in units.iter().enumerate().take(units.len() - 1) {
            let new = if i < piece.len() {
                *piece.get(i)?
            } else {
                *self.point.get(*units.get(i + 1)?)?
            };
            *self.point.get_mut(*unit)? = new;
        }
        let (before, after) = (*self.prev.get(x)?, *self.next.get(x)?);
        *self.alive.get_mut(x)? = false;
        *self.next.get_mut(before)? = after;
        if let Some(p) = self.prev.get_mut(after) {
            *p = before;
        }
        Some(before)
    }

    /// The bounds of the removals near `from` (the unit before one taken out), found again.
    fn refresh(&mut self, from: usize, heap: &mut Heap) {
        let p = self.degree;
        let mut at = from;
        for _ in 0..2 * p + 2 {
            match self.prev.get(at) {
                Some(&b) if b != NONE => at = b,
                _ => break,
            }
        }
        for _ in 0..3 * p + 5 {
            self.push(at, heap);
            match self.next.get(at) {
                Some(&n) if n != NONE => at = n,
                _ => break,
            }
        }
    }

    fn push(&mut self, x: usize, heap: &mut Heap) {
        if let Some(v) = self.version.get_mut(x) {
            *v += 1;
            let v = *v;
            if let Some((b, _)) = self.bound(x) {
                heap.push((Reverse(Key(b)), Reverse(x), v));
            }
        }
    }

    fn curve(&self) -> Option<Curve> {
        let (mut points, mut knots) = (Vec::new(), Vec::new());
        let mut at = 0;
        while at != NONE {
            points.push(*self.point.get(at)?);
            knots.push(*self.knot.get(at)?);
            at = *self.next.get(at)?;
        }
        knots.extend(std::iter::repeat_n(self.last, self.degree + 1));
        Curve::new(points, self.degree, knots).ok()
    }
}

/// `curve` with knots taken out, the cheapest first, until it has at most `cap` control points or
/// no removal is left, as many as keep it within `budget`; the result is within `budget` of `curve`
/// everywhere.
///
/// A knot is left in where the curve has a corner (held as many times as the degree, whose two
/// sides meet at more than [`CORNER`]; held more than that), and where taking it out would move
/// the curve at a parameter of `keep` or at an end of its domain by more than rounding.
///
/// # Errors
///
/// `InvalidInput` for a `budget` that is not finite and positive, or a curve that is rational or
/// not clamped.
pub fn reduce_to_points(orig: Curve, cap: usize, budget: f64, keep: &[f64]) -> Result<Reduction> {
    if !budget.is_finite() || budget <= 0.0 {
        return Err(SolidError::invalid_input(
            OP,
            "a budget that is not finite and positive",
        ));
    }
    if orig.is_rational() {
        return Err(SolidError::invalid_input(OP, "a rational curve"));
    }
    let Some(mut chain) = Chain::new(&orig) else {
        return Err(SolidError::invalid_input(OP, "a curve that is not clamped"));
    };
    let held: Vec<f64> = keep
        .iter()
        .copied()
        .chain([chain.first, chain.last])
        .filter(|t| *t >= chain.first && *t <= chain.last)
        .collect();
    let need = orig.cpts().len().saturating_sub(cap);
    let mut heap = Heap::new();
    for x in 0..orig.cpts().len() {
        chain.push(x, &mut heap);
    }
    let mut log: Vec<usize> = Vec::new();
    while log.len() < need {
        let Some((Reverse(_), Reverse(x), version)) = heap.pop() else {
            break;
        };
        if chain.alive.get(x) != Some(&true) || chain.version.get(x) != Some(&version) {
            continue;
        }
        let Some((_, s)) = chain.bound(x) else {
            continue;
        };
        if let Some(before) = chain.remove(x, s, &held, budget) {
            log.push(x);
            chain.refresh(before, &mut heap);
        }
    }
    let reached = |c: &Curve| c.cpts().len() <= cap;
    let Some(cur) = chain.curve().filter(|_| !log.is_empty()) else {
        return Ok(Reduction {
            reached: reached(&orig),
            curve: orig,
            bound: 0.0,
        });
    };
    let within = |c: &Curve| certified_distance(&orig, c).ok().filter(|d| *d <= budget);
    if let Some(bound) = within(&cur) {
        return Ok(Reduction {
            reached: reached(&cur),
            curve: cur,
            bound,
        });
    }
    // Bisect: `lo` removals pass (none do), `hi` do not.
    let (mut lo, mut hi) = (0usize, log.len());
    let mut best = (orig.clone(), 0.0);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        match replay(&orig, &log, mid, &held, budget) {
            Some(c) => match within(&c) {
                Some(d) => {
                    lo = mid;
                    best = (c, d);
                }
                None => hi = mid,
            },
            None => hi = mid,
        }
    }
    Ok(Reduction {
        reached: reached(&best.0),
        curve: best.0,
        bound: best.1,
    })
}

/// `orig` with the first `count` removals of `log` (the units whose knots were taken out) made.
fn replay(orig: &Curve, log: &[usize], count: usize, held: &[f64], budget: f64) -> Option<Curve> {
    let mut chain = Chain::new(orig)?;
    for &x in log.get(..count)? {
        let (_, s) = chain.bound(x)?;
        chain.remove(x, s, held, budget)?;
    }
    chain.curve()
}

/// Whether the knot at `r` of the points `pw`, held `degree` times, is at a corner: the two sides
/// of the joint are not smooth ([`CORNER`]) or one has no length.
fn is_corner(pw: &[CPoint], r: usize, degree: usize) -> bool {
    let at = |i: usize| pw.get(i).map(|p| [p.x, p.y, p.z]);
    let sides = r.checked_sub(degree).and_then(|j| {
        let (a, b, c) = (at(j.checked_sub(1)?)?, at(j)?, at(j + 1)?);
        Some((sub(b, a), sub(c, b)))
    });
    let Some((l, r)) = sides else { return true };
    let (nl, nr) = (norm(l), norm(r));
    if nl.is_nan() || nr.is_nan() || nl <= 0.0 || nr <= 0.0 {
        return true;
    }
    let dot = l[0] * r[0] + l[1] * r[1] + l[2] * r[2];
    let cross = [
        l[1] * r[2] - l[2] * r[1],
        l[2] * r[0] - l[0] * r[2],
        l[0] * r[1] - l[1] * r[0],
    ];
    dot <= 0.0 || norm(cross) / (nl * nr) > CORNER
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
