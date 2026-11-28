//! Where points fall along a line segment: the cut parameters of a profile edge that other edges
//! run along, from the kernel's [`solidcore::line_split`] primitive (which computes them).

use solidcore_record::Vec2;

/// The parameters (0 to 1 along `a` to `b`, ascending, two within `tol` of each other counted
/// once) of the `points` that lie on the segment, at least `tol` from either end and within `tol`
/// of its line. A segment no longer than `tol` has none.
#[must_use]
pub fn points_along_segment(a: Vec2, b: Vec2, points: &[Vec2], tol: f64) -> Vec<f64> {
    let pts: Vec<[f64; 2]> = points.iter().map(|p| [p.x, p.y]).collect();
    solidcore::line_split::points_along_segment([a.x, a.y], [b.x, b.y], &pts, tol)
}
