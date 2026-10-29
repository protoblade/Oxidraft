//! System variables: SETVAR, and a variable's name typed on its own
//! (ORTHOMODE, CLAYER, OSMODE, ...), as in AutoCAD. Each reads and sets
//! what the app, the command engine or the drawing keeps.

use oxidraft_commands::current_props::{color_text, parse_color, parse_lineweight};
use oxidraft_commands::sysvar::{SetVar, VarValue};
use oxidraft_geom::Point;
use oxidraft_model::snap::SnapKind;
use oxidraft_model::{AngleType, LengthType, Lineweight, MODEL, Transparency};

use crate::app::OxidraftApp;

type Get = fn(&OxidraftApp) -> String;
type Set = fn(&mut OxidraftApp, &str) -> Result<(), String>;

/// A variable: its name, how to read it, and how to set it (none when
/// it's read only).
struct Var {
    name: &'static str,
    get: Get,
    set: Option<Set>,
}

fn flag(on: bool) -> String {
    if on { "1".into() } else { "0".into() }
}

fn parse_flag(v: &str) -> Result<bool, String> {
    match v.trim() {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err("Requires an integer between 0 and 1.".into()),
    }
}

fn parse_number(v: &str) -> Result<f64, String> {
    v.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| "Requires a number.".into())
}

fn parse_positive(v: &str) -> Result<f64, String> {
    parse_number(v).and_then(|v| if v > 0.0 { Ok(v) } else { Err("Requires a positive number.".into()) })
}

fn parse_int(v: &str, lo: i64, hi: i64) -> Result<i64, String> {
    v.trim()
        .parse::<i64>()
        .ok()
        .filter(|n| (lo..=hi).contains(n))
        .ok_or_else(|| format!("Requires an integer between {lo} and {hi}."))
}

fn parse_point(v: &str) -> Result<Point, String> {
    let (x, y) = v.split_once(',').ok_or("Requires a point: x,y.")?;
    Ok(Point::new(parse_number(x)?, parse_number(y)?))
}

