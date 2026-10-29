//! NURBS curves (DXF SPLINE): evaluation, curves through fit points,
//! splitting, and the circular-arc approximation the rest of Oxidraft works
//! with (snaps, TRIM, OFFSET, hatching), computed once per shape.

use std::sync::Arc;

use crate::{
    Aabb, Point,
    curve::{ArcSeg, Seg, fit_arcs},
};

/// Pieces per knot span the approximation starts from; each is split
/// further until its arc is within 1e-7 of the curve's size.
const PIECES_PER_SPAN: usize = 4;

/// A control point in homogeneous form: (x·w, y·w, w).
#[derive(Debug, Clone, Copy)]
struct H {
    x: f64,
    y: f64,
    w: f64,
}

impl H {
    fn of(p: Point, w: f64) -> H {
        H { x: p.x * w, y: p.y * w, w }
    }

    fn point(self) -> Point {
        Point::new(self.x / self.w, self.y / self.w)
    }

    fn lerp(self, o: H, t: f64) -> H {
        H { x: self.x + (o.x - self.x) * t, y: self.y + (o.y - self.y) * t, w: self.w + (o.w - self.w) * t }
    }
}

/// A NURBS curve of `degree` with its control points, knots and weights.
/// Splines drawn through fit points keep them too: grips edit them, and the
/// curve is fitted again.
#[derive(Clone)]
pub struct Spline {
    degree: usize,
    /// Shared, so copies of drawings stay cheap.
    def: Arc<Definition>,
    closed: bool,
    /// The approximation, and the curve parameter at each piece boundary.
    segs: Arc<[Seg]>,
    params: Arc<[f64]>,
}

#[derive(Clone, PartialEq)]
struct Definition {
    control: Vec<Point>,
    knots: Vec<f64>,
    /// Empty when every weight is 1.
    weights: Vec<f64>,
    fit: Vec<Point>,
}

impl PartialEq for Spline {
    fn eq(&self, o: &Spline) -> bool {
        (self.degree, &self.def, self.closed) == (o.degree, &o.def, o.closed)
    }
}

impl std::fmt::Debug for Spline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spline")
            .field("degree", &self.degree)
            .field("control", &self.def.control)
            .field("knots", &self.def.knots)
            .field("weights", &self.def.weights)
            .field("fit", &self.def.fit)
            .field("closed", &self.closed)
            .finish()
    }
}

/// Index `k` of the knot span `[knots[k], knots[k + 1])` holding `u`, among
/// the spans of the curve's domain (`p..count`), skipping empty ones.
fn find_span(knots: &[f64], p: usize, count: usize, u: f64) -> usize {
    let mut k = knots.partition_point(|&x| x <= u).saturating_sub(1).clamp(p, count - 1);
    while k > p && knots[k] >= knots[k + 1] {
        k -= 1;
    }
    k
}

/// The `p + 1` basis functions that are nonzero on span `k`, at `u`.
fn basis(knots: &[f64], k: usize, u: f64, p: usize) -> Vec<f64> {
    let mut n = vec![0.0; p + 1];
    let (mut left, mut right) = (vec![0.0; p + 1], vec![0.0; p + 1]);
    n[0] = 1.0;
    for j in 1..=p {
        left[j] = u - knots[k + 1 - j];
        right[j] = knots[k + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            let temp = n[r] / (right[r + 1] + left[j - r]);
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }
    n
}

/// Solves `a · x = b` for the columns of `b` (Gaussian elimination with
/// partial pivoting). `None` if `a` is singular.
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<[f64; 2]>) -> Option<Vec<[f64; 2]>> {
    let n = a.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() < 1e-14 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let (top, rest) = a.split_at_mut(col + 1);
        let pivot_row = &top[col];
        for (i, row) in rest.iter_mut().enumerate() {
            let f = row[col] / pivot_row[col];
            if f != 0.0 {
                for (x, p) in row[col..].iter_mut().zip(&pivot_row[col..]) {
                    *x -= f * p;
                }
                let r = col + 1 + i;
                b[r] = [b[r][0] - f * b[col][0], b[r][1] - f * b[col][1]];
            }
        }
    }
    let mut x = vec![[0.0; 2]; n];
    for row in (0..n).rev() {
        let mut s = b[row];
        for c in row + 1..n {
            s = [s[0] - a[row][c] * x[c][0], s[1] - a[row][c] * x[c][1]];
        }
        x[row] = [s[0] / a[row][row], s[1] / a[row][row]];
    }
    Some(x)
}

