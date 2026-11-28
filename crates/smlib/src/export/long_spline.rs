//! Bringing a long B-spline edge down to the number of control points the portable record holds.
//!
//! SMLib can hand back the curve of a short intersection edge sampled so finely that it has more
//! control points than the record accepts ([`MAX_POINTS`]). The reduction is solidcore's
//! ([`solidcore_ops::curve_knots::reduce_to_points`]: knots taken out one at a time, each removal
//! certified by the kernel); this module maps an edge's curves onto it and the refusals back onto
//! the export's. A reduction that cannot reach the cap within its budget is refused with that
//! reason; the budget is never exceeded.

use super::{Res, invalid, unsupported};
use solidcore_ops::Error;
use solidcore_ops::curve_knots::reduce_to_points;
use solidcore_record::brep as b;

/// The most control points a spline of the portable record holds (`brep::knots`).
pub(super) const MAX_POINTS: usize = 4096;

/// An edge's curves within the record's size, and the proved largest distance of either from what
/// SMLib gave.
pub(super) struct Reduced {
    pub curve: b::Curve,
    pub nurbs: Option<b::Curve>,
    pub bound: f64,
}

/// `curve` and the NURBS form of the edge (`nurbs`, the curve its parameter is on when that is not
/// `curve`) with at most [`MAX_POINTS`] control points each, within `budget` of what they were
/// everywhere. The points of `interval` (the edge's ends) are not moved. Only B-splines are
/// reduced: a rational spline over the cap is refused with the reason.
pub(super) fn within_cap(curve: b::Curve, nurbs: Option<b::Curve>, interval: [f64; 2], budget: f64) -> Res<Reduced> {
    let (curve, on_curve) = reduce(curve, interval, budget)?;
    let (nurbs, on_nurbs) = match nurbs {
        Some(n) => {
            let (n, bound) = reduce(n, interval, budget)?;
            (Some(n), bound)
        }
        None => (None, 0.0),
    };
    Ok(Reduced { curve, nurbs, bound: on_curve.max(on_nurbs) })
}

fn reduce(curve: b::Curve, interval: [f64; 2], budget: f64) -> Res<(b::Curve, f64)> {
    match &curve {
        b::Curve::Spline { points, .. } if points.len() > MAX_POINTS => {
            let before = points.len();
            match reduce_to_points(&curve, MAX_POINTS, budget, &interval) {
                Ok(reduced) => Ok(reduced),
                Err(Error::PastTolerance { bound }) => Err(unsupported(format!(
                    "an edge spline of {before} control points cannot be reduced to the {MAX_POINTS} the record holds within {budget:e} of it (the removals made moved it {bound:e})"
                ))),
                Err(_) => Err(invalid("edge spline to reduce")),
            }
        }
        b::Curve::RationalSpline { homogeneous, .. } if homogeneous.len() > MAX_POINTS => Err(unsupported(format!(
            "a rational edge spline of {} control points, more than the {MAX_POINTS} the record holds: knot removal certifies a bound for B-splines only",
            homogeneous.len()
        ))),
        _ => Ok((curve, 0.0)),
    }
}
