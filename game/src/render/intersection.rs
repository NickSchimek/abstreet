use crate::app::App;
use crate::colors::ColorScheme;
use crate::helpers::ID;
use crate::options::TrafficSignalStyle;
use crate::render::{
    draw_signal_phase, DrawOptions, Renderable, CROSSWALK_LINE_THICKNESS, OUTLINE_THICKNESS,
};
use ezgui::{Color, Drawable, EventCtx, GeomBatch, GfxCtx, Line, RewriteColor, Text};
use geom::{Angle, ArrowCap, Distance, Line, PolyLine, Polygon, Pt2D, Ring, Time, EPSILON_DIST};
use map_model::{
    Intersection, IntersectionID, IntersectionType, Map, Road, RoadWithStopSign, Turn, TurnType,
    SIDEWALK_THICKNESS,
};
use std::cell::RefCell;

pub struct DrawIntersection {
    pub id: IntersectionID,
    intersection_type: IntersectionType,
    zorder: isize,

    draw_default: Drawable,
    pub draw_traffic_signal: RefCell<Option<(Time, Drawable)>>,
}

impl DrawIntersection {
    pub fn new(ctx: &EventCtx, i: &Intersection, map: &Map, cs: &ColorScheme) -> DrawIntersection {
        // Order matters... main polygon first, then sidewalk corners.
        let mut default_geom = GeomBatch::new();
        default_geom.push(cs.normal_intersection, i.polygon.clone());
        default_geom.extend(cs.sidewalk, calculate_corners(i, map));

        for turn in map.get_turns_in_intersection(i.id) {
            // Avoid double-rendering
            if turn.turn_type == TurnType::Crosswalk
                && !turn.other_crosswalk_ids.iter().any(|id| *id < turn.id)
            {
                make_crosswalk(&mut default_geom, turn, map, cs);
            }
        }

        if i.is_private(map) {
            default_geom.push(cs.private_road.alpha(0.5), i.polygon.clone());
        }

        match i.intersection_type {
            IntersectionType::Border => {
                let r = map.get_r(*i.roads.iter().next().unwrap());
                default_geom.extend(cs.road_center_line, calculate_border_arrows(i, r, map));
            }
            IntersectionType::StopSign => {
                for ss in map.get_stop_sign(i.id).roads.values() {
                    if ss.must_stop {
                        if let Some((octagon, pole)) = DrawIntersection::stop_sign_geom(ss, map) {
                            default_geom.push(cs.stop_sign, octagon);
                            default_geom.push(cs.stop_sign_pole, pole);
                        }
                    }
                }
            }
            IntersectionType::Construction => {
                // TODO Centering seems weird
                default_geom.append(
                    GeomBatch::mapspace_svg(
                        ctx.prerender,
                        "system/assets/map/under_construction.svg",
                    )
                    .scale(0.08)
                    .centered_on(i.polygon.center()),
                );
            }
            IntersectionType::TrafficSignal => {}
        }

        let zorder = i.get_zorder(map);
        if zorder < 0 {
            default_geom = default_geom.color(RewriteColor::ChangeAlpha(0.5));
        }

        DrawIntersection {
            id: i.id,
            intersection_type: i.intersection_type,
            zorder,
            draw_default: ctx.upload(default_geom),
            draw_traffic_signal: RefCell::new(None),
        }
    }

    // Returns the (octagon, pole) if there's room to draw it.
    pub fn stop_sign_geom(ss: &RoadWithStopSign, map: &Map) -> Option<(Polygon, Polygon)> {
        let trim_back = Distance::meters(0.1);
        let rightmost = map.get_l(ss.rightmost_lane);
        // TODO The dream of trimming f64's was to isolate epsilon checks like this...
        if rightmost.length() - trim_back <= EPSILON_DIST {
            // TODO warn
            return None;
        }
        let last_line = map.right_shift_line(
            rightmost
                .lane_center_pts
                .exact_slice(Distance::ZERO, rightmost.length() - trim_back)
                .last_line(),
            rightmost.width,
        );

        let octagon = make_octagon(last_line.pt2(), Distance::meters(1.0), last_line.angle());
        let pole = Line::must_new(
            last_line
                .pt2()
                .project_away(Distance::meters(1.5), last_line.angle().opposite()),
            // TODO Slightly < 0.9
            last_line
                .pt2()
                .project_away(Distance::meters(0.9), last_line.angle().opposite()),
        )
        .make_polygons(Distance::meters(0.3));
        Some((octagon, pole))
    }
}

impl Renderable for DrawIntersection {
    fn get_id(&self) -> ID {
        ID::Intersection(self.id)
    }

