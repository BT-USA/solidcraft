//! A chamfer's offsets on faces that bend in the section: the offsets `r_1`, `r_2`
//! whose point has its feet at the asked setbacks measured along the faces' own curves.
//!
//! [`ChamferDefinition::offsets`](crate::surface::procedural_blend::chamfer::ChamferDefinition)
//! reads the sizes on two straight sections, so a face that bends in the plane square to the
//! edge (a sphere meeting a cylinder, a cylinder meeting a plane along its axis) puts the
//! feet of those offsets at the wrong distances. Here the section is exact: each support is
//! the circle of its normal curvature at the edge (the sphere's and the cylinder's section
//! exactly, any other face's to second order), a setback is the arc length along it from the
//! edge, and an angle is the angle between the chamfer's line and the tangent of the face the
//! distance is on at that distance's foot. The feet are `F_i = C_i(d_i)` with the offsets'
//! point `P = F_i + r_i m_i(d_i)`, `m_i` the circle's normal towards its centre; two
//! equations for two offsets, solved exactly.
//!
//! Not upstream (solidcore's). Deviations: none. No recursion.

use crate::error::{Result, SolidError};
use crate::surface::procedural_blend::chamfer::ChamferSize;

const OP: &str = "blend::chamfer_section";

/// A support's curvature in the section below which the face counts as straight there.
pub(crate) const STRAIGHT: f64 = 1e-12;

type V = [f64; 2];

fn invalid(why: &'static str) -> SolidError {
    SolidError::invalid_input(OP, why)
}

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross(a: V, b: V) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1]]
}

fn scale(a: V, s: f64) -> V {
    [a[0] * s, a[1] * s]
}

/// One support in the section plane: the edge at the origin, `t` the unit direction into the
/// face, `m` the unit direction towards the other face, `kappa` how far the face bends
/// towards `m` (1/length, negative away from it).
#[derive(Clone, Copy)]
struct Support {
    t: V,
    m: V,
    kappa: f64,
}

impl Support {
    /// The point at arc length `s` along the face from the edge.
    fn point(&self, s: f64) -> V {
        let k = self.kappa;
        let (along, towards) = if k.abs() < STRAIGHT {
            (s, 0.5 * k * s * s)
        } else {
            ((k * s).sin() / k, (1.0 - (k * s).cos()) / k)
        };
        add(scale(self.t, along), scale(self.m, towards))
    }

    /// The face's unit tangent at arc length `s`, pointing away from the edge.
    fn tangent(&self, s: f64) -> V {
        let a = self.kappa * s;
        add(scale(self.t, a.cos()), scale(self.m, a.sin()))
    }

    /// The face's unit normal towards its centre of curvature (towards `m` for a straight
    /// face) at arc length `s`.
    fn inward(&self, s: f64) -> V {
        let a = self.kappa * s;
        add(scale(self.m, a.cos()), scale(self.t, -a.sin()))
    }

    /// The arc length of the point `p` on the face's circle (from the edge), or `None` where
    /// it falls behind the edge.
    fn arc_of(&self, p: V) -> Option<f64> {
        let k = self.kappa;
        let (x, y) = (dot(p, self.t), dot(p, self.m));
        let s = if k.abs() < STRAIGHT {
            x
        } else {
            (k * x).atan2(1.0 - k * y) / k
        };
        (s.is_finite() && s > 0.0).then_some(s)
    }
}

/// The two supports of a wedge `wedge` between them, whose faces bend towards the other face by
/// `kappa`.
fn supports(wedge: f64, kappa: [f64; 2]) -> [Support; 2] {
    let (s, c) = wedge.sin_cos();
    [
        Support {
            t: [1.0, 0.0],
            m: [0.0, 1.0],
            kappa: kappa[0],
        },
        Support {
            t: [c, s],
            m: [s, -c],
            kappa: kappa[1],
        },
    ]
}

/// The offsets of the point that stands off `a` and `b` at the feet `fa`, `fb` (arc lengths).
fn offsets_at(a: &Support, b: &Support, fa: f64, fb: f64) -> Result<[f64; 2]> {
    let (ma, mb) = (a.inward(fa), b.inward(fb));
    // r_a m_a - r_b m_b = F_b - F_a
    let rhs = sub(b.point(fb), a.point(fa));
    let det = cross(ma, scale(mb, -1.0));
    if !(det.is_finite() && det.abs() > 1e-12) {
        return Err(invalid("the chamfer's normals at its feet are parallel"));
    }
    let ra = cross(rhs, scale(mb, -1.0)) / det;
    let rb = cross(ma, rhs) / det;
    if !(ra.is_finite() && rb.is_finite() && ra > 0.0 && rb > 0.0) {
        return Err(invalid(
            "the chamfer's distances fold on the faces' curves: its feet leave the wedge",
        ));
    }
    Ok([ra, rb])
}

