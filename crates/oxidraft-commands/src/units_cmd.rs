//! -UNITS: the drawing's units at the command line, as AutoCAD asks for
//! them: length type and precision, angle type and precision, the
//! direction of angle 0 and whether angles go clockwise.

use oxidraft_model::{AngleType, LengthType, Units};

use crate::{Command, Ctx, Input, Prompt, Step};

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Length,
    LengthPrecision,
    Angle,
    AnglePrecision,
    Base,
    Clockwise,
}

pub struct UnitsCmd {
    stage: Stage,
    units: Units,
}

impl Default for UnitsCmd {
    fn default() -> Self {
        Self { stage: Stage::Length, units: Units::default() }
    }
}

/// The length types in AutoCAD's -UNITS order (1 to 5).
const LENGTHS: [LengthType; 5] = [
    LengthType::Scientific,
    LengthType::Decimal,
    LengthType::Engineering,
    LengthType::Architectural,
    LengthType::Fractional,
];

impl Command for UnitsCmd {
    fn prepare(&mut self, doc: &oxidraft_model::Document) -> Option<String> {
        self.units = doc.units();
        let lines = [
            "Report formats:          (Examples)",
            "  1. Scientific          1.55E+01",
            "  2. Decimal             15.50",
            "  3. Engineering         1'-3.50\"",
            "  4. Architectural       1'-3 1/2\"",
            "  5. Fractional          15 1/2",
        ];
        Some(lines.join("\n"))
    }

    fn prompt(&self) -> Prompt {
        let u = &self.units;
        match self.stage {
            Stage::Length => Prompt::new("Enter choice, 1 to 5").with_default(u.length.code().to_string()),
            Stage::LengthPrecision if matches!(u.length, LengthType::Architectural | LengthType::Fractional) => {
                Prompt::new("Enter denominator of smallest fraction to display (1, 2, 4, 8, 16, 32, 64, 128, or 256)")
                    .with_default((1u32 << u.length_precision).to_string())
            }
            Stage::LengthPrecision => Prompt::new("Enter number of digits to right of decimal point (0 to 8)")
                .with_default(u.length_precision.to_string()),
            Stage::Angle => Prompt::new(
                "Systems of angle measure:\n  1. Decimal degrees  45.0000\n  2. Degrees/minutes/seconds  45d0'0\"\n  \
                 3. Grads  50.0000g\n  4. Radians  0.7854r\n  5. Surveyor's units  N 45d0'0\" E\nEnter choice, 1 to 5",
            )
            .with_default((u.angle.code() + 1).to_string()),
            Stage::AnglePrecision => Prompt::new("Enter number of fractional places for display of angles (0 to 8)")
                .with_default(u.angle_precision.to_string()),
            Stage::Base => Prompt::new("Enter direction for angle 0").with_default(u.angle(u.base_angle)),
            Stage::Clockwise => Prompt::new("Measure angles clockwise?")
                .with_keywords(&["Yes", "No"])
                .with_default(if u.clockwise { "Y" } else { "N" }),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let whole = |v: f64, range: std::ops::RangeInclusive<i64>| {
            (v.fract() == 0.0 && range.contains(&(v as i64))).then_some(v as i64)
        };
        let u = &mut self.units;
        self.stage = match (self.stage, input) {
            (Stage::Length, Input::Value(v)) => {
                u.length = LENGTHS[whole(v, 1..=5).ok_or("Requires an integer between 1 and 5.")? as usize - 1];
                Stage::LengthPrecision
            }
            (Stage::LengthPrecision, Input::Value(v))
                if matches!(u.length, LengthType::Architectural | LengthType::Fractional) =>
            {
                let d = whole(v, 1..=256).filter(|d| (*d as u32).is_power_of_two());
                u.length_precision = d.ok_or("Requires 1, 2, 4, 8, 16, 32, 64, 128, or 256.")?.trailing_zeros() as u8;
                Stage::Angle
            }
            (Stage::LengthPrecision, Input::Value(v)) => {
                u.length_precision = whole(v, 0..=8).ok_or("Requires an integer between 0 and 8.")? as u8;
                Stage::Angle
            }
            (Stage::Angle, Input::Value(v)) => {
                u.angle =
                    AngleType::from_code(whole(v, 1..=5).ok_or("Requires an integer between 1 and 5.")? as i16 - 1);
                Stage::AnglePrecision
            }
            (Stage::AnglePrecision, Input::Value(v)) => {
                u.angle_precision = whole(v, 0..=8).ok_or("Requires an integer between 0 and 8.")? as u8;
                Stage::Base
            }
            (Stage::Base, Input::Value(v)) => {
                u.base_angle = v.rem_euclid(360.0);
                Stage::Clockwise
            }
            (Stage::Base, Input::Point(p)) => {
                u.base_angle = oxidraft_geom::Point::ORIGIN.angle_to(p);
                Stage::Clockwise
            }
            (Stage::Clockwise, input @ (Input::Keyword(_) | Input::Enter)) => {
                if let Input::Keyword(k) = input {
                    u.clockwise = k == "Yes";
                }
                ctx.doc.set_units(self.units);
                oxidraft_model::units::set_current(self.units);
                return Ok(Step::Done);
            }
            (stage, Input::Enter) => match stage {
                Stage::Length => Stage::LengthPrecision,
                Stage::LengthPrecision => Stage::Angle,
                Stage::Angle => Stage::AnglePrecision,
                Stage::AnglePrecision => Stage::Base,
                _ => Stage::Clockwise,
            },
            _ => return Err("Invalid input.".into()),
        };
        Ok(Step::Continue)
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{AngleType, Document, LengthType};

    #[test]
    fn units_at_the_command_line() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        e.dynamic_input = false;
        for t in ["-units", "4", "16", "2", "4", "90", "y"] {
            e.submit(t, &mut d, Point::ORIGIN, 0.5);
        }
        let u = d.units();
        assert_eq!((u.length, u.length_precision), (LengthType::Architectural, 4));
        assert_eq!((u.angle, u.angle_precision), (AngleType::DegMinSec, 4));
        assert!(u.clockwise && u.base_angle == 90.0);
        // Enter keeps each as it is.
        for t in ["-units", "", "", "", "", "", ""] {
            e.submit(t, &mut d, Point::ORIGIN, 0.5);
        }
        assert_eq!(d.units(), u);
    }
}
