//! Where points fall along a line segment: the cut parameters of a profile edge that other edges
//! run along (touching profiles are joined by cutting each edge where its neighbours start and end).

/// The parameters (0 to 1 along `a` to `b`, ascending, two within `tol` of each other counted
/// once) of the `points` that lie on the segment, at least `tol` from either end and within `tol`
/// of its line. A segment no longer than `tol`, or a number that is not finite, has none.
#[must_use]
pub fn points_along_segment(a: [f64; 2], b: [f64; 2], points: &[[f64; 2]], tol: f64) -> Vec<f64> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    let mut ts: Vec<f64> = Vec::new();
    if len.is_finite() && tol.is_finite() && len > tol {
        for v in points {
            let (vx, vy) = (v[0] - a[0], v[1] - a[1]);
            let t = (vx * dx + vy * dy) / (len * len);
            if t * len > tol && (1.0 - t) * len > tol && (vx * dy - vy * dx).abs() <= tol * len {
                ts.push(t);
            }
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|x, y| (*x - *y).abs() * len <= tol);
    ts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_on_the_segment_are_cut_once_and_the_ends_and_off_line_points_are_not() {
        let (a, b) = ([0.0, 0.0], [10.0, 0.0]);
        let pts = [
            [0.0, 0.0],
            [10.0, 0.0],
            [4.0, 0.0],
            [4.0, 0.0],
            [7.0, 0.5],
            [12.0, 0.0],
        ];
        assert_eq!(points_along_segment(a, b, &pts, 1e-6), vec![0.4]);
        assert!(points_along_segment(a, a, &pts, 1e-6).is_empty());
    }
}