    fn draw(&self, g: &mut GfxCtx, app: &App, opts: &DrawOptions) {
        g.redraw(&self.draw_default);

        if self.intersection_type == IntersectionType::TrafficSignal
            && !opts.suppress_traffic_signal_details.contains(&self.id)
        {
            let signal = app.primary.map.get_traffic_signal(self.id);
            let mut maybe_redraw = self.draw_traffic_signal.borrow_mut();
            let recalc = maybe_redraw
                .as_ref()
                .map(|(t, _)| *t != app.primary.sim.time())
                .unwrap_or(true);
            if recalc {
                let (idx, remaining) = app.primary.sim.current_phase_and_remaining_time(self.id);
                let mut batch = GeomBatch::new();
                draw_signal_phase(
                    g.prerender,
                    &signal.phases[idx],
                    self.id,
                    Some(remaining),
                    &mut batch,
                    app,
                    app.opts.traffic_signal_style.clone(),
                );
                if app.opts.traffic_signal_style != TrafficSignalStyle::BAP {
                    batch.append(
                        Text::from(Line(format!("{}", idx + 1)))
                            .render_to_batch(g.prerender)
                            .scale(0.1)
                            .centered_on(app.primary.map.get_i(self.id).polygon.center()),
                    );
                }
                *maybe_redraw = Some((app.primary.sim.time(), g.prerender.upload(batch)));
            }
            let (_, batch) = maybe_redraw.as_ref().unwrap();
            g.redraw(batch);
        }
    }

    fn get_outline(&self, map: &Map) -> Polygon {
        let poly = &map.get_i(self.id).polygon;
        poly.to_outline(OUTLINE_THICKNESS)
            .unwrap_or_else(|_| poly.clone())
    }

    fn contains_pt(&self, pt: Pt2D, map: &Map) -> bool {
        map.get_i(self.id).polygon.contains_pt(pt)
    }

    fn get_zorder(&self) -> isize {
        self.zorder
    }
}

// TODO Temporarily public for debugging.
pub fn calculate_corners(i: &Intersection, map: &Map) -> Vec<Polygon> {
    let mut corners = Vec::new();

    for turn in map.get_turns_in_intersection(i.id) {
        if turn.turn_type == TurnType::SharedSidewalkCorner {
            // Avoid double-rendering
            if map.get_l(turn.id.src).dst_i != i.id {
                continue;
            }
            let width = map
                .get_l(turn.id.src)
                .width
                .min(map.get_l(turn.id.dst).width);

            // Special case for dead-ends: just thicken the geometry.
            if i.roads.len() == 1 {
                corners.push(turn.geom.make_polygons(width));
                continue;
            }

            let l1 = map.get_l(turn.id.src);
            let l2 = map.get_l(turn.id.dst);

            let mut pts = map.left_shift(turn.geom.clone(), width / 2.0).into_points();
            pts.push(map.left_shift_line(l2.first_line(), width / 2.0).pt1());
            pts.push(map.right_shift_line(l2.first_line(), width / 2.0).pt1());
            pts.extend(
                map.right_shift(turn.geom.clone(), width / 2.0)
                    .reversed()
                    .into_points(),
            );
            pts.push(map.right_shift_line(l1.last_line(), width / 2.0).pt2());
            pts.push(map.left_shift_line(l1.last_line(), width / 2.0).pt2());
            pts.push(pts[0]);
            corners.push(Polygon::buggy_new(pts));
        }
    }

    corners
}

fn calculate_border_arrows(i: &Intersection, r: &Road, map: &Map) -> Vec<Polygon> {
    let mut result = Vec::new();

    let mut width_fwd = Distance::ZERO;
    let mut width_back = Distance::ZERO;
    for (l, _) in r.children(true) {
        width_fwd += map.get_l(*l).width;
    }
    for (l, _) in r.children(false) {
        width_back += map.get_l(*l).width;
    }
    let center = r.get_current_center(map);

    // These arrows should point from the void to the road
    if !i.outgoing_lanes.is_empty() {
        let (line, width) = if r.dst_i == i.id {
            (
                map.left_shift_line(center.last_line(), width_back / 2.0)
                    .reverse(),
                width_back,
            )
        } else {
            (
                map.right_shift_line(center.first_line(), width_fwd / 2.0),
                width_fwd,
            )
        };
        result.push(
            // DEGENERATE_INTERSECTION_HALF_LENGTH is 2.5m...
            PolyLine::must_new(vec![
                line.unbounded_dist_along(Distance::meters(-9.5)),
                line.unbounded_dist_along(Distance::meters(-0.5)),
            ])
            .make_arrow(width / 3.0, ArrowCap::Triangle),
        );
    }

    // These arrows should point from the road to the void
    if !i.incoming_lanes.is_empty() {
        let (line, width) = if r.dst_i == i.id {
            (
                map.right_shift_line(center.last_line(), width_fwd / 2.0)
                    .reverse(),
                width_fwd,
            )
        } else {
            (
                map.left_shift_line(center.first_line(), width_back / 2.0),
                width_back,
            )
        };
        result.push(
            PolyLine::must_new(vec![
                line.unbounded_dist_along(Distance::meters(-0.5)),
                line.unbounded_dist_along(Distance::meters(-9.5)),
            ])
            .make_arrow(width / 3.0, ArrowCap::Triangle),
        );
    }

    result
}