/// The cubic (or lower, for few points) curve through `points`, with
/// chord-length parameters and averaged knots, and each point's parameter.
fn interpolate(points: &[Point]) -> Option<(Spline, Vec<f64>)> {
    let n = points.len().checked_sub(1).filter(|&n| n >= 1)?;
    let p = n.min(3);
    let chords: Vec<f64> = points.windows(2).map(|w| w[0].distance(w[1])).collect();
    let total: f64 = chords.iter().sum();
    if total <= 1e-12 {
        return None;
    }
    let mut params = vec![0.0];
    for c in &chords {
        params.push(params[params.len() - 1] + c / total);
    }
    params[n] = 1.0;
    let mut knots = vec![0.0; p + 1];
    for j in 1..=n - p {
        knots.push(params[j..j + p].iter().sum::<f64>() / p as f64);
    }
    knots.extend(std::iter::repeat_n(1.0, p + 1));
    let count = n + 1;
    let rows: Vec<Vec<f64>> = params
        .iter()
        .map(|&u| {
            let k = find_span(&knots, p, count, u);
            let mut row = vec![0.0; count];
            for (j, v) in basis(&knots, k, u, p).into_iter().enumerate() {
                row[k - p + j] = v;
            }
            row
        })
        .collect();
    let control = solve(rows, points.iter().map(|q| [q.x, q.y]).collect())?;
    let control = control.into_iter().map(|[x, y]| Point::new(x, y)).collect();
    Some((Spline::new(p, control, knots, Vec::new(), false)?, params))
}

/// Consecutive repeats removed.
fn distinct(points: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for &p in points {
        if out.last().is_none_or(|q| !q.approx_eq(p)) {
            out.push(p);
        }
    }
    out
}

impl Spline {
    /// A spline from its definition, or `None` if the counts don't match
    /// (knots = control points + degree + 1) or the knots aren't sorted.
    pub fn new(degree: usize, control: Vec<Point>, knots: Vec<f64>, weights: Vec<f64>, closed: bool) -> Option<Spline> {
        let n = control.len();
        let valid = degree >= 1
            && n > degree
            && knots.len() == n + degree + 1
            && (weights.is_empty() || weights.len() == n)
            && knots.windows(2).all(|w| w[0] <= w[1])
            && knots[degree] < knots[n]
            && weights.iter().all(|w| *w > 0.0)
            && control.iter().all(|p| p.x.is_finite() && p.y.is_finite());
        if !valid {
            return None;
        }
        let weights = if weights.iter().all(|w| (w - 1.0).abs() < 1e-12) { Vec::new() } else { weights };
        let mut s = Spline {
            degree,
            def: Arc::new(Definition { control, knots, weights, fit: Vec::new() }),
            closed,
            segs: Arc::from(Vec::new()),
            params: Arc::from(Vec::new()),
        };
        s.approximate();
        Some(s)
    }

    /// The same curve, marked closed or open (a closed one must end where
    /// it starts).
    pub fn with_closed(mut self, closed: bool) -> Spline {
        self.closed = closed;
        self
    }

    /// The same curve run the other way (REVERSE).
    pub fn reversed(&self) -> Option<Spline> {
        let d = &self.def;
        let (a, b) = (d.knots[0], d.knots[d.knots.len() - 1]);
        let knots = d.knots.iter().rev().map(|k| a + b - k).collect();
        let control = d.control.iter().rev().copied().collect();
        let weights = d.weights.iter().rev().copied().collect();
        let fit = d.fit.iter().rev().copied().collect();
        Some(Spline::new(self.degree, control, knots, weights, self.closed)?.with_fit(fit))
    }

    /// The same curve, remembering the fit points it was drawn through.
    pub fn with_fit(mut self, fit: Vec<Point>) -> Spline {
        Arc::make_mut(&mut self.def).fit = fit;
        self
    }

    /// SPLINE's CV method: a uniform spline (cubic, or lower for fewer than
    /// four points) with these control vertices. Closed ones are periodic.
    pub fn from_control(points: &[Point], closed: bool) -> Option<Spline> {
        let points = distinct(points);
        let n = points.len();
        if n < if closed { 3 } else { 2 } {
            return None;
        }
        Spline::uniform((n - 1).min(3), &points, &[], closed)
    }

