//! Knots and degree of a free curve: a knot inserted or removed, the degree raised or lowered,
//! the curve split in two at a parameter. The curve is the record's ([`Curve`], a clamped
//! spline or rational spline); every step is the kernel's ([`NurbCurve::insert_knot`],
//! [`NurbCurve::remove_knot`], [`NurbCurve::elevate_degree`], [`NurbCurve::reduce_degree_once`],
//! [`NurbCurve::split`]: The NURBS Book's algorithms A5.1, A5.8, A5.9 and A5.11, and knot
//! insertion to full multiplicity), and a change that is not exact is kept only within a
//! tolerance of the kernel's certified bound ([`certified_distance`]). Ops checks the inputs
//! and converts.
//! A closed periodic curve (unclamped knots, as `curve_fit::closed_cv_curve` makes) is refused
//! for now: the kernel's knot routines take clamped knots. A curve's joints, where its knots make
//! it at most continuous ([`joints`]), are read from its knots as they are, clamped or not.

use solidcore::error::nurbs::NurbsFailure;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::crv_deviation::certified_distance;
use solidcore::nurbs::crv_reduce::reduce_to_points as reduce_to_points_of;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore_record::brep::{check_curve, Budget, Curve, Natural, MAX_SPLINE_DEGREE};

use crate::session::run;
use crate::{Error, Result};

/// Which way round the line through `origin` along `axis` `curve` turns over `interval` of its
/// parameter, as the kernel proves it ([`solidcore::nurbs::certificate::turn_about_axis`]):
/// `Some(1.0)` counter-clockwise seen from the tip of `axis`, `Some(-1.0)` clockwise, `None`
/// when that is not proved (the curve turns back or stands still about the line somewhere, or
/// the bound cannot tell). Any knots the record takes.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid spline or rational spline, or a
/// coordinate or interval end that is not finite; the kernel's status otherwise (an interval
/// that is not increasing).
pub fn turn_about_axis(
    curve: &Curve,
    interval: [f64; 2],
    origin: [f64; 3],
    axis: [f64; 3],
) -> Result<Option<f64>> {
    let nurb = net_of(curve)?;
    if !origin
        .iter()
        .chain(&axis)
        .chain(&interval)
        .all(|x| x.is_finite())
    {
        return Err(Error::Argument);
    }
    let ([ox, oy, oz], [ax, ay, az]) = (origin, axis);
    let (o, a) = (
        solidcore::Point3::new(ox, oy, oz),
        solidcore::Vec3::new(ax, ay, az),
    );
    run("curve_knots::turn_about_axis", |_| {
        Ok(Ok(solidcore::nurbs::certificate::turn_about_axis(
            &nurb, o, a, interval,
        )?))
    })?
}

/// `curve` with the knot `t` inserted once: the same curve, one control point more.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid clamped spline or rational spline, a `t`
/// not strictly inside its domain, or a knot already as many times as the degree (one more would
/// break the curve); the kernel's status otherwise.
pub fn insert_knot(curve: &Curve, t: f64) -> Result<Curve> {
    let nurb = nurb_of(curve)?;
    if !interior(curve, t) || multiplicity(nurb.knots(), t) >= nurb.degree() {
        return Err(Error::Argument);
    }
    run("curve_knots::insert_knot", |_| {
        Ok(record_of(&nurb.insert_knot(t, 1)?))
    })?
}

/// `curve` with a knot inserted at the middle of each of its spans
/// ([`solidcore::nurbs::knots::span_midpoints`]; Boehm's insertion, [`NurbCurve::insert_knot`]):
/// the same curve, twice the spans, so a curve fitted on its knots has twice the freedom.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid clamped spline or rational spline, or a
/// result the record cannot hold (more control points than it takes); the kernel's status
/// otherwise.
pub fn halve_spans(curve: &Curve) -> Result<Curve> {
    let nurb = nurb_of(curve)?;
    let Natural::Range(t0, t1) = curve.natural() else {
        return Err(Error::Argument);
    };
    let mids = solidcore::nurbs::knots::span_midpoints(nurb.knots(), [t0, t1]);
    run("curve_knots::halve_spans", |_| {
        let mut halved = nurb;
        for t in mids {
            halved = halved.insert_knot(t, 1)?;
        }
        Ok(record_of(&halved))
    })?
}

/// `curve` with one copy of its interior knot `t` removed (The NURBS Book, A5.8; a knot of
/// multiplicity `m` keeps `m - 1`), and the kernel's certified bound of how far the result is
/// from `curve` ([`solidcore::nurbs::crv_deviation::certified_distance`], measured against
/// the curve as it was, never the removal's own estimate), kept when within `tol`.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid clamped spline (a rational spline, whose
/// bound the kernel does not certify yet, and a closed periodic one are refused), a `t` that is
/// not one of its interior knots, or a `tol` not finite or negative;
/// [`Error::PastTolerance`] with the bound when it is past `tol`; the kernel's status otherwise.
pub fn remove_knot(curve: &Curve, t: f64, tol: f64) -> Result<(Curve, f64)> {
    let nurb = nurb_of(curve)?;
    if !matches!(curve, Curve::Spline { .. })
        || !interior(curve, t)
        || multiplicity(nurb.knots(), t) == 0
        || !tol.is_finite()
        || tol < 0.0
    {
        return Err(Error::Argument);
    }
    run("curve_knots::remove_knot", |_| {
        Ok(remove_one(&nurb, t)?.and_then(|(c, bound)| {
            if bound <= tol {
                record_of(&c).map(|c| (c, bound))
            } else {
                Err(Error::PastTolerance { bound })
            }
        }))
    })?
}

