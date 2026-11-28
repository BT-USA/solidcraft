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
//! Each removal is one knot taken out ([`Curve::remove_knot`], The NURBS Book A5.8), kept when
//! [`certified_distance`] of the result from the curve it came from is within what that round
//! allows. The bound of the result from the original curve is the sum of those bounds, by the
//! triangle inequality: each is the distance from the curve before it, at the same parameters.

use crate::nurbs::crv_basic::Curve;
use crate::nurbs::crv_deviation::certified_distance;
use crate::nurbs::knots::Side;

/// The tangent angle (the sine of it, for the small angles that matter) past which the two sides
/// of a joint held `degree` times meet at a corner, not a smooth join a knot may be taken out of.
const CORNER: f64 = 1e-3;

/// How many times a curve may be tried for a knot to take out, per control point it starts with.
const TRIES_PER_POINT: usize = 4;

/// What [`reduce_to_points`] came to.
#[derive(Clone, Debug)]
pub struct Reduction {
    /// The curve after the removals made.
    pub curve: Curve,
    /// The sum of the certified bounds of the removals made: how far `curve` is from the
    /// original, everywhere.
    pub bound: f64,
    /// Whether `curve` has at most the number of control points asked for.
    pub reached: bool,
}

/// `curve` with knots taken out, in rounds from the least a removal moves to the most `budget`
/// allows, until it has at most `cap` control points or no removal within `budget` is left.
///
/// A knot is left in where the curve has a corner (held as many times as the degree, whose two
/// sides meet at more than [`CORNER`]; held more than that), and where taking it out would move
/// the curve at a parameter of `keep` or at an end of its domain by more than rounding.
/// `budget` must be finite and positive, and `cap` more than the degree.
#[must_use]
pub fn reduce_to_points(mut cur: Curve, cap: usize, budget: f64, keep: &[f64]) -> Reduction {
    let (first, last) = cur.param_bounds();
    let held: Vec<f64> = keep
        .iter()
        .copied()
        .chain([first, last])
        .filter(|t| *t >= first && *t <= last)
        .collect();
    let need = cur.cpts().len().saturating_sub(cap).max(1);
    let tries = TRIES_PER_POINT * cur.cpts().len() + 1024;
    let mut tried = 0usize;
    let mut spent = 0.0_f64;
    // each removal may move the curve this far in the first round, then four times as far
    let mut allowance = (budget / need as f64).max(f64::MIN_POSITIVE);
    loop {
        let mut removed = false;
        let mut interior: Vec<f64> = cur
            .knots()
            .iter()
            .copied()
            .filter(|t| *t > first && *t < last)
            .collect();
        interior.dedup();
        for t in interior {
            let room = budget - spent;
            if cur.cpts().len() <= cap || room.is_nan() || room <= 0.0 {
                break;
            }
            tried += 1;
            if tried > tries {
                return Reduction {
                    curve: cur,
                    bound: spent,
                    reached: false,
                };
            }
            let tol = allowance.min(room);
            if is_corner(&cur, t) {
                continue;
            }
            let Some((next, bound)) = remove_one(&cur, t) else {
                continue;
            };
            if bound.is_nan() || bound > tol || moves_a_held_point(&cur, &next, &held, budget) {
                continue;
            }
            cur = next;
            spent += bound;
            removed = true;
        }
        let reached = cur.cpts().len() <= cap;
        if reached || (!removed && allowance >= budget - spent) {
            return Reduction {
                curve: cur,
                bound: spent,
                reached,
            };
        }
        allowance = (allowance * 4.0).min(budget);
    }
}

/// `cur` with one copy of its interior knot `t` removed, whatever it moves, and the certified
/// bound of the move; `None` when the kernel removes nothing or cannot say.
fn remove_one(cur: &Curve, t: f64) -> Option<(Curve, f64)> {
    let removal = cur.remove_knot(t, 1, f64::MAX).ok()?;
    if removal.removed == 0 {
        return None;
    }
    let bound = certified_distance(cur, &removal.curve).ok()?;
    Some((removal.curve, bound))
}

/// Whether the knot `t` of `nurb` is one the reduction leaves in: held more times than the degree
/// (a break in the curve), or as many times as the degree where the two sides of the joint are
/// not smooth ([`CORNER`]) or one has no length.
fn is_corner(nurb: &Curve, t: f64) -> bool {
    let (knots, degree) = (nurb.knots(), nurb.degree());
    let held = knots.iter().filter(|k| **k == t).count();
    if held < degree {
        return false;
    }
    if held > degree {
        return true;
    }
    let Some(r) = knots.partition_point(|k| *k <= t).checked_sub(1) else {
        return true;
    };
    let at = |i: usize| nurb.cpts().get(i).map(|p| [p.x, p.y, p.z]);
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

/// Whether `next` is at a parameter of `held` farther from `cur` than rounding (and a billionth
/// of the budget), or cannot be evaluated there.
fn moves_a_held_point(cur: &Curve, next: &Curve, held: &[f64], budget: f64) -> bool {
    held.iter().any(
        |&t| match (cur.eval(t, Side::Left), next.eval(t, Side::Left)) {
            (Ok(p), Ok(q)) => {
                let size = [p.x, p.y, p.z, q.x, q.y, q.z]
                    .iter()
                    .fold(0.0_f64, |m, x| m.max(x.abs()));
                let moved = norm([p.x - q.x, p.y - q.y, p.z - q.z]);
                moved.is_nan() || moved > 1e-9 * budget + 64.0 * f64::EPSILON * size
            }
            _ => true,
        },
    )
}