/// The arc length along `b` where the ray from `from` along the unit `dir` meets it first.
fn ray_meets(b: &Support, from: V, dir: V) -> Option<f64> {
    let k = b.kappa;
    if k.abs() < STRAIGHT {
        let denom = cross(dir, b.t);
        if denom.abs() < 1e-14 {
            return None;
        }
        let lambda = -cross(from, b.t) / denom;
        return (lambda > 0.0)
            .then(|| b.arc_of(add(from, scale(dir, lambda))))
            .flatten();
    }
    let centre = scale(b.m, 1.0 / k);
    let radius = 1.0 / k.abs();
    let w = sub(from, centre);
    let half = dot(dir, w);
    let disc = half * half - (dot(w, w) - radius * radius);
    if disc.is_nan() || disc < 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let mut hits: Vec<f64> = [-half - root, -half + root]
        .into_iter()
        .filter(|l| *l > 1e-14)
        .collect();
    hits.sort_by(f64::total_cmp);
    hits.into_iter()
        .find_map(|l| b.arc_of(add(from, scale(dir, l))))
}

/// The chord between the feet of equal setbacks `d`.
fn chord_at(a: &Support, b: &Support, d: f64) -> f64 {
    let v = sub(b.point(d), a.point(d));
    dot(v, v).sqrt()
}

/// The offsets of a chamfer of `size` between supports of wedge `wedge` (radians, in
/// `(0, pi)`) that bend towards the other face by `kappa` (1/length, per support), the first
/// size along support 1.
///
/// # Errors
///
/// `InvalidInput` where the sizes do not fit (an angle's line misses the other face, a chord
/// longer than the faces allow) or fold (an offset not positive).
pub(crate) fn offsets(wedge: f64, kappa: [f64; 2], size: ChamferSize) -> Result<[f64; 2]> {
    if !(wedge.is_finite() && wedge > 0.0 && wedge < crate::math::PI) {
        return Err(invalid("the wedge is between 0 and pi"));
    }
    let [a, b] = supports(wedge, kappa);
    let (fa, fb) = match size {
        ChamferSize::Distances([da, db]) => (da, db),
        ChamferSize::DistanceAngle { distance, angle } => {
            let from = a.point(distance);
            let toward_edge = scale(a.tangent(distance), -angle.cos());
            let up = scale(a.inward(distance), angle.sin());
            let dir = add(toward_edge, up);
            let db = ray_meets(&b, from, dir)
                .ok_or_else(|| invalid("the chamfer's line does not meet the other face"))?;
            (distance, db)
        }
        ChamferSize::Chord(c) => {
            // the chord grows from nothing with the equal setbacks: bracket, then bisect
            let mut hi = c / (2.0 * (0.5 * wedge).sin());
            let mut grown = 0;
            while chord_at(&a, &b, hi) < c {
                hi *= 2.0;
                grown += 1;
                if grown > 60 || !hi.is_finite() {
                    return Err(invalid("the chamfer's chord does not fit the faces"));
                }
            }
            let mut lo = 0.0;
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if chord_at(&a, &b, mid) < c {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            (0.5 * (lo + hi), 0.5 * (lo + hi))
        }
    };
    if !(fa.is_finite() && fb.is_finite() && fa > 0.0 && fb > 0.0) {
        return Err(invalid("the chamfer's setbacks are positive"));
    }
    offsets_at(&a, &b, fa, fb)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-12 * (1.0 + b.abs())
    }

    #[test]
    fn straight_faces_give_the_planar_offsets() {
        let w = 0.7 * crate::math::PI;
        let [r1, r2] = offsets(w, [0.0, 0.0], ChamferSize::Distances([1.0, 2.0])).unwrap();
        let (s, c) = w.sin_cos();
        assert!(close(r1, (2.0 - c) / s) && close(r2, (1.0 - 2.0 * c) / s));
        let [q1, q2] = offsets(
            w,
            [0.0, 0.0],
            ChamferSize::DistanceAngle {
                distance: 1.0,
                angle: 0.4,
            },
        )
        .unwrap();
        let d2 = 0.4_f64.sin() / (w + 0.4).sin();
        assert!(close(q1, (d2 - c) / s) && close(q2, (1.0 - d2 * c) / s));
    }

    #[test]
    fn a_sphere_s_feet_are_at_the_asked_arc() {
        // the issue's pin: sphere radius 3 meets a cylinder at 131.81 degrees inside
        let w = 2.3;
        let kappa = [0.0, 1.0 / 3.0];
        let [ra, rb] = offsets(w, kappa, ChamferSize::Distances([0.3, 0.3])).unwrap();
        let [a, b] = supports(w, kappa);
        let pa = add(a.point(0.3), scale(a.inward(0.3), ra));
        let pb = add(b.point(0.3), scale(b.inward(0.3), rb));
        assert!(close(pa[0], pb[0]) && close(pa[1], pb[1]));
    }
}
