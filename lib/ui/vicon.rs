// vicon -- vector icons from SVG path data, for styles (Style::paint_icon) and app icon hooks
//
//   paint_svg_icon(g, art, x, y, size_fx, color, stroke) -> drawn
//        art: SVG path data on a 24 x 24 grid; several layers are separated by '|', a layer
//        that starts with 'F' is filled (non-zero rule), the others are stroked with round
//        caps and joins `stroke` grid units wide (1.75 is a good outline weight); 'E' fills
//        with the even-odd rule (cut-outs: a mark inside a filled disc).  The icon's
//        em box has its top-left corner at (x, y) target pixels and is size_fx (24.8) pixels
//        square.  A layer that starts with 'A' is drawn in the accent colour (second colour).
//   paint_svg_icon2(.., color, accent, stroke)   the same with an accent colour for 'A' layers
//
// Icons are geometry, not font glyphs: crisp at every size and dpi, and free of the icon
// font's licence.  Parsing is cheap (a few dozen commands per icon).

use gfx::pathdata::PData;
use gfx::xform::Matrix;

use crate::cmd::Gfx;

/// paint an icon from 24-grid SVG path data (see the module comment)
pub fn paint_svg_icon(g: &mut Gfx, art: &str, x: i32, y: i32, size_fx: i32, color: u32, stroke: f32) -> bool {
    paint_svg_icon2(g, art, x, y, size_fx, color, color, stroke)
}

/// paint_svg_icon with a second (accent) colour for the layers that start with 'A'
#[allow(clippy::too_many_arguments)]
pub fn paint_svg_icon2(g: &mut Gfx, art: &str, x: i32, y: i32, size_fx: i32, color: u32, accent: u32, stroke: f32) -> bool {
    if size_fx <= 0 {
        return true;
    }
    let s = size_fx as f64 / 256.0 / 24.0;
    let m = Matrix { a: s, b: 0.0, c: x as f64, d: 0.0, e: s, f: y as f64 };
    // at least ~1 px wide so small icons stay visible
    let w = ((stroke as f64 * s).max(1.0) * 256.0) as i32;
    let mut pd = PData::new();
    for layer in art.split('|') {
        let layer = layer.trim();
        // fill: 0 stroke, 1 non-zero fill, 2 even-odd fill
        let (acc, layer) = match layer.as_bytes().first() {
            Some(b'A') => (true, layer[1..].trim_start()),
            _ => (false, layer),
        };
        let (fill, d) = match layer.as_bytes().first() {
            Some(b'F') => (1, &layer[1..]),
            Some(b'E') => (2, &layer[1..]),
            _ => (0, layer),
        };
        pd.clear();
        let (err, _) = pd.parse(d.as_bytes());
        if err != 0 {
            continue;
        }
        let path = &mut g.text.path;
        path.reset();
        path.set_origin(0, 0);
        if pd.emit(path, Some(&m)) != 0 {
            continue;
        }
        g.dc.set_color(if acc { accent } else { color });
        if fill != 0 {
            path.fill(&mut g.text.ras, g.dc, g.sf, if fill == 2 { gfx::RULE_EVENODD } else { gfx::RULE_NONZERO });
        } else {
            path.stroke(&mut g.text.ras, g.dc, g.sf, w, gfx::JOIN_ROUND | gfx::CAP_ROUND << 4);
        }
    }
    true
}