    /// A curve of `degree` (lower if there are too few points) on `control`
    /// with evenly spaced knots: clamped to its end points, or periodic if
    /// closed. `weights` is empty when every weight is 1.
    pub fn uniform(degree: usize, control: &[Point], weights: &[f64], closed: bool) -> Option<Spline> {
        let n = control.len();
        let p = degree.min(n.checked_sub(1)?);
        if closed {
            let control: Vec<Point> = control.iter().chain(&control[..p]).copied().collect();
            let weights: Vec<f64> =
                if weights.is_empty() { Vec::new() } else { weights.iter().chain(&weights[..p]).copied().collect() };
            let knots = (0..control.len() + p + 1).map(|i| i as f64).collect();
            return Spline::new(p, control, knots, weights, true);
        }
        let mut knots = vec![0.0; p + 1];
        knots.extend((1..n - p).map(|i| i as f64));
        knots.extend(std::iter::repeat_n((n - p) as f64, p + 1));
        Spline::new(p, control.to_vec(), knots, weights.to_vec(), false)
    }

    /// A straight line as a curve of degree 1.
    pub fn line(a: Point, b: Point) -> Option<Spline> {
        Spline::new(1, vec![a, b], vec![0.0, 0.0, 1.0, 1.0], Vec::new(), false)
    }

    /// A circular arc exactly: a rational quadratic piece for each quarter
    /// turn or less.
    pub fn arc(arc: &ArcSeg) -> Option<Spline> {
        let pieces = (arc.sweep.abs() / 90.0 - 1e-9).ceil().max(1.0) as usize;
        let w = (arc.sweep / pieces as f64 / 2.0).to_radians().cos();
        let (mut control, mut weights, mut knots) = (vec![arc.point_at(0.0)], vec![1.0], vec![0.0; 3]);
        for i in 0..pieces {
            let mid = arc.start + arc.sweep * (i as f64 + 0.5) / pieces as f64;
            control.extend([arc.center.polar(arc.radius / w, mid), arc.point_at((i + 1) as f64 / pieces as f64)]);
            weights.extend([w, 1.0]);
            knots.extend(std::iter::repeat_n((i + 1) as f64, if i + 1 < pieces { 2 } else { 3 }));
        }
        Spline::new(2, control, knots, weights, false)
    }

    /// The same curve one degree higher (SPLINEDIT's Elevate order): each
    /// span made a Bézier piece and raised, so the shape doesn't change.
    pub fn elevated(&self) -> Option<Spline> {
        let p = self.degree;
        let (a, b) = self.domain();
        let clamped = self.sub(a, b)?;
        let mut knots = clamped.def.knots.clone();
        let mut pts = clamped.homogeneous();
        let mut interior: Vec<f64> = knots.iter().copied().filter(|&k| k > a && k < b).collect();
        interior.dedup();
        for &u in &interior {
            for _ in knots.iter().filter(|&&k| k == u).count()..p {
                Self::insert(p, &mut knots, &mut pts, u);
            }
        }
        if pts.len() != (interior.len() + 1) * p + 1 {
            return None;
        }
        let mut raised = vec![pts[0]];
        for piece in pts.windows(p + 1).step_by(p) {
            for k in 1..=p {
                raised.push(piece[k].lerp(piece[k - 1], k as f64 / (p + 1) as f64));
            }
            raised.push(piece[p]);
        }
        let mut knots = vec![a; p + 2];
        for &u in &interior {
            knots.extend(std::iter::repeat_n(u, p + 1));
        }
        knots.extend(std::iter::repeat_n(b, p + 2));
        Some(Self::from_homogeneous(p + 1, &raised, knots)?.with_closed(self.closed))
    }

    /// The same curve raised to `degree` (no lower than it is).
    pub fn with_degree(&self, degree: usize) -> Option<Spline> {
        let mut s = self.clone();
        while s.degree < degree {
            s = s.elevated()?;
        }
        Some(s)
    }

    /// The control vertices editing works on (a periodic curve's repeats
    /// left out), and their weights.
    pub fn control_vertices(&self) -> (Vec<Point>, Vec<f64>) {
        let n = self.distinct_control();
        (self.def.control[..n].to_vec(), self.weights()[..n].to_vec())
    }

