//! ADJUST: a picture's brightness, contrast and fade (and a PDF's
//! monochrome); IMAGEFRAME, PDFFRAME and FRAME: whether their frames show
//! and plot.

use oxidraft_model::{Document, EntityId, Geometry, PictureAdjust};

use crate::{Command, Ctx, Input, Prompt, Step};

#[derive(Clone, Copy, PartialEq)]
enum AdjustStage {
    Select,
    Option,
    Value(&'static str),
}

/// ADJUST: the selected images' brightness, contrast or fade, or the
/// selected PDF underlays' fade, contrast or monochrome.
pub struct Adjust {
    stage: AdjustStage,
    ids: Vec<EntityId>,
    pdf: bool,
    /// The first one's settings, the defaults offered.
    first: PictureAdjust,
}

impl Default for Adjust {
    fn default() -> Self {
        Self { stage: AdjustStage::Select, ids: Vec::new(), pdf: false, first: PictureAdjust::default() }
    }
}

impl Adjust {
    fn set(&self, ctx: &mut Ctx, f: impl Fn(&mut PictureAdjust)) {
        for id in &self.ids {
            ctx.doc.update(*id, |e| {
                if let Geometry::Image { adjust, .. } = &mut e.geometry {
                    f(adjust);
                }
            });
        }
    }
}

impl Command for Adjust {
    fn prompt(&self) -> Prompt {
        let a = &self.first;
        match self.stage {
            AdjustStage::Select => Prompt::new("Select image or underlay"),
            AdjustStage::Option if self.pdf => Prompt::new("Enter underlay adjust option")
                .with_keywords(&["Fade", "Contrast", "Monochrome"])
                .with_default("Fade"),
            AdjustStage::Option => Prompt::new("Enter image adjust option")
                .with_keywords(&["Brightness", "Contrast", "Fade"])
                .with_default("Brightness"),
            AdjustStage::Value("Monochrome") => Prompt::new("Monochrome?")
                .with_keywords(&["Yes", "No"])
                .with_default(if a.monochrome { "Yes" } else { "No" }),
            AdjustStage::Value(what) => {
                let now = match what {
                    "Brightness" => a.brightness,
                    "Contrast" => a.contrast,
                    _ => a.fade,
                };
                Prompt::new(format!("Enter {} value (0-100)", what.to_lowercase())).with_default(now.to_string())
            }
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (AdjustStage::Select, Input::Selection(s)) => {
                let pictures: Vec<(EntityId, bool, PictureAdjust)> = s
                    .ids
                    .iter()
                    .filter_map(|id| match ctx.doc.entity(*id).map(|e| &e.geometry) {
                        Some(Geometry::Image { image, adjust, .. }) => {
                            Some((*id, ctx.doc.image_def(image).is_some_and(|d| d.is_pdf()), *adjust))
                        }
                        _ => None,
                    })
                    .collect();
                let Some(first) = pictures.first() else {
                    ctx.messages.push("No images or underlays selected.".into());
                    return Ok(Step::Done);
                };
                // One kind at a time, as AutoCAD asks: the first one's.
                (self.pdf, self.first) = (first.1, first.2);
                self.ids = pictures.iter().filter(|p| p.1 == self.pdf).map(|p| p.0).collect();
                self.stage = AdjustStage::Option;
            }
            (AdjustStage::Option, Input::Keyword(k)) => self.stage = AdjustStage::Value(k),
            (AdjustStage::Option, Input::Enter) => {
                self.stage = AdjustStage::Value(if self.pdf { "Fade" } else { "Brightness" });
            }
            (AdjustStage::Value("Monochrome"), input @ (Input::Keyword(_) | Input::Enter)) => {
                let on = match input {
                    Input::Keyword(k) => k == "Yes",
                    _ => self.first.monochrome,
                };
                self.set(ctx, |a| a.monochrome = on);
                return Ok(Step::Done);
            }
            (AdjustStage::Value(what), Input::Value(v)) => {
                if !(0.0..=100.0).contains(&v) {
                    return Err("Requires a value from 0 to 100.".into());
                }
                let v = v.round() as u8;
                self.set(ctx, |a| match what {
                    "Brightness" => a.brightness = v,
                    "Contrast" => a.contrast = v,
                    _ => a.fade = v,
                });
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_selection(&self) -> bool {
        self.stage == AdjustStage::Select
    }
}

/// TRANSPARENCY: whether the selected images' transparent pixels show
/// what's under them.
#[derive(Default)]
pub struct ImageTransparency {
    ids: Vec<EntityId>,
}

impl Command for ImageTransparency {
    fn prompt(&self) -> Prompt {
        if self.ids.is_empty() {
            Prompt::new("Select image(s)")
        } else {
            Prompt::new("Enter transparency mode").with_keywords(&["ON", "OFF"]).with_default("OFF")
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.ids.is_empty(), input) {
            (true, Input::Selection(s)) => {
                self.ids = s
                    .ids
                    .into_iter()
                    .filter(|id| match ctx.doc.entity(*id).map(|e| &e.geometry) {
                        Some(Geometry::Image { image, .. }) => !ctx.doc.image_def(image).is_some_and(|d| d.is_pdf()),
                        _ => false,
                    })
                    .collect();
                if self.ids.is_empty() {
                    ctx.messages.push("No images selected.".into());
                    return Ok(Step::Done);
                }
                Ok(Step::Continue)
            }
            (false, input @ (Input::Keyword(_) | Input::Enter)) => {
                let on = input == Input::Keyword("ON");
                for id in &self.ids {
                    ctx.doc.update(*id, |e| {
                        if let Geometry::Image { adjust, .. } = &mut e.geometry {
                            adjust.transparent = on;
                        }
                    });
                }
                Ok(Step::Done)
            }
            _ => Err("Invalid input.".into()),
        }
    }

    fn wants_selection(&self) -> bool {
        self.ids.is_empty()
    }
}

/// Which frames a frame variable sets.
#[derive(Clone, Copy, PartialEq)]
pub enum Frames {
    Image,
    Pdf,
    /// FRAME: images', PDF underlays' and wipeouts' together.
    All,
}

/// IMAGEFRAME, PDFFRAME and FRAME: frames shown and plotted (1), shown
/// but not plotted (2), or hidden (0).
pub struct FrameVar {
    which: Frames,
    now: u8,
}

impl FrameVar {
    pub fn new(which: Frames) -> Self {
        Self { which, now: 1 }
    }

    fn name(&self) -> &'static str {
        match self.which {
            Frames::Image => "IMAGEFRAME",
            Frames::Pdf => "PDFFRAME",
            Frames::All => "FRAME",
        }
    }
}

impl Command for FrameVar {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        let (image, pdf, wipeout) = (doc.picture_frame(false), doc.picture_frame(true), doc.wipeout_frame());
        self.now = match self.which {
            Frames::Image => image,
            Frames::Pdf => pdf,
            // 3: they differ.
            Frames::All if image == pdf && pdf == wipeout => image,
            Frames::All => 3,
        };
        None
    }

    fn prompt(&self) -> Prompt {
        Prompt::new(format!("Enter new value for {}", self.name())).with_default(self.now.to_string())
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let v = match input {
            Input::Value(v) if [0.0, 1.0, 2.0].contains(&v) => v as u8,
            Input::Enter => return Ok(Step::Done),
            _ => return Err("Requires 0 (off), 1 (shown and plotted) or 2 (shown, not plotted).".into()),
        };
        if matches!(self.which, Frames::Image | Frames::All) {
            ctx.doc.set_picture_frame(false, v);
        }
        if matches!(self.which, Frames::Pdf | Frames::All) {
            ctx.doc.set_picture_frame(true, v);
        }
        if self.which == Frames::All {
            ctx.doc.set_wipeout_frame(v);
        }
        Ok(Step::Done)
    }
}

#[cfg(test)]
mod tests {
    use oxidraft_geom::Point;
    use oxidraft_model::{ImageDef, XrefStatus};

