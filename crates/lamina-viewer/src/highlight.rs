//! Net highlight: the copper of one net in full colour over a dimmed board,
//! as in other layout editors. The triangles are made once for each load of
//! the board, in board millimetres; each frame only moves them to the screen.

use eframe::egui::{self, Color32, Pos2};
use lamina_geom::fill::ZoneFill;
use lamina_geom::{CopperItem, Prim};
use lamina_model::Point;
use lamina_render::tess::Tris;

/// The strength of a pour fill, as a factor of the layer colour.
const POUR: f32 = 0.45;
/// The strength of copper on a layer that is not the active layer.
const OTHER: f32 = 0.35;

/// The triangles of one layer in a list, added if the layer is new.
fn tris_of(list: &mut Vec<(u8, Tris)>, layer: u8) -> &mut Tris {
    match list.iter().position(|(l, _)| *l == layer) {
        Some(i) => &mut list[i].1,
        None => {
            list.push((layer, Tris::default()));
            &mut list.last_mut().unwrap().1
        }
    }
}

pub struct NetHighlight {
    pub net: String,
    /// Triangles of the pads, tracks and vias for each copper layer.
    layers: Vec<(u8, Tris)>,
    /// Triangles of the pour fills for each copper layer. They are drawn
    /// lighter and first, as in the normal view.
    pours: Vec<(u8, Tris)>,
    /// The number of copper items of the net.
    pub items: usize,
}

impl NetHighlight {
    pub fn new(net: String, copper: &[CopperItem], fills: &[ZoneFill], layers: u8) -> NetHighlight {
        let mut h = NetHighlight {
            net,
            layers: Vec::new(),
            pours: Vec::new(),
            items: 0,
        };
        h.rebuild(copper, fills, layers);
        h
    }

    /// Make the triangles again, for example after the board changed.
    pub fn rebuild(&mut self, copper: &[CopperItem], fills: &[ZoneFill], layers: u8) {
        self.layers.clear();
        self.pours.clear();
        self.items = 0;
        let net = self.net.clone();
        // The pour fills of the net go first, under its pads and tracks.
        for f in fills.iter().filter(|f| f.net.as_deref() == Some(&net)) {
            let Some(c) = f.layer.copper() else { continue };
            for poly in &f.polygons {
                self.items += 1;
                tris_of(&mut self.pours, c.index(layers)).polygon(&poly.outer, &poly.holes);
            }
        }
        for it in copper.iter().filter(|c| c.net.as_deref() == Some(&net)) {
            self.items += 1;
            let tris = tris_of(&mut self.layers, it.layer);
            match &it.prim {
                Prim::Poly { outer, holes } => tris.polygon(outer, holes),
                p => tris.polygon(&p.outline(0.01), &[]),
            }
        }
        // Draw the deepest layer first, so that the top copper is on top.
        self.layers.sort_by_key(|(l, _)| std::cmp::Reverse(*l));
        self.pours.sort_by_key(|(l, _)| std::cmp::Reverse(*l));
    }

    /// Dim the canvas and draw the net on it. `color` gives the colour of a
    /// copper layer, or `None` if the layer is hidden. Copper on other layers
    /// than `bright` is dimmer, as in the normal view.
    pub fn draw(
        &self,
        painter: &egui::Painter,
        canvas: egui::Rect,
        to_screen: impl Fn(Point) -> Pos2,
        color: impl Fn(u8) -> Option<Color32>,
        bright: u8,
    ) {
        painter.rect_filled(canvas, 0.0, Color32::from_black_alpha(165));
        let pours = self.pours.iter().map(|(l, t)| (l, t, POUR));
        let items = self.layers.iter().map(|(l, t)| (l, t, 1.0));
        for (layer, t, k) in pours.chain(items) {
            let Some(c) = color(*layer) else { continue };
            let c = c.gamma_multiply(if *layer == bright { k } else { k * OTHER });
            let mut mesh = egui::Mesh::default();
            for p in &t.pos {
                mesh.colored_vertex(to_screen(Point::new(p[0] as f64, p[1] as f64)), c);
            }
            mesh.indices = t.idx.clone();
            painter.add(egui::Shape::mesh(mesh));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamina_geom::Owner;

    fn item(layer: u8, net: &str, prim: Prim) -> CopperItem {
        CopperItem {
            layer,
            net: Some(net.into()),
            prim,
            owner: Owner::Pad {
                part: "U1".into(),
                pad: "1".into(),
                index: 0,
            },
        }
    }

    #[test]
    fn only_the_net_and_the_top_layer_last() {
        let copper = vec![
            item(
                0,
                "GND",
                Prim::Circle {
                    c: Point::new(0.0, 0.0),
                    r: 0.5,
                },
            ),
            item(
                1,
                "GND",
                Prim::Capsule {
                    a: Point::new(0.0, 0.0),
                    b: Point::new(2.0, 0.0),
                    r: 0.1,
                },
            ),
            item(
                0,
                "VCC",
                Prim::Circle {
                    c: Point::new(5.0, 0.0),
                    r: 0.5,
                },
            ),
        ];
        let h = NetHighlight::new("GND".into(), &copper, &[], 2);
        assert_eq!(h.items, 2);
        let order: Vec<u8> = h.layers.iter().map(|(l, _)| *l).collect();
        assert_eq!(order, [1, 0]);
        assert!(h.layers.iter().all(|(_, t)| !t.idx.is_empty()));
    }
}