    /// The curve on changed control vertices and weights (as
    /// [`Spline::control_vertices`] gives them), with the same knots. Fit
    /// points go: the curve no longer passes through them.
    pub fn with_control(&self, mut control: Vec<Point>, mut weights: Vec<f64>) -> Option<Spline> {
        if self.is_periodic() {
            control.extend_from_within(..self.degree);
            weights.extend_from_within(..self.degree);
        }
        Spline::new(self.degree, control, self.def.knots.clone(), weights, self.closed)
    }

    /// The parameter where the curve passes closest to `p`.
    pub fn closest_to(&self, p: Point) -> f64 {
        let (a, b) = self.domain();
        let samples = 64 * self.def.control.len();
        let rough = (0..=samples)
            .map(|i| a + (b - a) * i as f64 / samples as f64)
            .min_by(|u, v| self.point_at(*u).distance(p).total_cmp(&self.point_at(*v).distance(p)))
            .unwrap_or(a);
        self.closest_param(p, rough)
    }

    /// One more control vertex where the curve passes closest to `near`,
    /// keeping its shape (a knot inserted there).
    pub fn with_control_added(&self, near: Point) -> Option<Spline> {
        let (a, b) = self.domain();
        let base = if self.is_periodic() { self.sub(a, b)?.with_closed(true) } else { self.clone() };
        let u = base.closest_to(near);
        if u <= a || u >= b {
            return None;
        }
        let (mut knots, mut pts) = (base.def.knots.clone(), base.homogeneous());
        if knots.iter().filter(|&&k| k == u).count() >= self.degree {
            return None;
        }
        Self::insert(self.degree, &mut knots, &mut pts, u);
        Some(Self::from_homogeneous(self.degree, &pts, knots)?.with_closed(self.closed))
    }

    /// The curve without control vertex `i`: a knot near it goes too (a
    /// closed periodic curve gets even knots again), lowering the degree if
    /// there are too few vertices left for it.
    pub fn with_control_deleted(&self, i: usize) -> Option<Spline> {
        let (mut control, mut weights) = self.control_vertices();
        let n = control.len();
        if i >= n || n <= 2 {
            return None;
        }
        control.remove(i);
        weights.remove(i);
        let p = self.degree;
        let weights = if self.is_rational() { weights } else { Vec::new() };
        if self.is_periodic() || n - 1 <= p {
            return Spline::uniform(p, &control, &weights, self.closed);
        }
        let mut knots = self.def.knots.clone();
        knots.remove((i + p.div_ceil(2)).clamp(p + 1, n - 1));
        Spline::new(p, control, knots, weights, self.closed)
    }

    /// SPLINE's Fit method: a curve through `points`. A closed one goes
    /// round smoothly through the first point again.
    pub fn through(points: &[Point], closed: bool) -> Option<Spline> {
        let mut pts = distinct(points);
        if closed && pts.len() > 1 && pts[0].approx_eq(pts[pts.len() - 1]) {
            pts.pop();
        }
        if !closed {
            return Some(interpolate(&pts)?.0.with_fit(pts));
        }
        let n = pts.len();
        if n < 3 {
            return None;
        }
        // Fit an open curve through the points with a few more on either
        // side, then keep the stretch from the first point round to itself.
        let k = n.min(3);
        let wrapped: Vec<Point> = pts[n - k..]
            .iter()
            .chain(&pts)
            .chain(std::iter::once(&pts[0]))
            .chain(&pts[1..=k.min(n - 1)])
            .copied()
            .collect();
        let (open, params) = interpolate(&wrapped)?;
        let mut s = open.sub(params[k], params[k + n])?;
        s.closed = true;
        Some(s.with_fit(pts))
    }

    pub fn degree(&self) -> usize {
        self.degree
    }

    pub fn control(&self) -> &[Point] {
        &self.def.control
    }

    pub fn knots(&self) -> &[f64] {
        &self.def.knots
    }

    /// Each control point's weight (all 1 unless rational).
    pub fn weights(&self) -> Vec<f64> {
        (0..self.def.control.len()).map(|i| self.weight(i)).collect()
    }

    pub fn is_rational(&self) -> bool {
        !self.def.weights.is_empty()
    }