/// `nurb` with one copy of its interior knot `t` removed, whatever it moves, and the kernel's
/// certified bound of the move ([`certified_distance`] against `nurb` as it is); [`Error::NoEffect`]
/// when the kernel removes nothing.
fn remove_one(nurb: &NurbCurve, t: f64) -> solidcore::Result<Result<(NurbCurve, f64)>> {
    let removal = nurb.remove_knot(t, 1, f64::MAX)?;
    if removal.removed == 0 {
        return Ok(Err(Error::NoEffect));
    }
    let bound = certified_distance(nurb, &removal.curve)?;
    Ok(Ok((removal.curve, bound)))
}

/// `curve`, a clamped spline with any number of control points, with knots taken out until it has
/// at most `cap`, and the kernel's certified distance of the result from `curve` (the largest
/// distance between them, over the whole curve), kept within `budget`. A curve with at most `cap`
/// points is returned as it is, bound 0.
///
/// A knot is left in where the curve has a corner (a knot held as many times as the degree whose
/// two sides meet at more than a tiny angle) and where taking it out would move the curve at one
/// of `keep`, or at the ends of its domain, by more than rounding: those points stay where they are.
/// Knots are taken out greedily, the one whose removal moves the curve least first, and the
/// result is checked against `budget` once.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a clamped spline (a rational one is refused: the
/// kernel does not certify its bound), a `cap` the degree does not allow or the record does not
/// hold, a `budget` that is not finite or is not positive and normal, or a `keep` not finite;
/// [`Error::PastTolerance`] with the certified distance of the best result when `cap` is not
/// reached within `budget`.
pub fn reduce_to_points(
    curve: &Curve,
    cap: usize,
    budget: f64,
    keep: &[f64],
) -> Result<(Curve, f64)> {
    let Curve::Spline { knots, points } = curve else {
        return Err(Error::Argument);
    };
    if !budget.is_finite() || budget < f64::MIN_POSITIVE || !keep.iter().all(|t| t.is_finite()) {
        return Err(Error::Argument);
    }
    let nurb = long_nurb_of(knots, points)?;
    if cap > 4096 || cap <= nurb.degree() {
        return Err(Error::Argument);
    }
    if nurb.cpts().len() <= cap {
        return Ok((curve.clone(), 0.0));
    }
    run("curve_knots::reduce_to_points", |_| {
        let done = reduce_to_points_of(nurb, cap, budget, keep)?;
        Ok(if done.reached {
            record_of(&done.curve).map(|c| (c, done.bound))
        } else {
            Err(Error::PastTolerance { bound: done.bound })
        })
    })?
}

/// The kernel's curve of a clamped spline of any size, valid as the record takes a spline but for
/// the number of its control points.
fn long_nurb_of(knots: &[f64], points: &[[f64; 3]]) -> Result<NurbCurve> {
    let degree = knots
        .len()
        .checked_sub(points.len() + 1)
        .ok_or(Error::Argument)?;
    let n = points.len();
    let flat =
        |ends: Option<&[f64]>| ends.is_some_and(|e| e.windows(2).all(|w| w.first() == w.get(1)));
    let valid = (1..=MAX_SPLINE_DEGREE).contains(&degree)
        && n > degree
        && knots.iter().all(|k| k.is_finite())
        && knots.windows(2).all(|w| w.first() <= w.get(1))
        && points
            .iter()
            .flatten()
            .all(|x| x.is_finite() && x.abs() <= 1e12)
        && flat(knots.get(..=degree))
        && flat(knots.get(n..))
        && knots.get(degree) < knots.get(n);
    if !valid {
        return Err(Error::Argument);
    }
    let pw: Vec<CPoint> = points
        .iter()
        .map(|&[x, y, z]| CPoint::new(x, y, z, NOW))
        .collect();
    NurbCurve::new(pw, degree, knots.to_vec()).map_err(|_| Error::Argument)
}

/// `curve` of degree `degree`, and the kernel's certified bound of how far it is from `curve`
/// ([`solidcore::nurbs::crv_deviation::certified_distance`]). Raising it is exact in exact
/// arithmetic (the same curve, more control points; `tol` is not used), its bound the
/// rounding; the same degree is the curve as it is, bound 0; lowering it (The NURBS Book,
/// A5.11, once per degree) is kept when the bound is within `tol`. A rational spline is raised
/// with no bound (`None`): the kernel's bound does not take rational curves yet.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid clamped spline or rational spline (a
/// closed periodic one is refused), a degree of 0 or past the record's most
/// ([`MAX_SPLINE_DEGREE`]), a `tol` not finite or negative, or a lower degree of a rational
/// spline (whose bound the kernel does not certify yet); [`Error::PastTolerance`] with the
/// bound when it is past `tol`; [`Error::NoEffect`] when the kernel does not lower it; the
/// kernel's status otherwise.
pub fn set_degree(curve: &Curve, degree: usize, tol: f64) -> Result<(Curve, Option<f64>)> {
    let nurb = nurb_of(curve)?;
    if degree == 0 || degree > MAX_SPLINE_DEGREE || !tol.is_finite() || tol < 0.0 {
        return Err(Error::Argument);
    }
    if let Some(raise) = degree.checked_sub(nurb.degree()) {
        if raise == 0 {
            return Ok((curve.clone(), Some(0.0)));
        }
        let rational = !matches!(curve, Curve::Spline { .. });
        return run("curve_knots::set_degree", |_| {
            let raised = nurb.elevate_degree(raise)?;
            if rational {
                return Ok(record_of(&raised).map(|c| (c, None)));
            }
            let bound = certified_distance(&nurb, &raised)?;
            Ok(record_of(&raised).map(|c| (c, Some(bound))))
        })?;
    }
    if !matches!(curve, Curve::Spline { .. }) {
        return Err(Error::Argument);
    }
    run("curve_knots::set_degree", |_| {
        let mut lowered = nurb.clone();
        while lowered.degree() > degree {
            // lowered whatever it moves; the certified bound decides
            match lowered.reduce_degree_once(f64::MAX)?.curve {
                Some(c) => lowered = c,
                None => return Ok(Err(Error::NoEffect)),
            }
        }
        Ok(within(&nurb, &lowered, tol)?.map(|(c, bound)| (c, Some(bound))))
    })?
}

