//! Layouts on the way out: the DXF written for the drawing has each
//! layout's objects in its paper space block (*Paper_Space, then
//! *Paper_Space0, …); this gives them their LAYOUT objects (name, tab
//! order, page setup) and viewports before the file is written.

use acadrust::CadDocument;
use acadrust::entities::{EntityType, Viewport, ViewportStatusFlags};
use acadrust::objects::{Layout, ObjectType, PlotFlags};
use acadrust::types::Vector3;
use oxidraft_model::{Document, Geometry, Paper};

/// The page setup's fields acadrust writes to DWG (DXF replays `raw`).
fn set_paper(l: &mut Layout, p: &Paper) {
    let text = |code: i32| p.raw.iter().find(|r| r.0 == code).map(|r| r.1.clone());
    let num = |code: i32| text(code).and_then(|v| v.trim().parse::<f64>().ok());
    l.paper_size = p.size_name.clone();
    (l.paper_width, l.paper_height) = (p.width, p.height);
    [l.plot_margin_left, l.plot_margin_bottom, l.plot_margin_right, l.plot_margin_top] = p.margins;
    l.plot_paper_units = i16::from(p.millimeters);
    l.plot_rotation = i16::from(p.rotation);
    if p.raw.is_empty() {
        return;
    }
    l.raw_plot_settings_codes = Some(p.raw.clone());
    l.plot_page_name = text(1).unwrap_or_default();
    l.plot_printer_name = text(2).unwrap_or_default();
    l.plot_view_name = text(6).unwrap_or_default();
    l.plot_style_sheet = text(7).unwrap_or_default();
    l.plot_origin_x = num(46).unwrap_or(0.0);
    l.plot_origin_y = num(47).unwrap_or(0.0);
    l.plot_window_min_x = num(48).unwrap_or(0.0);
    l.plot_window_min_y = num(49).unwrap_or(0.0);
    l.plot_window_max_x = num(140).unwrap_or(0.0);
    l.plot_window_max_y = num(141).unwrap_or(0.0);
    l.plot_scale_numerator = num(142).unwrap_or(1.0);
    l.plot_scale_denominator = num(143).unwrap_or(1.0);
    l.plot_flags = PlotFlags::from_bits(num(70).unwrap_or(0.0) as i32);
    l.plot_type = num(74).unwrap_or(0.0) as i16;
    l.plot_scale_type = num(75).unwrap_or(0.0) as i16;
    l.plot_scale_factor = num(147).unwrap_or(1.0);
    l.paper_image_origin_x = num(148).unwrap_or(0.0);
    l.paper_image_origin_y = num(149).unwrap_or(0.0);
}

/// Gives `doc`'s layouts their LAYOUT objects and viewports in `cad`.
pub(crate) fn finish(cad: &mut CadDocument, doc: &Document) {
    for (i, layout) in doc.layouts().into_iter().enumerate() {
        let record_name = if i == 0 { "*Paper_Space".to_string() } else { format!("*Paper_Space{}", i - 1) };
        let Some(record) = cad.block_records.iter().find(|r| r.name.eq_ignore_ascii_case(&record_name)) else {
            continue;
        };
        let record = record.handle;
        // The LAYOUT already made for the record, or a new one.
        let existing = cad.objects.iter().find_map(|(h, o)| match o {
            ObjectType::Layout(l) if l.block_record == record => Some(*h),
            _ => None,
        });
        let handle = existing.unwrap_or_else(|| {
            let handle = cad.allocate_handle();
            let mut l = Layout::new(layout.name.clone());
            l.handle = handle;
            l.owner = cad.header.acad_layout_dict_handle;
            l.block_record = record;
            cad.objects.insert(handle, ObjectType::Layout(l));
            if let Some(ObjectType::Dictionary(d)) = cad.objects.get_mut(&cad.header.acad_layout_dict_handle) {
                d.add_entry(layout.name.clone(), handle);
            }
            if let Some(r) = cad.block_records.iter_mut().find(|r| r.handle == record) {
                r.layout = handle;
            }
            handle
        });
        let (w, h) = layout.paper.sheet();
        if let Some(ObjectType::Layout(l)) = cad.objects.get_mut(&handle) {
            let old = std::mem::replace(&mut l.name, layout.name.clone());
            l.tab_order = layout.tab_order.clamp(1, i32::from(i16::MAX)) as i16;
            set_paper(l, &layout.paper);
            (l.min_limits, l.max_limits) = ((0.0, 0.0), (w, h));
            // Renamed: its dictionary entry too.
            if old != layout.name
                && let Some(ObjectType::Dictionary(d)) = cad.objects.get_mut(&cad.header.acad_layout_dict_handle)
            {
                d.entries.retain(|(_, e)| *e != handle);
                d.add_entry(layout.name.clone(), handle);
            }
        }
        // The layout's own viewport, framing the sheet.
        let has_own = cad.entities_in_block(&record_name).any(|e| matches!(e, EntityType::Viewport(v) if v.id == 1));
        if !has_own {
            let mut own = Viewport::new();
            own.id = 1;
            own.center = Vector3::new(w / 2.0, h / 2.0, 0.0);
            (own.width, own.height) = (w.max(1.0), h.max(1.0));
            own.view_center = own.center;
            own.view_height = own.height;
            if let Ok(vp) = cad.add_entity_to_layout(EntityType::Viewport(Box::new(own)), &layout.name)
                && let Some(ObjectType::Layout(l)) = cad.objects.get_mut(&handle)
            {
                l.viewport = vp;
            }
        }
        // The viewports onto model space.
        let mut id = 2;
        for e in doc.space_in_draw_order(layout.id) {
            let Geometry::Viewport { center, width, height, view_center, view_height, twist, frozen, locked, on } =
                &e.geometry
            else {
                continue;
            };
            let mut vp = Viewport::new();
            vp.id = id;
            id += 1;
            vp.center = Vector3::new(center.x, center.y, 0.0);
            (vp.width, vp.height) = (*width, *height);
            vp.view_center = Vector3::new(view_center.x, view_center.y, 0.0);
            vp.view_height = *view_height;
            vp.twist_angle = twist.to_radians();
            vp.status =
                ViewportStatusFlags { is_on: *on, is_off: !*on, locked: *locked, ..ViewportStatusFlags::default_on() };
            vp.frozen_layers = frozen
                .iter()
                .filter_map(|n| cad.layers.iter().find(|l| l.name.eq_ignore_ascii_case(n)).map(|l| l.handle))
                .collect();
            vp.common.layer = e.layer.clone();
            let _ = cad.add_entity_to_layout(EntityType::Viewport(Box::new(vp)), &layout.name);
        }
    }
}