    pub fn fit(&self) -> &[Point] {
        &self.def.fit
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// True for a closed curve whose knots run on past its ends (DXF's
    /// periodic flag), as SPLINE's closed CV curves are.
    pub fn is_periodic(&self) -> bool {
        self.closed && self.def.knots[0] < self.def.knots[self.degree]
    }

    fn weight(&self, i: usize) -> f64 {
        self.def.weights.get(i).copied().unwrap_or(1.0)
    }

    fn homogeneous(&self) -> Vec<H> {
        self.def.control.iter().enumerate().map(|(i, p)| H::of(*p, self.weight(i))).collect()
    }

    fn from_homogeneous(degree: usize, pts: &[H], knots: Vec<f64>) -> Option<Spline> {
        let control = pts.iter().map(|h| h.point()).collect();
        Spline::new(degree, control, knots, pts.iter().map(|h| h.w).collect(), false)
    }

    /// The range of parameters the curve covers.
    pub fn domain(&self) -> (f64, f64) {
        (self.def.knots[self.degree], self.def.knots[self.def.control.len()])
    }

    /// The point at parameter `u` (de Boor's algorithm).
    pub fn point_at(&self, u: f64) -> Point {
        let p = self.degree;
        let k = find_span(&self.def.knots, p, self.def.control.len(), u);
        let mut d: Vec<H> = (0..=p).map(|j| H::of(self.def.control[j + k - p], self.weight(j + k - p))).collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = j + k - p;
                let denom = self.def.knots[i + p + 1 - r] - self.def.knots[i];
                let alpha = if denom > 0.0 { (u - self.def.knots[i]) / denom } else { 0.0 };
                d[j] = d[j - 1].lerp(d[j], alpha);
            }
        }
        d[p].point()
    }

    pub fn start_point(&self) -> Point {
        self.point_at(self.domain().0)
    }

    pub fn end_point(&self) -> Point {
        self.point_at(self.domain().1)
    }

    fn approximate(&mut self) {
        let (p, n) = (self.degree, self.def.control.len());
        let size = self.def.control.iter().fold(Aabb::from_points(self.def.control[0], self.def.control[0]), |b, q| {
            b.union(Aabb::from_points(*q, *q))
        });
        let tol = (size.width().hypot(size.height()) * 1e-7).max(1e-12);
        let mut pieces = Vec::new();
        for k in p..n {
            let (a, b) = (self.def.knots[k], self.def.knots[k + 1]);
            for j in 0..PIECES_PER_SPAN {
                let step = (b - a) / PIECES_PER_SPAN as f64;
                if step > 0.0 {
                    let t0 = a + step * j as f64;
                    fit_arcs(&|u| self.point_at(u), t0, t0 + step, tol, 8, &mut pieces);
                }
            }
        }
        let mut params: Vec<f64> = pieces.iter().map(|(u, _)| *u).collect();
        params.push(self.def.knots[n]);
        self.segs = Arc::from(pieces.into_iter().map(|(_, s)| s).collect::<Vec<_>>());
        self.params = Arc::from(params);
    }

    /// The curve as circular arcs, each within 1e-7 of the curve's size
    /// of it.
    pub fn segs(&self) -> &[Seg] {
        &self.segs
    }

    /// The curve parameter at `s` along the approximation (piece `i` covers
    /// `[i, i + 1]`), refined onto the curve itself.
    pub fn param_at(&self, s: f64) -> f64 {
        let last = self.segs.len().saturating_sub(1);
        let i = (s.max(0.0).floor() as usize).min(last);
        let rough = self.params[i] + (self.params[i + 1] - self.params[i]) * (s - i as f64).clamp(0.0, 1.0);
        self.closest_param(self.segs[i].point_at((s - i as f64).clamp(0.0, 1.0)), rough)
    }

    /// The parameter of the curve point closest to `p`, starting near `u`
    /// (a few Newton steps).
    fn closest_param(&self, p: Point, mut u: f64) -> f64 {
        let (a, b) = self.domain();
        let h = (b - a) * 1e-7;
        for _ in 0..5 {
            let c = self.point_at(u);
            let d = (self.point_at((u + h).min(b)) - self.point_at((u - h).max(a))) * (1.0 / (2.0 * h));
            let len2 = d.dot(d);
            if len2 <= 1e-300 {
                break;
            }
            u = (u - (c - p).dot(d) / len2).clamp(a, b);
        }
        u
    }

    /// Inserts knot `u` once (Boehm's algorithm), keeping the curve's shape.
    fn insert(degree: usize, knots: &mut Vec<f64>, pts: &mut Vec<H>, u: f64) {
        let p = degree;
        let n = pts.len();
        // At the end of the domain, the last span (a periodic curve's knots run on past it).
        let k = (knots.partition_point(|&x| x <= u) - 1).min(n - 1);
        let mut q = Vec::with_capacity(n + 1);
        q.extend_from_slice(&pts[..=k - p]);
        for i in k - p + 1..=k {
            let alpha = (u - knots[i]) / (knots[i + p] - knots[i]);
            q.push(pts[i - 1].lerp(pts[i], alpha));
        }
        q.extend_from_slice(&pts[k..]);
        knots.insert(k + 1, u);
        *pts = q;
    }

    /// The curve split at parameter `u`: the parts before and after (`None`
    /// where a part is empty).
    pub fn split_at(&self, u: f64) -> (Option<Spline>, Option<Spline>) {
        let p = self.degree;
        let (a, b) = self.domain();
        // Land exactly on a knot that's within rounding of `u`.
        let tol = (b - a) * 1e-10;
        let u = self.def.knots.iter().copied().find(|k| (k - u).abs() <= tol).unwrap_or(u).clamp(a, b);
        let mut knots = self.def.knots.clone();
        let mut pts = self.homogeneous();
        let multiplicity = knots.iter().filter(|&&k| k == u).count();
        for _ in multiplicity..=p {
            Self::insert(p, &mut knots, &mut pts, u);
        }
        let j = knots.iter().position(|&k| k == u).unwrap_or(0);
        let left = (j > p).then(|| Self::from_homogeneous(p, &pts[..j], knots[..=j + p].to_vec())).flatten();
        let right = (pts.len() - j > p).then(|| Self::from_homogeneous(p, &pts[j..], knots[j..].to_vec())).flatten();
        (left, right)
    }

    /// The part of the curve from parameter `u0` to `u1`.
    pub fn sub(&self, u0: f64, u1: f64) -> Option<Spline> {
        if u1 <= u0 {
            return None;
        }
        self.split_at(u0).1?.split_at(u1).0
    }

    /// `self` followed by `next`, which starts where `self` ends; both must
    /// be clamped there (as [`Spline::sub`]'s results are).
    pub fn joined(&self, next: &Spline) -> Option<Spline> {
        let p = self.degree;
        if next.degree != p {
            return None;
        }
        let shift = self.domain().1 - next.domain().0;
        let mut knots = self.def.knots[..self.def.knots.len() - 1].to_vec();
        knots.extend(next.def.knots[p + 1..].iter().map(|k| k + shift));
        // Scaling a curve's weights together doesn't change it: match them where the two meet.
        let scale = self.weight(self.def.control.len() - 1) / next.weight(0);
        let mut pts = self.homogeneous();
        pts.extend(next.homogeneous()[1..].iter().map(|h| H::of(h.point(), h.w * scale)));
        Self::from_homogeneous(p, &pts, knots)
    }

    /// The curve after moving its control and fit points with `f` (exact
    /// for moves, rotations, uniform scaling and mirroring).
    pub fn mapped(&self, f: &dyn Fn(Point) -> Point) -> Spline {
        let def = Definition {
            control: self.def.control.iter().map(|p| f(*p)).collect(),
            fit: self.def.fit.iter().map(|p| f(*p)).collect(),
            ..(*self.def).clone()
        };
        let mut s = Spline { def: Arc::new(def), ..self.clone() };
        s.approximate();
        s
    }

    /// Control points of a periodic curve repeat its first few at the end;
    /// these are the distinct ones.
    fn distinct_control(&self) -> usize {
        if self.is_periodic() { self.def.control.len() - self.degree } else { self.def.control.len() }
    }

    /// The points grips edit: fit points if the curve has them, else its
    /// control vertices.
    pub fn grip_points(&self) -> Vec<Point> {
        if self.def.fit.is_empty() {
            self.def.control[..self.distinct_control()].to_vec()
        } else {
            self.def.fit.clone()
        }
    }

    /// The curve with grip point `i` moved to `to`.
    pub fn with_grip_moved(&self, i: usize, to: Point) -> Option<Spline> {
        let mut points = self.grip_points();
        *points.get_mut(i)? = to;
        self.with_grip_points(&points)
    }

    /// The curve rebuilt from changed grip points (STRETCH).
    pub fn with_grip_points(&self, points: &[Point]) -> Option<Spline> {
        if !self.def.fit.is_empty() {
            return Spline::through(points, self.closed);
        }
        let mut control = points.to_vec();
        if self.is_periodic() {
            control.extend_from_within(..self.degree);
        }
        Spline::new(self.degree, control, self.def.knots.clone(), self.def.weights.clone(), self.closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(c: &[(f64, f64)]) -> Vec<Point> {
        c.iter().map(|&(x, y)| Point::new(x, y)).collect()
    }

    #[test]
    fn fit_curves_pass_through_their_points() {
        let fit = pts(&[(0.0, 0.0), (10.0, 5.0), (20.0, 0.0), (30.0, 8.0), (35.0, 2.0)]);
        let s = Spline::through(&fit, false).unwrap();
        assert_eq!(s.degree(), 3);
        // Every fit point lies on the curve, and on its arc approximation.
        for q in &fit {
            let on = s.segs().iter().map(|seg| seg.distance_to(*q)).fold(f64::INFINITY, f64::min);
            assert!(on < 1e-5, "{q:?} off by {on}");
        }
        assert!(s.start_point().approx_eq(fit[0]) && s.end_point().approx_eq(fit[4]));
        // Two points make a straight line.
        let line = Spline::through(&pts(&[(0.0, 0.0), (4.0, 3.0)]), false).unwrap();
        assert!(line.point_at(0.5).approx_eq(Point::new(2.0, 1.5)));
    }

    #[test]
    fn closed_fit_curves_come_back_round() {
        let fit = pts(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        let s = Spline::through(&fit, true).unwrap();
        assert!(s.is_closed() && s.start_point().approx_eq(s.end_point()));
        assert!(s.start_point().approx_eq(fit[0]));
        for q in &fit {
            let on = s.segs().iter().map(|seg| seg.distance_to(*q)).fold(f64::INFINITY, f64::min);
            assert!(on < 1e-5);
        }
        // Smooth where it closes: the tangents at both ends agree.
        let first = s.segs()[0].tangent_at(0.0);
        let last = s.segs()[s.segs().len() - 1].tangent_at(1.0);
        assert!(first.dot(last) > 0.99, "{first:?} vs {last:?}");
    }

    #[test]
    fn control_vertex_curves_start_and_end_on_their_ends() {
        let cv = pts(&[(0.0, 0.0), (5.0, 10.0), (10.0, -10.0), (15.0, 0.0), (20.0, 5.0)]);
        let s = Spline::from_control(&cv, false).unwrap();
        assert!(s.start_point().approx_eq(cv[0]) && s.end_point().approx_eq(cv[4]));
        let closed = Spline::from_control(&cv, true).unwrap();
        assert!(closed.is_periodic() && closed.start_point().approx_eq(closed.end_point()));
        assert_eq!(closed.grip_points().len(), 5);
    }

    #[test]
    fn splitting_keeps_the_shape() {
        let fit = pts(&[(0.0, 0.0), (10.0, 5.0), (20.0, 0.0), (30.0, 8.0)]);
        let s = Spline::through(&fit, false).unwrap();
        let (a, b) = s.domain();
        let u = a + (b - a) * 0.37;
        let (left, right) = s.split_at(u);
        let (left, right) = (left.unwrap(), right.unwrap());
        assert!(left.end_point().approx_eq(s.point_at(u)) && right.start_point().approx_eq(s.point_at(u)));
        for t in [0.1, 0.5, 0.9] {
            let (la, lb) = left.domain();
            let lu = la + (lb - la) * t;
            assert!(left.point_at(lu).approx_eq(s.point_at(lu)));
        }
        let middle = s.sub(a + (b - a) * 0.2, a + (b - a) * 0.6).unwrap();
        assert!(middle.start_point().approx_eq(s.point_at(a + (b - a) * 0.2)));
        let back = left.joined(&right).unwrap();
        for t in [0.05, 0.37, 0.8] {
            let (ja, jb) = back.domain();
            assert!(back.point_at(ja + (jb - ja) * t).approx_eq(s.point_at(a + (b - a) * t)));
        }
        // Splitting a periodic curve works the same.
        let closed = Spline::from_control(&fit, true).unwrap();
        let (ca, cb) = closed.domain();
        let piece = closed.sub(ca + 0.5, cb - 0.5).unwrap();
        assert!(piece.start_point().approx_eq(closed.point_at(ca + 0.5)));
        assert!(piece.end_point().approx_eq(closed.point_at(cb - 0.5)));
    }

    /// True if `b` traces `a`'s path (sampled along both).
    fn same_path(a: &Spline, b: &Spline) -> bool {
        let on = |s: &Spline, q: Point| s.point_at(s.closest_to(q)).distance(q) < 1e-6;
        let along = |s: &Spline| {
            let (u0, u1) = s.domain();
            (0..=40).map(move |i| s.point_at(u0 + (u1 - u0) * i as f64 / 40.0)).collect::<Vec<_>>()
        };
        along(a).into_iter().all(|q| on(b, q)) && along(b).into_iter().all(|q| on(a, q))
    }

    #[test]
    fn elevating_and_adding_vertices_keep_the_shape() {
        let fit = pts(&[(0.0, 0.0), (10.0, 5.0), (20.0, 0.0), (30.0, 8.0), (35.0, 2.0)]);
        let s = Spline::through(&fit, false).unwrap();
        let up = s.elevated().unwrap();
        assert_eq!(up.degree(), 4);
        assert!(same_path(&s, &up));
        assert!(up.start_point().approx_eq(fit[0]) && up.end_point().approx_eq(fit[4]));
        // A closed periodic curve too, and rational ones.
        let closed = Spline::from_control(&fit, true).unwrap();
        let up = closed.with_degree(5).unwrap();
        assert!(up.degree() == 5 && up.is_closed() && same_path(&closed, &up));
        let quarter = Spline::arc(&ArcSeg { center: Point::ORIGIN, radius: 2.0, start: 30.0, sweep: 250.0 }).unwrap();
        assert!((quarter.point_at(1.234).length() - 2.0).abs() < 1e-12, "arcs are exact");
        assert!(quarter.start_point().approx_eq(Point::ORIGIN.polar(2.0, 30.0)));
        assert!(quarter.end_point().approx_eq(Point::ORIGIN.polar(2.0, 280.0)));
        let up = quarter.elevated().unwrap();
        assert!((up.point_at(1.7).length() - 2.0).abs() < 1e-9);
        // A control vertex added where the curve passes keeps its shape.
        let more = s.with_control_added(s.point_at(0.4 * s.domain().1)).unwrap();
        assert_eq!(more.control().len(), s.control().len() + 1);
        assert!(same_path(&s, &more));
        let more = closed.with_control_added(closed.point_at(closed.domain().0 + 1.3)).unwrap();
        assert!(more.is_closed() && same_path(&closed, &more));
        // Deleting one makes it pass elsewhere, with one fewer.
        let cv = Spline::from_control(&fit, false).unwrap();
        let fewer = cv.with_control_deleted(2).unwrap();
        assert_eq!(fewer.control().len(), 4);
        assert!(fewer.start_point().approx_eq(fit[0]) && fewer.end_point().approx_eq(fit[4]));
        assert_eq!(cv.with_control_deleted(1).unwrap().with_control_deleted(1).unwrap().degree(), 2);
    }

    #[test]
    fn rational_curves_draw_exact_circles() {
        // A quarter circle as a rational quadratic.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let s = Spline::new(
            2,
            pts(&[(1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![1.0, w, 1.0],
            false,
        )
        .unwrap();
        for t in [0.1, 0.5, 0.77] {
            assert!((s.point_at(t).length() - 1.0).abs() < 1e-12);
        }
        let (l, r) = s.split_at(0.5);
        assert!((l.unwrap().point_at(0.25).length() - 1.0).abs() < 1e-12);
        assert!((r.unwrap().point_at(0.9).length() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn approximation_parameters_land_on_the_curve() {
        let s = Spline::through(&pts(&[(0.0, 0.0), (10.0, 5.0), (20.0, 0.0), (30.0, 8.0)]), false).unwrap();
        let s_mid = s.segs().len() as f64 / 3.0 + 0.4;
        let i = s_mid.floor() as usize;
        let on_arc = s.segs()[i].point_at(s_mid - i as f64);
        assert!(s.point_at(s.param_at(s_mid)).distance(on_arc) < 1e-5);
        assert!(s.segs().len() < 200, "{} arcs", s.segs().len());
    }
}