    use super::*;
    use crate::Engine;

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        for (i, s) in inputs.iter().enumerate() {
            if i == 0 {
                e.run(s, d);
            } else {
                e.submit(s, d, Point::ORIGIN, 0.5);
            }
        }
    }

    #[test]
    fn adjust_and_frames() {
        let mut d = Document::new();
        d.set_image_def(ImageDef {
            name: "site".into(),
            path: "site.png".into(),
            page: None,
            width: 100.0,
            height: 50.0,
            status: XrefStatus::Loaded,
        });
        let id = d.add(Geometry::Image {
            image: "site".into(),
            origin: Point::ORIGIN,
            u: Point::new(10.0, 0.0),
            v: Point::new(0.0, 5.0),
            clip: Vec::new(),
            adjust: PictureAdjust::default(),
        });
        let mut e = Engine::new();
        run(&mut e, &mut d, &["adjust", "all", "", "f", "40"]);
        run(&mut e, &mut d, &["adjust", "all", "", "", "70"]);
        match &d.entity(id).unwrap().geometry {
            Geometry::Image { adjust, .. } => assert_eq!((adjust.fade, adjust.brightness), (40, 70)),
            g => panic!("{g:?}"),
        }
        run(&mut e, &mut d, &["transparency", "all", "", "on"]);
        assert!(matches!(&d.entity(id).unwrap().geometry, Geometry::Image { adjust, .. } if adjust.transparent));
        run(&mut e, &mut d, &["imageframe", "0"]);
        assert_eq!((d.picture_frame(false), d.picture_frame(true)), (0, 1));
        run(&mut e, &mut d, &["frame", "2"]);
        assert_eq!((d.picture_frame(false), d.picture_frame(true), d.wipeout_frame()), (2, 2, 2));
    }

    #[test]
    fn adjusted_pixels() {
        let mut px = [200, 100, 0, 255];
        PictureAdjust { fade: 50, ..Default::default() }.apply(&mut px);
        assert_eq!(px, [228, 178, 128, 255], "halfway to white paper");
        let mut px = [200, 100, 0, 255];
        PictureAdjust { monochrome: true, ..Default::default() }.apply(&mut px);
        assert_eq!(px[0], px[1]);
    }
}