/// `curve` cut at `t`, strictly inside its domain: the piece before (over `[start, t]`) and
/// the piece after (over `[t, end]`), each a clamped curve of `curve`'s degree, rational where
/// `curve` is, together the same shape ([`NurbCurve::split`]: `t` inserted to full
/// multiplicity and the control points shared out). A `t` within `1e-6` of the domain's length
/// of a knot is moved onto that knot (the kernel's snap), so the pieces meet on it.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid clamped spline or rational spline (a
/// closed periodic one is refused: cut it open at its seam first), or a `t` not strictly inside
/// its domain or within the kernel's snap of an end; the kernel's status otherwise.
pub fn split_at(curve: &Curve, t: f64) -> Result<(Curve, Curve)> {
    let nurb = nurb_of(curve)?;
    if !interior(curve, t) {
        return Err(Error::Argument);
    }
    run("curve_knots::split_at", |_| {
        let (before, after) = match nurb.split(t) {
            Ok(pieces) => pieces,
            Err(e) if e.nurbs_failure() == Some(&NurbsFailure::Parameter) => {
                return Ok(Err(Error::Argument))
            }
            Err(e) => return Err(e),
        };
        Ok(record_of(&before).and_then(|b| record_of(&after).map(|a| (b, a))))
    })?
}

/// The joints of spline `curve` strictly inside `interval`, in order: its knots held at least
/// as many times as its degree, where it is at most continuous, as where the pieces of a chain
/// meet ([`solidcore::nurbs::knots::joints`]).
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a spline or rational spline, or whose knots are
/// fewer than its control points need.
pub fn joints(curve: &Curve, interval: [f64; 2]) -> Result<Vec<f64>> {
    let (knots, count) = match curve {
        Curve::Spline { knots, points } => (knots, points.len()),
        Curve::RationalSpline { knots, homogeneous } => (knots, homogeneous.len()),
        _ => return Err(Error::Argument),
    };
    let degree = knots.len().checked_sub(count + 1).ok_or(Error::Argument)?;
    Ok(solidcore::nurbs::knots::joints(knots, degree, interval))
}

/// The record of `changed` and the kernel's certified bound of its distance from `original`,
/// when that is within `tol`, else [`Error::PastTolerance`] with the bound.
fn within(
    original: &NurbCurve,
    changed: &NurbCurve,
    tol: f64,
) -> solidcore::Result<Result<(Curve, f64)>> {
    let bound = certified_distance(original, changed)?;
    Ok(if bound <= tol {
        record_of(changed).map(|c| (c, bound))
    } else {
        Err(Error::PastTolerance { bound })
    })
}

/// The kernel's curve of a record spline or rational spline that is valid and clamped (its
/// first and last `degree + 1` knots equal), as the kernel's knot routines take it.
///
/// # Errors
/// [`Error::Argument`] for another curve, one the record refuses, or knots not clamped.
pub(crate) fn nurb_of(curve: &Curve) -> Result<NurbCurve> {
    let nurb = net_of(curve)?;
    let (knots, degree) = (nurb.knots(), nurb.degree());
    let flat =
        |ends: Option<&[f64]>| ends.is_some_and(|e| e.windows(2).all(|w| w.first() == w.get(1)));
    let last = knots.len().checked_sub(degree + 1);
    if !flat(knots.get(..=degree)) || !flat(last.and_then(|s| knots.get(s..))) {
        return Err(Error::Argument);
    }
    Ok(nurb)
}

/// The kernel's curve of a record spline or rational spline that is valid, on any knots the
/// record takes (clamped, or a closed curve's periodic ones): its control points and their
/// weights, its degree and knots as they are.
///
/// # Errors
/// [`Error::Argument`] for another curve, or one the record refuses.
pub(crate) fn net_of(curve: &Curve) -> Result<NurbCurve> {
    check_curve(curve, &mut Budget::input()).map_err(|_| Error::Argument)?;
    let (knots, pw): (&[f64], Vec<CPoint>) = match curve {
        Curve::Spline { knots, points } => (
            knots,
            points
                .iter()
                .map(|&[x, y, z]| CPoint::new(x, y, z, NOW))
                .collect(),
        ),
        Curve::RationalSpline { knots, homogeneous } => (
            knots,
            homogeneous
                .iter()
                .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                .collect(),
        ),
        _ => return Err(Error::Argument),
    };
    let degree = knots
        .len()
        .checked_sub(pw.len() + 1)
        .ok_or(Error::Argument)?;
    NurbCurve::new(pw, degree, knots.to_vec()).map_err(|_| Error::Argument)
}

/// The record of the kernel's curve `nurb`: a spline where no control point carries a weight,
/// a rational spline where every one does.
///
/// # Errors
/// [`Error::Argument`] for a curve mixing the two, or one the record refuses (more control
/// points or a higher degree than a record holds).
pub(crate) fn record_of(nurb: &NurbCurve) -> Result<Curve> {
    let pw = nurb.cpts();
    let knots = nurb.knots().to_vec();
    let curve = if pw.iter().all(|p| p.w == NOW) {
        Curve::Spline {
            knots,
            points: pw.iter().map(|p| [p.x, p.y, p.z]).collect(),
        }
    } else if pw.iter().all(|p| p.w != NOW) {
        Curve::RationalSpline {
            knots,
            homogeneous: pw.iter().map(|p| [p.x, p.y, p.z, p.w]).collect(),
        }
    } else {
        return Err(Error::Argument);
    };
    check_curve(&curve, &mut Budget::input()).map_err(|_| Error::Argument)?;
    Ok(curve)
}

/// Whether `t` lies strictly inside the domain of the (valid) spline `curve`.
fn interior(curve: &Curve, t: f64) -> bool {
    matches!(curve.natural(), Natural::Range(a, b) if a < t && t < b)
}

