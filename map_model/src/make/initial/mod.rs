mod geometry;
pub mod lane_specs;

pub use self::geometry::intersection_polygon;
use crate::raw::{DrivingSide, OriginalIntersection, OriginalRoad, RawMap, RawRoad};
use crate::IntersectionType;
use abstutil::{Tags, Timer};
use geom::{Bounds, Distance, PolyLine, Pt2D};
use lane_specs::LaneSpec;
use std::collections::{BTreeMap, BTreeSet};

pub struct InitialMap {
    pub roads: BTreeMap<OriginalRoad, Road>,
    pub intersections: BTreeMap<OriginalIntersection, Intersection>,

    pub bounds: Bounds,
}

pub struct Road {
    // Redundant but useful to embed
    pub id: OriginalRoad,
    pub src_i: OriginalIntersection,
    pub dst_i: OriginalIntersection,
    // The true center of the road, including sidewalks
    pub trimmed_center_pts: PolyLine,
    pub half_width: Distance,
    pub lane_specs: Vec<LaneSpec>,
    pub osm_tags: Tags,
}

impl Road {
    pub fn new(id: OriginalRoad, r: &RawRoad, driving_side: DrivingSide) -> Road {
        let lane_specs = lane_specs::get_lane_specs(&r.osm_tags);
        let (trimmed_center_pts, total_width) = r.get_geometry(id, driving_side);

        Road {
            id,
            src_i: id.i1,
            dst_i: id.i2,
            trimmed_center_pts,
            half_width: total_width / 2.0,
            lane_specs,
            osm_tags: r.osm_tags.clone(),
        }
    }
}

pub struct Intersection {
    // Redundant but useful to embed
    pub id: OriginalIntersection,
    pub polygon: Vec<Pt2D>,
    pub roads: BTreeSet<OriginalRoad>,
    pub intersection_type: IntersectionType,
    pub elevation: Distance,
}

impl InitialMap {
    pub fn new(raw: &RawMap, bounds: &Bounds, timer: &mut Timer) -> InitialMap {
        let mut m = InitialMap {
            roads: BTreeMap::new(),
            intersections: BTreeMap::new(),
            bounds: bounds.clone(),
        };

        for (id, i) in &raw.intersections {
            m.intersections.insert(
                *id,
                Intersection {
                    id: *id,
                    polygon: Vec::new(),
                    roads: BTreeSet::new(),
                    intersection_type: i.intersection_type,
                    elevation: i.elevation,
                },
            );
        }

        for (id, r) in &raw.roads {
            if id.i1 == id.i2 {
                timer.warn(format!("Skipping loop {}", id));
                continue;
            }
            if PolyLine::new(r.center_points.clone()).is_err() {
                timer.warn(format!("Skipping broken geom {}", id));
                continue;
            }

            m.intersections.get_mut(&id.i1).unwrap().roads.insert(*id);
            m.intersections.get_mut(&id.i2).unwrap().roads.insert(*id);

            m.roads
                .insert(*id, Road::new(*id, r, raw.config.driving_side));
        }

        // Detect all overlapping geometry upfront
        timer.start_iter("detect overlapping roads", m.intersections.len());
        let mut problems = BTreeSet::new();
        for i in m.intersections.values() {
            timer.next();
            for r1 in &i.roads {
                for r2 in &i.roads {
                    if r1 >= r2 {
                        continue;
                    }
                    if m.roads[r1].trimmed_center_pts == m.roads[r2].trimmed_center_pts {
                        problems.insert(format!("{} and {} overlap", r1.way_url(), r2.way_url()));
                    }
                }
            }
        }
        if !problems.is_empty() {
            for x in problems {
                println!("- {}", x);
            }
            panic!(
                "Some roads have overlapping segments in OSM. You likely need to fix OSM and make \
                 the two ways meet at exactly one node."
            );
        }

        timer.start_iter("find each intersection polygon", m.intersections.len());
        for i in m.intersections.values_mut() {
            timer.next();

            i.polygon = intersection_polygon(raw.config.driving_side, i, &mut m.roads, timer).0;
        }

        m
    }
}