fn number(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn point(p: Point) -> String {
    format!("{},{}", number(p.x), number(p.y))
}

/// OSMODE's bits for the running snaps, as AutoCAD numbers them.
const OSMODE_BITS: [(SnapKind, u32); 10] = [
    (SnapKind::Endpoint, 1),
    (SnapKind::Midpoint, 2),
    (SnapKind::Center, 4),
    (SnapKind::Node, 8),
    (SnapKind::Quadrant, 16),
    (SnapKind::Intersection, 32),
    (SnapKind::Insertion, 64),
    (SnapKind::Perpendicular, 128),
    (SnapKind::Tangent, 256),
    (SnapKind::Nearest, 512),
];
/// OSMODE's bit for running snaps turned off (F3).
const OSMODE_OFF: u32 = 16384;

const LENGTH_TYPES: [LengthType; 5] = [
    LengthType::Scientific,
    LengthType::Decimal,
    LengthType::Engineering,
    LengthType::Architectural,
    LengthType::Fractional,
];
const ANGLE_TYPES: [AngleType; 5] =
    [AngleType::DecimalDegrees, AngleType::DegMinSec, AngleType::Grads, AngleType::Radians, AngleType::Surveyor];

macro_rules! var {
    ($name:literal, $get:expr) => {
        Var { name: $name, get: $get, set: None }
    };
    ($name:literal, $get:expr, $set:expr) => {
        Var { name: $name, get: $get, set: Some($set) }
    };
}

fn vars() -> Vec<Var> {
    vec![
        var!("ACADVER", |_| format!("Oxidraft {}", env!("CARGO_PKG_VERSION"))),
        var!("ANGBASE", |a| number(a.doc.units().base_angle), |a, v| {
            let mut u = a.doc.units();
            u.base_angle = parse_number(v)?;
            a.doc.set_units(u);
            Ok(())
        }),
        var!("ANGDIR", |a| flag(a.doc.units().clockwise), |a, v| {
            let mut u = a.doc.units();
            u.clockwise = parse_flag(v)?;
            a.doc.set_units(u);
            Ok(())
        }),
        var!("ANNOALLVISIBLE", |a| flag(a.doc.anno_all_visible()), |a, v| {
            a.doc.set_anno_all_visible(parse_flag(v)?);
            Ok(())
        }),
        var!("ANNOAUTOSCALE", |a| if a.anno_autoscale { "4".into() } else { "-4".into() }, |a, v| {
            a.anno_autoscale = parse_int(v, -4, 4)? > 0;
            Ok(())
        }),
        var!("ARRAYASSOCIATIVITY", |a| flag(a.engine.vars.array_assoc), |a, v| {
            a.engine.vars.array_assoc = parse_flag(v)?;
            Ok(())
        }),
        var!(
            "AUNITS",
            |a| ANGLE_TYPES.iter().position(|t| *t == a.doc.units().angle).unwrap_or(0).to_string(),
            |a, v| {
                let mut u = a.doc.units();
                u.angle = ANGLE_TYPES[parse_int(v, 0, 4)? as usize];
                a.doc.set_units(u);
                Ok(())
            }
        ),
        var!("AUPREC", |a| a.doc.units().angle_precision.to_string(), |a, v| {
            let mut u = a.doc.units();
            u.angle_precision = parse_int(v, 0, 8)? as u8;
            a.doc.set_units(u);
            Ok(())
        }),
        var!("CANNOSCALE", |a| oxidraft_model::annoscale::scale_name(a.doc.anno_scale()), |a, v| {
            let s = oxidraft_model::annoscale::parse_scale(v).ok_or("Requires a scale like 1:50.")?;
            a.set_anno_scale(s);
            Ok(())
        }),
        var!("CECOLOR", |a| color_text(a.doc.current_props().color), |a, v| {
            let mut p = a.doc.current_props().clone();
            p.color = parse_color(v).ok_or("Requires a color: a name, 1 to 255, BYLAYER or BYBLOCK.")?;
            a.doc.set_current_props(p);
            Ok(())
        }),
        var!("CELTYPE", |a| a.doc.current_props().linetype.clone(), |a, v| {
            let mut p = a.doc.current_props().clone();
            let t = v.trim();
            p.linetype = if t.eq_ignore_ascii_case("bylayer") || t.eq_ignore_ascii_case("byblock") {
                t.to_ascii_uppercase()
            } else {
                a.doc.linetype(t).map(|l| l.name).ok_or("That linetype isn't loaded.")?
            };
            a.doc.set_current_props(p);
            Ok(())
        }),
        var!(
            "CELWEIGHT",
            |a| match a.doc.current_props().lineweight {
                Lineweight::ByLayer => "-1".into(),
                Lineweight::ByBlock => "-2".into(),
                Lineweight::Default => "-3".into(),
                Lineweight::Hundredths(h) => h.to_string(),
            },
            |a, v| {
                let mut p = a.doc.current_props().clone();
                p.lineweight = match v.trim() {
                    "-1" => Lineweight::ByLayer,
                    "-2" => Lineweight::ByBlock,
                    "-3" => Lineweight::Default,
                    t => parse_lineweight(&format!("{}", parse_number(t)? / 100.0)).ok_or("Requires a lineweight.")?,
                };
                a.doc.set_current_props(p);
                Ok(())
            }
        ),
        var!(
            "CETRANSPARENCY",
            |a| match a.doc.current_props().transparency {
                Transparency::ByLayer => "ByLayer".into(),
                Transparency::ByBlock => "ByBlock".into(),
                Transparency::Percent(p) => p.to_string(),
            },
            |a, v| {
                let mut p = a.doc.current_props().clone();
                p.transparency = match v.trim().to_ascii_lowercase().as_str() {
                    "bylayer" => Transparency::ByLayer,
                    "byblock" => Transparency::ByBlock,
                    t => Transparency::Percent(parse_int(t, 0, 90)? as u8),
                };
                a.doc.set_current_props(p);
                Ok(())
            }
        ),
        var!("CHAMFERA", |a| number(a.engine.vars.chamfer_a), |a, v| {
            a.engine.vars.chamfer_a = parse_number(v)?.max(0.0);
            Ok(())
        }),
        var!("CHAMFERB", |a| number(a.engine.vars.chamfer_b), |a, v| {
            a.engine.vars.chamfer_b = parse_number(v)?.max(0.0);
            Ok(())
        }),
        var!("CLAYER", |a| a.doc.current_layer().to_string(), |a, v| {
            if a.doc.set_current_layer(v.trim()) { Ok(()) } else { Err("No such layer.".into()) }
        }),
        var!("CTAB", |a| {
            let space = a.doc.space();
            if space == MODEL {
                "Model".into()
            } else {
                a.doc.layouts().iter().find(|l| l.id == space).map_or_else(String::new, |l| l.name.clone())
            }
        }),
        var!("DIMASSOC", |a| a.engine.vars.dim_assoc.to_string(), |a, v| {
            a.engine.vars.dim_assoc = parse_int(v, 0, 2)? as u8;
            Ok(())
        }),
        var!("DIMSCALE", |a| number(a.engine.vars.dim_scale), |a, v| {
            a.engine.vars.dim_scale = parse_positive(v)?;
            Ok(())
        }),
        var!("DWGNAME", |a| a.file_name()),
        var!("DWGPREFIX", |a| {
            a.path
                .as_ref()
                .and_then(|p| p.parent())
                .map_or_else(String::new, |d| format!("{}{}", d.display(), std::path::MAIN_SEPARATOR))
        }),
        var!("DYNMODE", |a| if a.engine.dynamic_input { "3".into() } else { "0".into() }, |a, v| {
            a.engine.dynamic_input = parse_int(v, -3, 3)? > 0;
            Ok(())
        }),
        var!("EXTMAX", |a| a.doc.extents().map_or_else(String::new, |b| point(b.max))),
        var!("EXTMIN", |a| a.doc.extents().map_or_else(String::new, |b| point(b.min))),
        var!("FILLETRAD", |a| number(a.engine.vars.fillet_radius), |a, v| {
            a.engine.vars.fillet_radius = parse_number(v)?.max(0.0);
            Ok(())
        }),
        var!("FILLMODE", |a| flag(a.doc.fill()), |a, v| {
            a.doc.set_fill(parse_flag(v)?);
            Ok(())
        }),
        var!("GRIDMODE", |a| flag(a.grid), |a, v| {
            a.grid = parse_flag(v)?;
            Ok(())
        }),
        var!("GRIDUNIT", |a| number(a.grid_spacing.unwrap_or(0.0)), |a, v| {
            let s = parse_number(v)?;
            a.grid_spacing = (s > 0.0).then_some(s);
            a.grid = true;
            Ok(())
        }),
        var!("IMAGEFRAME", |a| a.doc.picture_frame(false).to_string(), |a, v| {
            a.doc.set_picture_frame(false, parse_int(v, 0, 2)? as u8);
            Ok(())
        }),
        var!("INSBASE", |a| point(a.doc.insbase()), |a, v| {
            a.doc.set_insbase(parse_point(v)?);
            Ok(())
        }),
        var!("INSUNITS", |a| a.doc.units().insertion.to_string(), |a, v| {
            let mut u = a.doc.units();
            u.insertion = parse_int(v, 0, 24)? as i16;
            a.doc.set_units(u);
            Ok(())
        }),
        var!("LAYOUTTAB", |a| flag(a.show_layout_tabs), |a, v| {
            a.show_layout_tabs = parse_flag(v)?;
            Ok(())
        }),
        var!("LIMCHECK", |a| flag(a.doc.limits_check()), |a, v| {
            a.doc.set_limits_check(parse_flag(v)?);
            Ok(())
        }),
        var!("LIMMAX", |a| point(a.doc.limits().1), |a, v| {
            let min = a.doc.limits().0;
            a.doc.set_limits(min, parse_point(v)?);
            Ok(())
        }),
        var!("LIMMIN", |a| point(a.doc.limits().0), |a, v| {
            let max = a.doc.limits().1;
            a.doc.set_limits(parse_point(v)?, max);
            Ok(())
        }),
        var!("LTSCALE", |a| number(a.doc.ltscale()), |a, v| {
            a.doc.set_ltscale(parse_positive(v)?);
            Ok(())
        }),
        var!(
            "LUNITS",
            |a| (LENGTH_TYPES.iter().position(|t| *t == a.doc.units().length).unwrap_or(1) + 1).to_string(),
            |a, v| {
                let mut u = a.doc.units();
                u.length = LENGTH_TYPES[parse_int(v, 1, 5)? as usize - 1];
                a.doc.set_units(u);
                Ok(())
            }
        ),
        var!("LUPREC", |a| a.doc.units().length_precision.to_string(), |a, v| {
            let mut u = a.doc.units();
            u.length_precision = parse_int(v, 0, 8)? as u8;
            a.doc.set_units(u);
            Ok(())
        }),
        var!("LWDISPLAY", |a| flag(a.show_lineweights), |a, v| {
            a.show_lineweights = parse_flag(v)?;
            Ok(())
        }),
        var!("OFFSETDIST", |a| a.engine.vars.offset_distance.map_or("-1".into(), number), |a, v| {
            let d = parse_number(v)?;
            a.engine.vars.offset_distance = (d >= 0.0).then_some(d);
            Ok(())
        }),
        var!("ORTHOMODE", |a| flag(a.engine.ortho()), |a, v| {
            a.engine.set_ortho(parse_flag(v)?);
            Ok(())
        }),
        var!(
            "OSMODE",
            |a| {
                let bits: u32 = OSMODE_BITS.iter().filter(|(k, _)| a.osnap_modes.contains(k)).map(|(_, b)| b).sum();
                (bits + if a.osnap { 0 } else { OSMODE_OFF }).to_string()
            },
            |a, v| {
                let bits = parse_int(v, 0, 32767)? as u32;
                a.osnap_modes = OSMODE_BITS.iter().filter(|(_, b)| bits & b != 0).map(|(k, _)| *k).collect();
                a.osnap = bits & OSMODE_OFF == 0 && bits != 0;
                Ok(())
            }
        ),
        var!("PDFFRAME", |a| a.doc.picture_frame(true).to_string(), |a, v| {
            a.doc.set_picture_frame(true, parse_int(v, 0, 2)? as u8);
            Ok(())
        }),
        var!("PDMODE", |a| a.doc.pdmode().to_string(), |a, v| {
            let size = a.doc.pdsize();
            a.doc.set_point_style(parse_int(v, 0, 100)? as i32, size);
            Ok(())
        }),
        var!("PDSIZE", |a| number(a.doc.pdsize()), |a, v| {
            let mode = a.doc.pdmode();
            a.doc.set_point_style(mode, parse_number(v)?);
            Ok(())
        }),
        var!("PICKSTYLE", |a| flag(a.engine.vars.group_select), |a, v| {
            a.engine.vars.group_select = parse_int(v, 0, 3)? & 1 != 0;
            Ok(())
        }),
        var!("PLINEWID", |a| number(a.engine.vars.pline_width), |a, v| {
            a.engine.vars.pline_width = parse_number(v)?.max(0.0);
            Ok(())
        }),
        var!("POLARANG", |a| number(a.engine.polar_increment), |a, v| {
            a.engine.polar_increment = parse_positive(v)?;
            Ok(())
        }),
        var!("POLARMODE", |a| flag(a.engine.polar()), |a, v| {
            a.engine.set_polar(parse_int(v, 0, 15)? != 0);
            Ok(())
        }),
        var!("POLYSIDES", |a| a.engine.vars.polygon_sides.to_string(), |a, v| {
            a.engine.vars.polygon_sides = parse_int(v, 3, 1024)? as usize;
            Ok(())
        }),
        var!("QTEXTMODE", |a| flag(a.doc.quick_text()), |a, v| {
            a.doc.set_quick_text(parse_flag(v)?);
            Ok(())
        }),
        var!("SNAPMODE", |a| flag(a.engine.vars.snap), |a, v| {
            a.engine.vars.snap = parse_flag(v)?;
            Ok(())
        }),
        var!("SNAPUNIT", |a| number(a.engine.vars.snap_spacing), |a, v| {
            a.engine.vars.snap_spacing = parse_positive(v)?;
            Ok(())
        }),
        var!("TEXTSIZE", |a| number(a.engine.vars.text_height), |a, v| {
            a.engine.vars.text_height = parse_positive(v)?;
            Ok(())
        }),
        var!("TEXTSTYLE", |a| a.doc.current_text_style().name, |a, v| a.doc.set_current_text_style(v.trim())),
        var!("TILEMODE", |a| flag(a.doc.space() == MODEL), |a, v| {
            if parse_flag(v)? {
                a.switch_space(MODEL);
            } else if let Some(first) = a.doc.layouts().first().map(|l| l.id) {
                a.switch_space(first);
            }
            Ok(())
        }),
        var!("VIEWCTR", |a| point(a.view.center)),
        var!("VIEWSIZE", |a| number(f64::from(a.drawing_rect.height()) / a.view.scale)),
        var!("WIPEOUTFRAME", |a| a.doc.wipeout_frame().to_string(), |a, v| {
            a.doc.set_wipeout_frame(parse_int(v, 0, 2)? as u8);
            Ok(())
        }),
    ]
}

/// The variable named `name`, if Oxidraft has it.
pub(crate) fn find(name: &str) -> Option<&'static str> {
    vars().into_iter().find(|v| v.name.eq_ignore_ascii_case(name.trim())).map(|v| v.name)
}