/// How many times `t` appears among `knots`.
fn multiplicity(knots: &[f64], t: f64) -> usize {
    knots.iter().filter(|k| **k == t).count()
}

#[cfg(test)]
// the oracles are closed forms, not ops geometry
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::curve::cv_curve;
    use solidcore::nurbs::knots::Side;

    fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn norm(a: [f64; 3]) -> f64 {
        (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
    }

    /// Six control points no cubic with fewer knots passes through.
    const WAVE: [[f64; 3]; 6] = [
        [0.0, 0.0, 0.0],
        [1.0, 2.0, 0.0],
        [2.0, -1.0, 0.0],
        [3.0, 3.0, 0.0],
        [4.0, 0.0, 1.0],
        [5.0, 1.0, 0.0],
    ];

    /// The point of `c` at `t`, by the kernel's evaluator.
    fn at(c: &Curve, t: f64) -> [f64; 3] {
        let p = nurb_of(c).unwrap().eval(t, Side::Left).unwrap();
        [p.x, p.y, p.z]
    }

    /// `a` and `b` are the same curve: the same point at 101 parameters across `[0, 1]`.
    fn same_shape(a: &Curve, b: &Curve, tol: f64) {
        for i in 0..=100 {
            let t = f64::from(i) / 100.0;
            let (p, q) = (at(a, t), at(b, t));
            assert!(
                p.iter().zip(q).all(|(x, y)| (x - y).abs() <= tol),
                "at {t}: {p:?} and {q:?}"
            );
        }
    }

    /// A spline's knots and control point count.
    fn parts(c: &Curve) -> (&[f64], usize) {
        match c {
            Curve::Spline { knots, points } => (knots, points.len()),
            Curve::RationalSpline { knots, homogeneous } => (knots, homogeneous.len()),
            _ => panic!("a spline: {c:?}"),
        }
    }

    #[test]
    fn an_inserted_knot_adds_a_point_and_keeps_the_shape() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let more = insert_knot(&c, 0.3).unwrap();
        let (knots, count) = parts(&more);
        assert_eq!(count, WAVE.len() + 1);
        assert_eq!(multiplicity(knots, 0.3), 1);
        assert_eq!(knots.len(), parts(&c).0.len() + 1);
        same_shape(&c, &more, 1e-12);

        // Up to the degree at one value; one more is refused.
        let mut c3 = c.clone();
        for _ in 0..3 {
            c3 = insert_knot(&c3, 0.5).unwrap();
        }
        assert_eq!(multiplicity(parts(&c3).0, 0.5), 3);
        same_shape(&c, &c3, 1e-12);
        assert_eq!(insert_knot(&c3, 0.5), Err(Error::Argument));
    }

    /// The cubic on six points has three spans (inner knots 1/3 and 2/3): halved, the knots
    /// 1/6, 1/2 and 5/6 come in, three points more, the same curve. A quarter circle's one span
    /// halved stays rational, on its circle.
    #[test]
    fn halved_spans_keep_the_shape() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let halved = halve_spans(&c).unwrap();
        let (knots, count) = parts(&halved);
        assert_eq!(count, WAVE.len() + 3);
        for t in [1.0 / 6.0, 0.5, 5.0 / 6.0] {
            assert!(
                knots.iter().any(|k| (k - t).abs() < 1e-15),
                "{t}: {knots:?}"
            );
        }
        same_shape(&c, &halved, 1e-12);
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [10.0, 0.0, 0.0, 1.0],
                [10.0 * w, 10.0 * w, 0.0, w],
                [0.0, 10.0, 0.0, 1.0],
            ],
        };
        let halved = halve_spans(&arc).unwrap();
        assert!(matches!(halved, Curve::RationalSpline { .. }), "{halved:?}");
        assert_eq!(parts(&halved).1, 4);
        same_shape(&arc, &halved, 1e-12);
        let line = Curve::Line {
            start: [0.0; 3],
            end: [1.0; 3],
        };
        assert_eq!(halve_spans(&line), Err(Error::Argument));
    }

    #[test]
    fn a_knot_outside_the_domain_or_on_a_curve_without_knots_is_refused() {
        let c = cv_curve(&WAVE, 3).unwrap();
        for t in [0.0, 1.0, -0.5, 1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(insert_knot(&c, t), Err(Error::Argument), "{t}");
        }
        let line = Curve::Line {
            start: [0.0; 3],
            end: [1.0; 3],
        };
        assert_eq!(insert_knot(&line, 0.5), Err(Error::Argument));
        assert_eq!(remove_knot(&line, 0.5, 1.0), Err(Error::Argument));
        assert_eq!(set_degree(&line, 3, 0.0), Err(Error::Argument));
    }

    #[test]
    fn a_knot_inserted_then_removed_gives_the_curve_back() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let more = insert_knot(&c, 0.3).unwrap();
        let (back, bound) = remove_knot(&more, 0.3, 1e-9).unwrap();
        assert!(bound < 1e-12, "{bound}");
        let (knots, count) = parts(&back);
        assert_eq!(knots, parts(&c).0);
        assert_eq!(count, WAVE.len());
        same_shape(&c, &back, 1e-9);
    }

    #[test]
    fn an_inserted_knot_moves_the_curve_by_no_more_than_rounding() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let more = insert_knot(&c, 0.3).unwrap();
        let bound = certified_distance(&nurb_of(&c).unwrap(), &nurb_of(&more).unwrap()).unwrap();
        assert!(bound < 1e-12, "{bound}");
        for i in 0..=200 {
            let t = f64::from(i) / 200.0;
            let (p, q) = (at(&c, t), at(&more, t));
            let gap2: f64 = p.iter().zip(q).map(|(a, b)| (a - b) * (a - b)).sum();
            assert!(gap2 <= bound * bound, "at {t}: {gap2} past {bound}²");
        }
    }

    #[test]
    fn a_repeated_knot_is_removed_one_copy_at_a_time() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let twice = insert_knot(&insert_knot(&c, 0.5).unwrap(), 0.5).unwrap();
        assert_eq!(multiplicity(parts(&twice).0, 0.5), 2);
        let (once, _) = remove_knot(&twice, 0.5, 1e-9).unwrap();
        assert_eq!(multiplicity(parts(&once).0, 0.5), 1);
        assert_eq!(parts(&once).1, WAVE.len() + 1);
        let (none, bound) = remove_knot(&once, 0.5, 1e-9).unwrap();
        assert_eq!(multiplicity(parts(&none).0, 0.5), 0);
        assert_eq!(parts(&none).0, parts(&c).0);
        // measured against the curve it started from, within rounding of it
        assert!(bound < 1e-9, "{bound}");
        same_shape(&c, &none, 1e-9);
    }

    #[test]
    fn a_knot_the_shape_needs_is_not_removed() {
        let c = cv_curve(&WAVE, 3).unwrap();
        // The interior knots of six points' cubic are 1/3 and 2/3.
        let first = parts(&c).0[4];
        assert!(first > 0.0 && first < 1.0, "{first}");
        assert!(
            matches!(remove_knot(&c, first, 1e-6), Err(Error::PastTolerance { bound }) if bound > 1e-6)
        );
        // Not a knot, an end knot, or a bad tolerance.
        assert_eq!(remove_knot(&c, 0.25, 1.0), Err(Error::Argument));
        assert_eq!(remove_knot(&c, 0.0, 1.0), Err(Error::Argument));
        assert_eq!(remove_knot(&c, first, -1.0), Err(Error::Argument));
        assert_eq!(remove_knot(&c, first, f64::NAN), Err(Error::Argument));
    }

    #[test]
    fn a_raised_degree_keeps_the_shape() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let (quintic, bound) = set_degree(&c, 5, 0.0).unwrap();
        assert!(bound.is_some_and(|b| b < 1e-12), "{bound:?}");
        let (knots, count) = parts(&quintic);
        assert_eq!(knots.len() - count - 1, 5);
        assert_eq!(knots.first(), Some(&0.0));
        assert_eq!(knots.last(), Some(&1.0));
        same_shape(&c, &quintic, 1e-12);
        assert_eq!(set_degree(&c, 3, 0.0), Ok((c.clone(), Some(0.0))));
        assert_eq!(set_degree(&c, 0, 0.0), Err(Error::Argument));
        assert_eq!(
            set_degree(&c, MAX_SPLINE_DEGREE + 1, 0.0),
            Err(Error::Argument)
        );
        assert_eq!(set_degree(&c, 5, f64::NAN), Err(Error::Argument));
    }

    #[test]
    fn a_degree_is_lowered_only_within_the_certified_tolerance() {
        let c = cv_curve(&WAVE, 3).unwrap();
        // a raised curve comes back down to its own degree, within rounding
        let (quintic, bound) = set_degree(&c, 5, 0.0).unwrap();
        assert!(bound.is_some_and(|b| b < 1e-12), "{bound:?}");
        let (back, bound) = set_degree(&quintic, 3, 1e-9).unwrap();
        assert!(bound.is_some_and(|b| b <= 1e-9), "{bound:?}");
        let (knots, count) = parts(&back);
        assert_eq!(knots.len() - count - 1, 3);
        same_shape(&c, &back, 1e-9);
        // the wave is no quadratic
        assert!(
            matches!(set_degree(&c, 2, 1e-6), Err(Error::PastTolerance { bound }) if bound > 1e-6)
        );
    }

    #[test]
    fn a_raised_arc_stays_on_its_circle() {
        // A quarter circle of radius 10 about the origin, as a rational quadratic.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [10.0, 0.0, 0.0, 1.0],
                [10.0 * w, 10.0 * w, 0.0, w],
                [0.0, 10.0, 0.0, 1.0],
            ],
        };
        let (cubic, bound) = set_degree(&arc, 3, 0.0).unwrap();
        assert_eq!(bound, None);
        assert!(matches!(cubic, Curve::RationalSpline { .. }));
        assert_eq!(parts(&cubic).1, 4);
        same_shape(&arc, &cubic, 1e-12);
        for i in 0..=20 {
            let p = at(&cubic, f64::from(i) / 20.0);
            let r2 = p[0] * p[0] + p[1] * p[1];
            assert!((r2 - 100.0).abs() < 1e-9, "r² = {r2}");
        }
        // a rational curve's removal and lowering wait for a certified rational bound
        assert_eq!(set_degree(&cubic, 2, 1.0), Err(Error::Argument));
        let more = insert_knot(&arc, 0.5).unwrap();
        assert_eq!(remove_knot(&more, 0.5, 1.0), Err(Error::Argument));
    }

    #[test]
    fn unclamped_knots_are_refused() {
        let floating = Curve::Spline {
            knots: (0..10).map(f64::from).collect(),
            points: WAVE.to_vec(),
        };
        assert_eq!(nurb_of(&floating).err(), Some(Error::Argument));
        assert_eq!(insert_knot(&floating, 4.5), Err(Error::Argument));
    }

    /// The degree and domain of a clamped spline.
    fn degree_and_domain(c: &Curve) -> (usize, [f64; 2]) {
        let (knots, count) = parts(c);
        (knots.len() - count - 1, [knots[0], knots[knots.len() - 1]])
    }

    /// `piece` is `whole` over the piece's domain: the same point at 101 parameters.
    fn same_over(piece: &Curve, whole: &Curve, tol: f64) {
        let (_, [a, b]) = degree_and_domain(piece);
        for i in 0..=100 {
            let t = a + (b - a) * f64::from(i) / 100.0;
            let (p, q) = (at(piece, t), at(whole, t));
            assert!(
                p.iter().zip(q).all(|(x, y)| (x - y).abs() <= tol),
                "at {t}: {p:?} and {q:?}"
            );
        }
    }

    #[test]
    fn the_two_pieces_of_a_split_are_the_curve_on_each_side() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let (before, after) = split_at(&c, 0.4).unwrap();
        assert_eq!(degree_and_domain(&before), (3, [0.0, 0.4]));
        assert_eq!(degree_and_domain(&after), (3, [0.4, 1.0]));
        same_over(&before, &c, 1e-12);
        same_over(&after, &c, 1e-12);
        // clamped: the pieces start and end on their end points, and meet
        let (Curve::Spline { points: p0, .. }, Curve::Spline { points: p1, .. }) =
            (&before, &after)
        else {
            panic!("splines: {before:?} {after:?}")
        };
        assert_eq!(p0[0], WAVE[0]);
        assert_eq!(p1[p1.len() - 1], WAVE[5]);
        let (m0, m1) = (p0[p0.len() - 1], p1[0]);
        assert!(m0.iter().zip(m1).all(|(a, b)| (a - b).abs() < 1e-12));
        assert!(m0
            .iter()
            .zip(at(&c, 0.4))
            .all(|(a, b)| (a - b).abs() < 1e-12));

        // at an interior knot (1/3 for six points' cubic)
        let knot = parts(&c).0[4];
        let (before, after) = split_at(&c, knot).unwrap();
        assert_eq!(degree_and_domain(&before).1, [0.0, knot]);
        same_over(&before, &c, 1e-12);
        same_over(&after, &c, 1e-12);
    }

    #[test]
    fn a_split_arc_stays_rational_on_its_circle() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [10.0, 0.0, 0.0, 1.0],
                [10.0 * w, 10.0 * w, 0.0, w],
                [0.0, 10.0, 0.0, 1.0],
            ],
        };
        let (before, after) = split_at(&arc, 0.5).unwrap();
        for piece in [&before, &after] {
            assert!(matches!(piece, Curve::RationalSpline { .. }), "{piece:?}");
            assert_eq!(degree_and_domain(piece).0, 2);
            same_over(piece, &arc, 1e-12);
            let (_, [a, b]) = degree_and_domain(piece);
            for i in 0..=20 {
                let p = at(piece, a + (b - a) * f64::from(i) / 20.0);
                let r2 = p[0] * p[0] + p[1] * p[1];
                assert!((r2 - 100.0).abs() < 1e-9, "r² = {r2}");
            }
        }
    }

    #[test]
    fn a_split_at_an_end_outside_or_of_a_closed_curve_is_refused() {
        let c = cv_curve(&WAVE, 3).unwrap();
        for t in [0.0, 1.0, -0.5, 1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(split_at(&c, t), Err(Error::Argument), "{t}");
        }
        // within the kernel's snap of an end: moved onto it, so no piece is left
        assert_eq!(split_at(&c, 1e-8), Err(Error::Argument));
        assert_eq!(split_at(&c, 1.0 - 1e-8), Err(Error::Argument));
        let closed = crate::curve_fit::closed_cv_curve(&WAVE, 3).unwrap();
        assert_eq!(split_at(&closed, 0.5), Err(Error::Argument));
        let line = Curve::Line {
            start: [0.0; 3],
            end: [1.0; 3],
        };
        assert_eq!(split_at(&line, 0.5), Err(Error::Argument));
    }

    #[test]
    fn joints_are_the_knots_held_as_many_times_as_the_degree() {
        // The cubic on WAVE has the inner knots 1/3 and 2/3 once each: no joint. With 0.5
        // inserted three times, the degree, it has one there, and only strictly inside.
        let mut c = cv_curve(&WAVE, 3).unwrap();
        assert_eq!(joints(&c, [0.0, 1.0]), Ok(vec![]));
        for _ in 0..3 {
            c = insert_knot(&c, 0.5).unwrap();
        }
        assert_eq!(joints(&c, [0.0, 1.0]), Ok(vec![0.5]));
        assert_eq!(joints(&c, [0.5, 1.0]), Ok(vec![]));
        // Unclamped knots are read as they are: 0 to 9 under 6 points, a cubic, each held once.
        let floating = Curve::Spline {
            knots: (0..10).map(f64::from).collect(),
            points: WAVE.to_vec(),
        };
        assert_eq!(joints(&floating, [3.0, 6.0]), Ok(vec![]));
        // Not a spline, or fewer knots than its points need: refused.
        let line = Curve::Line {
            start: [0.0; 3],
            end: [1.0; 3],
        };
        assert_eq!(joints(&line, [0.0, 1.0]), Err(Error::Argument));
        let short = Curve::Spline {
            knots: vec![0.0, 1.0],
            points: WAVE.to_vec(),
        };
        assert_eq!(joints(&short, [0.0, 1.0]), Err(Error::Argument));
    }

    /// A clamped cubic chain of Bezier pieces over `[t0, t1]` through `m` samples of a curve: knots
    /// of multiplicity 3 at the joints, a Hermite fit of each piece, as a fine trace is held.
    fn chain(
        m: usize,
        [t0, t1]: [f64; 2],
        f: impl Fn(f64) -> [f64; 3],
        df: impl Fn(f64) -> [f64; 3],
    ) -> Curve {
        let h = (t1 - t0) / (m - 1) as f64;
        let mut knots = vec![t0; 4];
        let mut points = Vec::new();
        for k in 0..m - 1 {
            let (a, z) = (t0 + k as f64 * h, t0 + (k + 1) as f64 * h);
            let (p0, p1, d0, d1) = (f(a), f(z), df(a), df(z));
            if k == 0 {
                points.push(p0);
            }
            points.push([0, 1, 2].map(|c| p0[c] + d0[c] * h / 3.0));
            points.push([0, 1, 2].map(|c| p1[c] - d1[c] * h / 3.0));
            points.push(p1);
            if k + 1 < m - 1 {
                knots.extend([z; 3]);
            }
        }
        knots.extend([t1; 4]);
        Curve::Spline { knots, points }
    }

    fn helix(t: f64) -> [f64; 3] {
        let a = 6.0 * std::f64::consts::PI * t;
        [3.0 * a.cos(), 3.0 * a.sin(), 20.0 * t]
    }

    fn dhelix(t: f64) -> [f64; 3] {
        let a = 6.0 * std::f64::consts::PI * t;
        let w = 6.0 * std::f64::consts::PI;
        [-3.0 * w * a.sin(), 3.0 * w * a.cos(), 20.0]
    }

    /// The count of control points and the knots of a spline of any size.
    fn count_of(c: &Curve) -> usize {
        match c {
            Curve::Spline { points, .. } => points.len(),
            _ => panic!("a spline: {c:?}"),
        }
    }

    /// The point of the spline `c` (of any size) at `t`.
    fn long_at(c: &Curve, t: f64) -> [f64; 3] {
        let Curve::Spline { knots, points } = c else {
            panic!("a spline: {c:?}")
        };
        let p = long_nurb_of(knots, points)
            .unwrap()
            .eval(t, Side::Left)
            .unwrap();
        [p.x, p.y, p.z]
    }

    /// The largest distance of two splines sampled across `[a, z]`.
    fn gap(x: &Curve, y: &Curve, [a, z]: [f64; 2]) -> f64 {
        (0..=4000)
            .map(|i| {
                let t = a + (z - a) * f64::from(i) / 4000.0;
                norm(sub(long_at(x, t), long_at(y, t)))
            })
            .fold(0.0, f64::max)
    }

    /// 1400 pieces: 4201 points, past the 4096 the record holds, brought down to the cap by knot
    /// removal. The sum of the certified bounds is within the budget and bounds the real gap.
    #[test]
    fn a_long_chain_comes_down_to_the_cap_within_its_budget() {
        let c = chain(1401, [0.0, 1.0], helix, dhelix);
        assert_eq!(count_of(&c), 4201);
        let budget = 2e-3;
        let (reduced, bound) = reduce_to_points(&c, 4096, budget, &[]).unwrap();
        assert!(count_of(&reduced) <= 4096, "{}", count_of(&reduced));
        assert!(
            bound > 0.0 && bound <= budget,
            "bound {bound} budget {budget}"
        );
        let real = gap(&c, &reduced, [0.0, 1.0]);
        assert!(real <= bound, "real {real} bound {bound}");
        assert!(check_curve(&reduced, &mut Budget::input()).is_ok());
        // the ends do not move
        for t in [0.0, 1.0] {
            assert_eq!(long_at(&c, t), long_at(&reduced, t));
        }
    }

    /// Knots inserted exactly add points and no shape: taking them out costs nothing, so a curve
    /// that reduces to the cap alone reduces to it with a thousand of them added, however many
    /// knots there are to try.
    #[test]
    fn exact_extra_knots_do_not_make_a_reducible_curve_refused() {
        let m = 3000usize;
        let mut knots = vec![0.0; 4];
        knots.extend((1..m - 3).map(|i| i as f64));
        knots.extend([(m - 3) as f64; 4]);
        let points: Vec<[f64; 3]> = (0..m)
            .map(|i| {
                let amp = if i < 2500 { 1.0 } else { 0.5 };
                [i as f64 * 0.3, if i % 2 == 0 { amp } else { -amp }, 0.0]
            })
            .collect();
        let base = Curve::Spline { knots, points };
        // the budget is the cheapest single removal of the base curve, and a hundredth over
        let cheapest = (1..m - 3)
            .filter_map(|i| remove_knot(&base, i as f64, f64::MAX).ok().map(|(_, b)| b))
            .fold(f64::MAX, f64::min);
        let budget = cheapest * 1.01;
        assert!(reduce_to_points(&base, m - 1, budget, &[]).is_ok());
        let mut padded = base.clone();
        for i in 0..1023 {
            padded = insert_knot(&padded, f64::from(i) + 0.5).unwrap();
        }
        assert_eq!(count_of(&padded), m + 1023);
        let (reduced, bound) = reduce_to_points(&padded, m - 1, budget, &[]).unwrap();
        assert_eq!(count_of(&reduced), m - 1);
        assert!(bound <= budget, "bound {bound} budget {budget}");
    }

    /// The work grows a little faster than the number of points, not as its square: a trace of
    /// 50401 points (16800 pieces) comes down to the record's cap well inside a few seconds.
    #[test]
    fn a_very_long_chain_comes_down_in_time() {
        let pieces = if cfg!(debug_assertions) { 2801 } else { 16801 };
        let c = chain(pieces, [0.0, 1.0], helix, dhelix);
        let budget = 2e-3;
        let started = std::time::Instant::now();
        let (reduced, bound) = reduce_to_points(&c, 4096, budget, &[0.0, 1.0]).unwrap();
        let took = started.elapsed();
        assert!(count_of(&reduced) <= 4096 && bound <= budget);
        assert!(gap(&c, &reduced, [0.0, 1.0]) <= bound);
        // a quadratic reduction took 3 minutes here; this has 10 times the room of what it needs
        if !cfg!(debug_assertions) {
            assert!(took.as_secs_f64() < 5.0, "{took:?}");
        }
    }

    /// The kernel's own function takes no budget that is not a number, finite and positive, as
    /// an error and not a loop.
    #[test]
    fn the_kernels_reduction_refuses_a_budget_that_is_not_a_usable_number() {
        let Curve::Spline { knots, points } = chain(301, [0.0, 1.0], helix, dhelix) else {
            panic!("a chain is a spline");
        };
        let nurb = long_nurb_of(&knots, &points).unwrap();
        for budget in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            let r = solidcore::nurbs::crv_reduce::reduce_to_points(nurb.clone(), 600, budget, &[]);
            assert!(r.is_err(), "{budget}");
        }
    }

    #[test]
    fn a_spline_within_the_cap_is_returned_as_it_is() {
        let c = cv_curve(&WAVE, 3).unwrap();
        assert_eq!(reduce_to_points(&c, 4096, 1e-3, &[]), Ok((c, 0.0)));
    }

    /// A budget that is not finite, is zero or negative, or is too small to divide (subnormal) is
    /// refused at once: nothing loops on it, and no number not a number lets a removal through.
    #[test]
    fn a_budget_that_is_not_a_usable_number_is_refused_without_a_loop() {
        let c = chain(301, [0.0, 1.0], helix, dhelix);
        for budget in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            -1e-3,
            1e-322,
            f64::MIN_POSITIVE / 2.0,
        ] {
            assert_eq!(
                reduce_to_points(&c, 600, budget, &[]),
                Err(Error::Argument),
                "{budget}"
            );
        }
        // a cap that cannot be reached ends in the refusal too, with a small usable budget
        let tiny = reduce_to_points(&c, 100, 1e-300, &[]);
        assert!(matches!(tiny, Err(Error::PastTolerance { .. })), "{tiny:?}");
    }

    /// A corner (a joint held as many times as the degree, its sides at an angle) is kept whatever
    /// the budget: the pieces next to it are so short that taking the knot out moves the curve
    /// less than the budget, and the curve would be smooth there.
    #[test]
    fn a_corner_is_kept() {
        for angle in [60.0_f64, 90.0, 120.0] {
            let turn = angle.to_radians();
            // two straight arms of 100 cubic pieces each, meeting at (0.5, 0, 0) at 0.5
            let end = [0.5 + 0.5 * turn.cos(), 0.5 * turn.sin(), 0.0];
            let (mut knots, mut points) = (vec![0.0; 4], vec![[0.0; 3]]);
            let corner = [0.5, 0.0, 0.0];
            for (from, to, lo) in [([0.0; 3], corner, 0.0), (corner, end, 0.5)] {
                for k in 0..100 {
                    let at = |x: f64| [0, 1, 2].map(|c| from[c] + (to[c] - from[c]) * x);
                    let x = |q: f64| (f64::from(k) + q) / 100.0;
                    points.extend([at(x(1.0 / 3.0)), at(x(2.0 / 3.0)), at(x(1.0))]);
                    if k < 99 || lo == 0.0 {
                        knots.extend([lo + 0.5 * x(1.0); 3]);
                    }
                }
            }
            knots.extend([1.0; 4]);
            let c = Curve::Spline { knots, points };
            assert_eq!(count_of(&c), 601);
            let (reduced, bound) = reduce_to_points(&c, 400, 1e-2, &[]).unwrap();
            let Curve::Spline { knots, .. } = &reduced else {
                panic!("a spline")
            };
            assert_eq!(multiplicity(knots, 0.5), 3, "{angle} degrees: {bound}");
            assert!(count_of(&reduced) <= 400);
        }
    }

    /// A curve whose knot domain is wider than the part of it an edge uses: the points at the
    /// edge's two ends (`keep`) stay where they were, the reduction taking the knots out that
    /// leave them be.
    #[test]
    fn the_points_to_keep_do_not_move() {
        let (f, df) = (
            |t: f64| helix((t + 0.2) / 1.4),
            |t: f64| dhelix((t + 0.2) / 1.4).map(|x| x / 1.4),
        );
        let c = chain(1401, [-0.2, 1.2], f, df);
        let budget = 4e-3;
        let (reduced, bound) = reduce_to_points(&c, 4096, budget, &[0.0, 1.0]).unwrap();
        assert!(count_of(&reduced) <= 4096 && bound <= budget);
        for t in [-0.2, 0.0, 1.0, 1.2] {
            let moved = norm(sub(long_at(&c, t), long_at(&reduced, t)));
            assert!(moved <= 1e-9 * budget + 1e-12, "at {t}: {moved}");
        }
        assert!(gap(&c, &reduced, [-0.2, 1.2]) <= bound);
    }

    /// A rational spline over the cap is refused, as the kernel certifies no bound for it; so is
    /// a cap its degree cannot hold, one the record does not hold, and a spline not clamped.
    #[test]
    fn what_cannot_be_reduced_is_refused() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [10.0, 0.0, 0.0, 1.0],
                [10.0 * w, 10.0 * w, 0.0, w],
                [0.0, 10.0, 0.0, 1.0],
            ],
        };
        assert_eq!(reduce_to_points(&arc, 2, 1.0, &[]), Err(Error::Argument));
        let c = chain(1401, [0.0, 1.0], helix, dhelix);
        assert_eq!(reduce_to_points(&c, 3, 1.0, &[]), Err(Error::Argument));
        assert_eq!(reduce_to_points(&c, 4097, 1.0, &[]), Err(Error::Argument));
        assert_eq!(
            reduce_to_points(&c, 4096, 1.0, &[f64::NAN]),
            Err(Error::Argument)
        );
        let Curve::Spline { knots, mut points } = c else {
            panic!("a spline")
        };
        let mut open = knots.clone();
        open[0] = -1.0;
        let open = Curve::Spline {
            knots: open,
            points: points.clone(),
        };
        assert_eq!(
            reduce_to_points(&open, 4096, 1.0, &[]),
            Err(Error::Argument)
        );
        points[7][0] = f64::NAN;
        let nan = Curve::Spline { knots, points };
        assert_eq!(reduce_to_points(&nan, 4096, 1.0, &[]), Err(Error::Argument));
    }

    /// The cap is not reached within a budget too small for it: refused with the certified distance.
    #[test]
    fn a_budget_too_small_for_the_cap_is_refused_with_its_bound() {
        let c = chain(301, [0.0, 1.0], helix, dhelix);
        let budget = 1e-12;
        match reduce_to_points(&c, 100, budget, &[]) {
            Err(Error::PastTolerance { bound }) => assert!(bound <= budget, "{bound}"),
            other => panic!("{other:?}"),
        }
    }
}
