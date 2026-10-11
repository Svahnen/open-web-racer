//! Bounded geometric support diagnostics for a local preview reference path.
//!
//! This module reports whether a polyline is covered by the same imported
//! walkable triangles used by the local preview. It does not define a race
//! route, checkpoint, start, finish, or collision contract.

use std::collections::BTreeMap;

use super::{triangle_normal, PreviewCollisionWorld, PreviewGroundTriangle, Vec2};

const GRID_CELL: f64 = 8.0;
const MAX_PATH_ROWS: usize = 100_000;
const MAX_ROUTE_SAMPLES: usize = 500_000;
const MAX_TRIANGLE_REFS: usize = 2_000_000;
const MAX_CANDIDATE_CHECKS: usize = 5_000_000;
const MAX_CELL_TRIANGLES: usize = 10_000;
const MAX_CELLS_PER_TRIANGLE: u64 = 64;

/// One ordered point, in the collision world's simulation XY and scene Z
/// coordinates. Coordinates are transient diagnostic inputs only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReferencePathPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteAuditConfig {
    /// Scene units per simulation XY unit.
    pub units_per_sim_unit: f64,
    /// Maximum distance between probes, measured in scene units.
    pub max_spacing_scene: f64,
}

impl Default for RouteAuditConfig {
    fn default() -> Self {
        Self {
            units_per_sim_unit: 1.0,
            max_spacing_scene: 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteAuditError {
    ReferencePathUnavailable,
    TooFewRows,
    TooManyRows,
    InvalidConfig,
    NonFinitePath,
    InvalidSupport,
    TooManySamples,
    IndexTooLarge,
    WorkLimitExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteSampleDiagnostic {
    /// Segment index in the closed ordered polyline.
    pub segment_index: usize,
    /// Fraction from the segment start, in [0, 1].
    pub segment_fraction: f64,
    pub supported: bool,
    /// Selected support height, when covered.
    pub support_z: Option<f64>,
    /// Number of other containing triangles at a distinct height.
    pub alternate_planes: usize,
    /// Multiple geometrically distinct candidates tied under the selection policy.
    pub selection_ambiguous: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RouteCoverageReport {
    pub sample_count: usize,
    pub supported_count: usize,
    pub unsupported_count: usize,
    pub unsupported_runs: Vec<(usize, usize)>,
    /// The closing segment's endpoint is the first route row.
    pub seam_supported: bool,
    pub seam_support_z: Option<f64>,
    /// True only when the closing probe selects the same plane as the initial probe.
    pub seam_matches_initial_support: bool,
    pub candidate_checks: usize,
    pub samples: Vec<RouteSampleDiagnostic>,
}

#[derive(Clone, Copy)]
struct Candidate {
    z: f64,
    normal: [f64; 3],
}

/// Audit a closed path against the production local-preview ground triangles.
/// Exact projected containment is required; there is no edge-distance fallback.
pub fn audit_closed_reference_path(
    collision: &PreviewCollisionWorld,
    path: &[ReferencePathPoint],
    config: RouteAuditConfig,
) -> Result<RouteCoverageReport, RouteAuditError> {
    if path.len() < 2 {
        return Err(RouteAuditError::TooFewRows);
    }
    if path.len() > MAX_PATH_ROWS {
        return Err(RouteAuditError::TooManyRows);
    }
    if !config.units_per_sim_unit.is_finite()
        || !(0.01..=100.0).contains(&config.units_per_sim_unit)
        || !config.max_spacing_scene.is_finite()
        || config.max_spacing_scene <= 0.0
        || config.max_spacing_scene > 100.0
    {
        return Err(RouteAuditError::InvalidConfig);
    }
    if path.iter().any(|p| {
        !p.x.is_finite()
            || !p.y.is_finite()
            || !p.z.is_finite()
            || p.x.abs() > 1.0e6
            || p.y.abs() > 1.0e6
            || p.z.abs() > 1.0e6
    }) {
        return Err(RouteAuditError::NonFinitePath);
    }
    if collision.ground.is_empty()
        || collision.ground.len() > PreviewCollisionWorld::MAX_GROUND_TRIANGLES
        || collision.ground.iter().any(|t| {
            t.vertices
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1.0e6)
        })
    {
        return Err(RouteAuditError::InvalidSupport);
    }

    let mut index = SupportIndex::build(&collision.ground)?;
    let mut segment_counts = Vec::with_capacity(path.len());
    let mut total = 1usize;
    for i in 0..path.len() {
        let a = path[i];
        let b = path[(i + 1) % path.len()];
        let scene_len = (((b.x - a.x).powi(2) + (b.y - a.y).powi(2))
            * config.units_per_sim_unit.powi(2)
            + (b.z - a.z).powi(2))
        .sqrt();
        let count = (scene_len / config.max_spacing_scene).ceil().max(1.0) as usize;
        total = total
            .checked_add(count)
            .ok_or(RouteAuditError::TooManySamples)?;
        if total > MAX_ROUTE_SAMPLES {
            return Err(RouteAuditError::TooManySamples);
        }
        segment_counts.push(count);
    }

    let mut samples = Vec::with_capacity(total);
    let mut candidate_checks = 0usize;
    let mut prior: Option<Candidate> = None;
    let mut seam = None;
    let mut initial_support = None;
    for (segment, count) in segment_counts.iter().copied().enumerate() {
        let a = path[segment];
        let b = path[(segment + 1) % path.len()];
        for step in 0..count {
            let fraction = step as f64 / count as f64;
            let p = ReferencePathPoint {
                x: a.x + (b.x - a.x) * fraction,
                y: a.y + (b.y - a.y) * fraction,
                z: a.z + (b.z - a.z) * fraction,
            };
            let (selected, alternatives, checks) =
                index.select(p, config.units_per_sim_unit, prior)?;
            candidate_checks = candidate_checks
                .checked_add(checks)
                .ok_or(RouteAuditError::WorkLimitExceeded)?;
            if candidate_checks > MAX_CANDIDATE_CHECKS {
                return Err(RouteAuditError::WorkLimitExceeded);
            }
            if let Some(candidate) = selected {
                prior = Some(candidate);
            }
            if samples.is_empty() {
                initial_support = selected;
            }
            samples.push(RouteSampleDiagnostic {
                segment_index: segment,
                segment_fraction: fraction,
                supported: selected.is_some(),
                support_z: selected.map(|c| c.z),
                alternate_planes: alternatives,
                selection_ambiguous: index.last_ambiguous,
            });
        }
        // Sample every row endpoint exactly once as the start of the next segment.
        // For the final segment, append row zero to make seam support explicit.
        if segment + 1 == path.len() {
            let p = path[0];
            let (selected, alternatives, checks) =
                index.select(p, config.units_per_sim_unit, prior)?;
            candidate_checks = candidate_checks
                .checked_add(checks)
                .ok_or(RouteAuditError::WorkLimitExceeded)?;
            if candidate_checks > MAX_CANDIDATE_CHECKS {
                return Err(RouteAuditError::WorkLimitExceeded);
            }
            seam = Some(selected);
            samples.push(RouteSampleDiagnostic {
                segment_index: segment,
                segment_fraction: 1.0,
                supported: selected.is_some(),
                support_z: selected.map(|c| c.z),
                alternate_planes: alternatives,
                selection_ambiguous: index.last_ambiguous,
            });
        }
    }

    let supported_count = samples.iter().filter(|s| s.supported).count();
    let unsupported_runs = runs(&samples);
    let seam = seam.expect("closed path always emits a seam probe");
    let seam_matches_initial_support = match (initial_support, seam) {
        (Some(a), Some(b)) => {
            !samples.first().is_some_and(|s| s.selection_ambiguous)
                && !samples.last().is_some_and(|s| s.selection_ambiguous)
                && (a.z - b.z).abs() <= 1.0e-8
                && normal_distance(a.normal, b.normal) <= 1.0e-12
        }
        _ => false,
    };
    Ok(RouteCoverageReport {
        sample_count: samples.len(),
        supported_count,
        unsupported_count: samples.len() - supported_count,
        unsupported_runs,
        seam_supported: seam.is_some(),
        seam_support_z: seam.map(|c| c.z),
        seam_matches_initial_support,
        candidate_checks,
        samples,
    })
}

fn runs(samples: &[RouteSampleDiagnostic]) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut start = None;
    for (i, sample) in samples.iter().enumerate() {
        if !sample.supported {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            result.push((s, i - 1));
        }
    }
    if let Some(s) = start {
        result.push((s, samples.len() - 1));
    }
    result
}

struct SupportIndex<'a> {
    triangles: &'a [PreviewGroundTriangle],
    cells: BTreeMap<(i32, i32), Vec<usize>>,
    spanning: Vec<usize>,
    last_ambiguous: bool,
}

impl<'a> SupportIndex<'a> {
    fn build(triangles: &'a [PreviewGroundTriangle]) -> Result<Self, RouteAuditError> {
        let mut cells: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
        let mut spanning = Vec::new();
        let mut refs = 0usize;
        for (i, tri) in triangles.iter().enumerate() {
            let min_x = tri
                .vertices
                .iter()
                .map(|v| v[0])
                .fold(f64::INFINITY, f64::min);
            let max_x = tri
                .vertices
                .iter()
                .map(|v| v[0])
                .fold(f64::NEG_INFINITY, f64::max);
            let min_y = tri
                .vertices
                .iter()
                .map(|v| v[1])
                .fold(f64::INFINITY, f64::min);
            let max_y = tri
                .vertices
                .iter()
                .map(|v| v[1])
                .fold(f64::NEG_INFINITY, f64::max);
            let cell = |v: f64| -> Result<i32, RouteAuditError> {
                let n = (v / GRID_CELL).floor();
                if !n.is_finite() || n < i32::MIN as f64 || n > i32::MAX as f64 {
                    Err(RouteAuditError::IndexTooLarge)
                } else {
                    Ok(n as i32)
                }
            };
            let (x0, x1, y0, y1) = (cell(min_x)?, cell(max_x)?, cell(min_y)?, cell(max_y)?);
            let nx = (i64::from(x1) - i64::from(x0) + 1) as u64;
            let ny = (i64::from(y1) - i64::from(y0) + 1) as u64;
            let area = nx.checked_mul(ny).ok_or(RouteAuditError::IndexTooLarge)?;
            if area > MAX_CELLS_PER_TRIANGLE {
                spanning.push(i);
                continue;
            }
            refs = refs
                .checked_add(usize::try_from(area).map_err(|_| RouteAuditError::IndexTooLarge)?)
                .ok_or(RouteAuditError::IndexTooLarge)?;
            if refs > MAX_TRIANGLE_REFS {
                return Err(RouteAuditError::IndexTooLarge);
            }
            for x in x0..=x1 {
                for y in y0..=y1 {
                    cells.entry((x, y)).or_default().push(i);
                }
            }
        }
        Ok(Self {
            triangles,
            cells,
            spanning,
            last_ambiguous: false,
        })
    }

    fn select(
        &mut self,
        p: ReferencePathPoint,
        units: f64,
        prior: Option<Candidate>,
    ) -> Result<(Option<Candidate>, usize, usize), RouteAuditError> {
        let x = p.x / units;
        let y = p.y / units;
        let cx = (x / GRID_CELL).floor() as i32;
        let cy = (y / GRID_CELL).floor() as i32;
        let cell_indices = self.cells.get(&(cx, cy));
        let cell_count = cell_indices.map_or(0, Vec::len);
        let checks = cell_count
            .checked_add(self.spanning.len())
            .ok_or(RouteAuditError::WorkLimitExceeded)?;
        if checks > MAX_CELL_TRIANGLES {
            return Err(RouteAuditError::WorkLimitExceeded);
        }
        let mut candidates = Vec::with_capacity(checks);
        if let Some(indices) = cell_indices {
            for &i in indices {
                let triangle = self.triangles[i];
                if let Some(z) = audit_triangle_height(triangle.vertices, Vec2 { x, y }) {
                    if let Some(normal) = triangle_normal(triangle.vertices) {
                        candidates.push(Candidate { z, normal });
                    }
                }
            }
        }
        for &i in &self.spanning {
            let triangle = self.triangles[i];
            if let Some(z) = audit_triangle_height(triangle.vertices, Vec2 { x, y }) {
                if let Some(normal) = triangle_normal(triangle.vertices) {
                    candidates.push(Candidate { z, normal });
                }
            }
        }
        candidates.sort_unstable_by(candidate_key_cmp);
        candidates.dedup_by(|a, b| candidate_key_cmp(a, b).is_eq());
        let mut unique = candidates;
        unique.sort_by(|a, b| {
            let ap = prior.map_or(0.0, |q| (a.z - q.z).abs());
            let bp = prior.map_or(0.0, |q| (b.z - q.z).abs());
            (a.z - p.z)
                .abs()
                .total_cmp(&(b.z - p.z).abs())
                .then_with(|| ap.total_cmp(&bp))
                .then_with(|| {
                    normal_distance(a.normal, prior.map_or([0.0, 0.0, 1.0], |q| q.normal))
                        .total_cmp(&normal_distance(
                            b.normal,
                            prior.map_or([0.0, 0.0, 1.0], |q| q.normal),
                        ))
                })
                .then_with(|| a.z.total_cmp(&b.z))
                .then_with(|| a.normal[0].total_cmp(&b.normal[0]))
                .then_with(|| a.normal[1].total_cmp(&b.normal[1]))
                .then_with(|| a.normal[2].total_cmp(&b.normal[2]))
        });
        let chosen = unique.first().copied();
        self.last_ambiguous = unique.get(1).is_some_and(|second| {
            let Some(first) = chosen else {
                return false;
            };
            let prior_z = prior.map_or(p.z, |q| q.z);
            let prior_normal = prior.map_or([0.0, 0.0, 1.0], |q| q.normal);
            (first.z - p.z).abs() == (second.z - p.z).abs()
                && (first.z - prior_z).abs() == (second.z - prior_z).abs()
                && normal_distance(first.normal, prior_normal)
                    == normal_distance(second.normal, prior_normal)
                && (first.z - second.z).abs() > 1.0
        });
        let alternatives = chosen.map_or(0, |c| {
            unique
                .iter()
                .filter(|other| (other.z - c.z).abs() > 1.0)
                .count()
        });
        Ok((chosen, alternatives, checks))
    }
}

fn candidate_key_cmp(a: &Candidate, b: &Candidate) -> std::cmp::Ordering {
    a.z.total_cmp(&b.z)
        .then_with(|| a.normal[0].total_cmp(&b.normal[0]))
        .then_with(|| a.normal[1].total_cmp(&b.normal[1]))
        .then_with(|| a.normal[2].total_cmp(&b.normal[2]))
}

/// Audit-specific projected containment uses a scale-aware orient2d error
/// bound, avoiding the preview contact selector's broader gameplay epsilon.
fn audit_triangle_height(vertices: [[f64; 3]; 3], point: Vec2) -> Option<f64> {
    let a = Vec2::new(vertices[0][0], vertices[0][1]);
    let b = Vec2::new(vertices[1][0], vertices[1][1]);
    let c = Vec2::new(vertices[2][0], vertices[2][1]);
    let (area, _) = orient2d(a, b, c);
    if !area.is_finite() || area.abs() <= 1e-10 {
        return None;
    }
    for (start, end) in [(a, b), (b, c), (c, a)] {
        let (edge, error) = orient2d(start, end, point);
        if !edge.is_finite() || (area > 0.0 && edge < -error) || (area < 0.0 && edge > error) {
            return None;
        }
    }
    let (weight_a, _) = orient2d(b, c, point);
    let (weight_b, _) = orient2d(c, a, point);
    let (weight_c, _) = orient2d(a, b, point);
    let weights = [weight_a / area, weight_b / area, weight_c / area];
    if weights.iter().any(|value| !value.is_finite()) {
        return None;
    }
    Some(vertices[0][2] * weights[0] + vertices[1][2] * weights[1] + vertices[2][2] * weights[2])
}

fn orient2d(a: Vec2, b: Vec2, point: Vec2) -> (f64, f64) {
    let left = (b.x - a.x) * (point.y - a.y);
    let right = (b.y - a.y) * (point.x - a.x);
    let determinant = left - right;
    let error_bound = 8.0 * f64::EPSILON * (left.abs() + right.abs());
    (determinant, error_bound)
}

fn normal_distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(ground: Vec<PreviewGroundTriangle>) -> PreviewCollisionWorld {
        PreviewCollisionWorld {
            ground,
            walls: vec![],
            vehicle_radius: 1.0,
            vehicle_min_z: 0.0,
            vehicle_max_z: 1.0,
            initial_ground_z: 0.0,
            units_per_sim_unit: 1.0,
            edge_rows: Vec::new(),
        }
    }

    fn tri(z: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> PreviewGroundTriangle {
        PreviewGroundTriangle {
            vertices: [[x0, y0, z], [x1, y0, z], [x0, y1, z]],
        }
    }

    fn square(z: f64) -> Vec<PreviewGroundTriangle> {
        vec![
            tri(z, 0.0, 0.0, 4.0, 4.0),
            PreviewGroundTriangle {
                vertices: [[4.0, 0.0, z], [4.0, 4.0, z], [0.0, 4.0, z]],
            },
        ]
    }

    fn loop_path() -> [ReferencePathPoint; 4] {
        [
            ReferencePathPoint {
                x: 1.0,
                y: 1.0,
                z: 0.0,
            },
            ReferencePathPoint {
                x: 3.0,
                y: 1.0,
                z: 0.0,
            },
            ReferencePathPoint {
                x: 3.0,
                y: 3.0,
                z: 0.0,
            },
            ReferencePathPoint {
                x: 1.0,
                y: 3.0,
                z: 0.0,
            },
        ]
    }

    #[test]
    fn ordered_closed_seam_is_sampled_and_spacing_is_bounded() {
        let report = audit_closed_reference_path(
            &world(square(0.0)),
            &loop_path(),
            RouteAuditConfig::default(),
        )
        .unwrap();
        assert!(report.seam_supported);
        assert_eq!(report.samples.last().unwrap().segment_fraction, 1.0);
        assert_eq!(report.samples.last().unwrap().segment_index, 3);
        assert_eq!(report.unsupported_count, 0);
        for pair in report.samples.windows(2) {
            let a = pair[0];
            let b = pair[1];
            if a.segment_index == b.segment_index {
                assert!(b.segment_fraction >= a.segment_fraction);
                assert!((b.segment_fraction - a.segment_fraction) * 2.0 <= 0.5 + 1.0e-9);
            }
        }
        assert!(report.candidate_checks <= report.sample_count * 2);
    }

    #[test]
    fn overpass_choice_uses_expected_height_and_ignores_triangle_order() {
        let low = vec![tri(0.0, 0.0, 0.0, 4.0, 4.0), tri(12.0, 0.0, 0.0, 4.0, 4.0)];
        let mut reversed = low.clone();
        reversed.reverse();
        let path = [
            ReferencePathPoint {
                x: 1.0,
                y: 1.0,
                z: 0.0,
            },
            ReferencePathPoint {
                x: 2.0,
                y: 1.0,
                z: 0.1,
            },
        ];
        for triangles in [low, reversed] {
            let report =
                audit_closed_reference_path(&world(triangles), &path, RouteAuditConfig::default())
                    .unwrap();
            assert!(report.samples.iter().all(|s| s.support_z.unwrap() == 0.0));
            assert!(report.samples.iter().all(|s| s.alternate_planes == 1));
        }
    }

    #[test]
    fn nearby_edge_does_not_count_as_support() {
        let geometry = world(vec![tri(0.0, 0.0, 0.0, 2.0, 2.0)]);
        let path = [
            ReferencePathPoint {
                x: 2.01,
                y: 1.0,
                z: 0.0,
            },
            ReferencePathPoint {
                x: 2.01,
                y: 1.1,
                z: 0.0,
            },
        ];
        let report =
            audit_closed_reference_path(&geometry, &path, RouteAuditConfig::default()).unwrap();
        assert!(report.unsupported_count > 0);
        assert!(!report.seam_supported);
    }

    #[test]
    fn large_edge_roundoff_does_not_turn_a_nearby_point_into_support() {
        let triangle = PreviewGroundTriangle {
            vertices: [
                [0.0, 0.0, 0.0],
                [1_000_000.0, 0.0, 0.0],
                [0.0, 1_000_000.0, 0.0],
            ],
        };
        let path = [
            ReferencePathPoint {
                x: 500_000.0,
                y: 500_000.08,
                z: 0.0,
            },
            ReferencePathPoint {
                x: 500_010.0,
                y: 500_000.08,
                z: 0.0,
            },
        ];
        let report = audit_closed_reference_path(
            &world(vec![triangle]),
            &path,
            RouteAuditConfig {
                max_spacing_scene: 100.0,
                ..RouteAuditConfig::default()
            },
        )
        .unwrap();
        assert_eq!(report.supported_count, 0);
    }

    #[test]
    fn pathological_overlapping_cell_fails_with_bounded_work() {
        let triangles = vec![tri(0.0, 0.0, 0.0, 4.0, 4.0); 100_000];
        let path = [
            ReferencePathPoint {
                x: 1.0,
                y: 1.0,
                z: 0.0,
            },
            ReferencePathPoint {
                x: 2.0,
                y: 1.0,
                z: 0.0,
            },
        ];
        assert_eq!(
            audit_closed_reference_path(&world(triangles), &path, RouteAuditConfig::default()),
            Err(RouteAuditError::WorkLimitExceeded)
        );
    }

    #[test]
    fn ambiguous_initial_overpass_does_not_claim_seam_continuity() {
        let triangles = vec![tri(0.0, 0.0, 0.0, 4.0, 4.0), tri(12.0, 0.0, 0.0, 4.0, 4.0)];
        let path = [
            ReferencePathPoint {
                x: 1.0,
                y: 1.0,
                z: 6.0,
            },
            ReferencePathPoint {
                x: 2.0,
                y: 1.0,
                z: 6.0,
            },
        ];
        let report =
            audit_closed_reference_path(&world(triangles), &path, RouteAuditConfig::default())
                .unwrap();
        assert!(report.samples[0].selection_ambiguous);
        assert!(!report.seam_matches_initial_support);
    }

    #[test]
    fn malformed_and_excessive_inputs_are_rejected() {
        let geometry = world(vec![tri(0.0, 0.0, 0.0, 4.0, 4.0)]);
        let mut path = loop_path();
        path[0].x = f64::NAN;
        assert_eq!(
            audit_closed_reference_path(&geometry, &path, RouteAuditConfig::default()),
            Err(RouteAuditError::NonFinitePath)
        );
        let mut path = loop_path();
        path[0].x = 1_000_001.0;
        assert_eq!(
            audit_closed_reference_path(&geometry, &path, RouteAuditConfig::default()),
            Err(RouteAuditError::NonFinitePath)
        );
        assert_eq!(
            audit_closed_reference_path(
                &geometry,
                &loop_path(),
                RouteAuditConfig {
                    max_spacing_scene: 0.000001,
                    ..RouteAuditConfig::default()
                }
            ),
            Err(RouteAuditError::TooManySamples)
        );
        assert_eq!(
            audit_closed_reference_path(&world(vec![]), &loop_path(), RouteAuditConfig::default()),
            Err(RouteAuditError::InvalidSupport)
        );
    }
}