impl OxidraftApp {
    /// CANNOSCALE: the annotation scale, annotative objects sized for it
    /// (an undoable step).
    pub fn set_anno_scale(&mut self, scale: f64) {
        self.doc.begin("CANNOSCALE");
        let n = self.doc.set_anno_scale(scale, self.anno_autoscale);
        self.doc.commit();
        if n > 0 {
            self.engine
                .log(format!("{n} annotative object(s) sized for {}.", oxidraft_model::annoscale::scale_name(scale)));
        }
    }

    /// SETVAR, or a variable typed on its own: asks for its new value.
    pub(crate) fn setvar(&mut self, name: Option<&'static str>) {
        let values =
            vars().iter().map(|v| VarValue { name: v.name, value: (v.get)(self), writable: v.set.is_some() }).collect();
        self.engine.run_with("SETVAR", "", Box::new(SetVar::new(name, values)), &mut self.doc);
    }

    /// Sets a variable to what was typed.
    pub(crate) fn set_var(&mut self, name: &str, value: &str) {
        let Some(var) = vars().into_iter().find(|v| v.name.eq_ignore_ascii_case(name)) else { return };
        let Some(set) = var.set else { return };
        if let Err(e) = set(self, value) {
            self.engine.log(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_sorted_and_unique() {
        let names: Vec<&str> = vars().iter().map(|v| v.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted);
    }
}