// TODO A squished octagon would look better
fn make_octagon(center: Pt2D, radius: Distance, facing: Angle) -> Polygon {
    Ring::must_new(
        (0..=8)
            .map(|i| center.project_away(radius, facing.rotate_degs(22.5 + f64::from(i * 360 / 8))))
            .collect(),
    )
    .to_polygon()
}

pub fn make_crosswalk(batch: &mut GeomBatch, turn: &Turn, map: &Map, cs: &ColorScheme) {
    if make_rainbow_crosswalk(batch, turn, map) {
        return;
    }

    // This size also looks better for shoulders
    let width = SIDEWALK_THICKNESS;
    // Start at least width out to not hit sidewalk corners. Also account for the thickness of the
    // crosswalk line itself. Center the lines inside these two boundaries.
    let boundary = width;
    let tile_every = width * 0.6;
    let line = {
        // The middle line in the crosswalk geometry is the main crossing line.
        let pts = turn.geom.points();
        if pts.len() < 3 {
            println!(
                "Not rendering crosswalk for {}; its geometry was squished earlier",
                turn.id
            );
            return;
        }
        match Line::new(pts[1], pts[2]) {
            Some(l) => l,
            None => {
                return;
            }
        }
    };

    let available_length = line.length() - (boundary * 2.0);
    if available_length > Distance::ZERO {
        let num_markings = (available_length / tile_every).floor() as usize;
        let mut dist_along =
            boundary + (available_length - tile_every * (num_markings as f64)) / 2.0;
        // TODO Seems to be an off-by-one sometimes. Not enough of these.
        let err = format!("make_crosswalk for {} broke", turn.id);
        for _ in 0..=num_markings {
            let pt1 = line.dist_along(dist_along).expect(&err);
            // Reuse perp_line. Project away an arbitrary amount
            let pt2 = pt1.project_away(Distance::meters(1.0), turn.angle());
            batch.push(
                cs.general_road_marking,
                perp_line(Line::must_new(pt1, pt2), width).make_polygons(CROSSWALK_LINE_THICKNESS),
            );

            // Actually every line is a double
            let pt3 = line
                .dist_along(dist_along + 2.0 * CROSSWALK_LINE_THICKNESS)
                .expect(&err);
            let pt4 = pt3.project_away(Distance::meters(1.0), turn.angle());
            batch.push(
                cs.general_road_marking,
                perp_line(Line::must_new(pt3, pt4), width).make_polygons(CROSSWALK_LINE_THICKNESS),
            );

            dist_along += tile_every;
        }
    }
}

fn make_rainbow_crosswalk(batch: &mut GeomBatch, turn: &Turn, map: &Map) -> bool {
    // TODO The crosswalks aren't tagged in OSM yet. Manually hardcoding some now.
    let node = map.get_i(turn.id.parent).orig_id.osm_node_id;
    let way = map.get_parent(turn.id.src).orig_id.osm_way_id;
    match (node, way) {
        // Broadway and Pine
        (53073255, 428246441) |
        (53073255, 332601014) |
        // Broadway and Pike
        (53073254, 6447455) |
        (53073254, 607690679) |
        // 10th and Pine
        (53168934, 6456052) |
        // 10th and Pike
        (53200834, 6456052) |
        // 11th and Pine
        (53068795, 607691081) |
        (53068795, 65588105) |
        // 11th and Pike
        (53068794, 65588105) => {}
        _ => { return false; }
    }

    let total_width = map.get_l(turn.id.src).width;
    let colors = vec![
        Color::WHITE,
        Color::RED,
        Color::ORANGE,
        Color::YELLOW,
        Color::GREEN,
        Color::BLUE,
        Color::hex("#8B00FF"),
        Color::WHITE,
    ];
    let band_width = total_width / (colors.len() as f64);
    let slice = turn
        .geom
        .exact_slice(total_width, turn.geom.length() - total_width)
        .must_shift_left(total_width / 2.0 - band_width / 2.0);
    for (idx, color) in colors.into_iter().enumerate() {
        batch.push(
            color,
            slice
                .must_shift_right(band_width * (idx as f64))
                .make_polygons(band_width),
        );
    }
    true
}

// TODO copied from DrawLane
fn perp_line(l: Line, length: Distance) -> Line {
    let pt1 = l.shift_right(length / 2.0).pt1();
    let pt2 = l.shift_left(length / 2.0).pt1();
    Line::must_new(pt1, pt2)
}
