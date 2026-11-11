//! Offscreen PNG snapshots of the real UI, for eyeballing drawing changes without opening a window.
//!
//! `GDDBG_SNAP_DIR=some/dir cargo test snapshots -- --ignored --nocapture`
//!
//! egui is run headless, its output tessellated, and the triangles rasterised on the CPU (nearest
//! texel sampling, premultiplied alpha). Not pixel-exact with the GPU renderer, but close enough.

use std::collections::HashMap;
use std::io::Write;

use eframe::egui::{self, Color32, Pos2, Rect, TextureId, vec2};
use egui::epaint::{ImageData, ImageDelta, Primitive};

use super::*;

struct Tex {
    size: [usize; 2],
    px: Vec<Color32>,
}

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
    textures: HashMap<TextureId, Tex>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            px: vec![[0.0, 0.0, 0.0, 1.0]; w * h],
            textures: HashMap::new(),
        }
    }

    fn apply(&mut self, id: TextureId, delta: &ImageDelta) {
        let (size, px): ([usize; 2], Vec<Color32>) = match &delta.image {
            ImageData::Color(c) => (c.size, c.pixels.clone()),
            ImageData::Font(f) => (f.size, f.srgba_pixels(None).collect()),
        };
        match delta.pos {
            None => {
                self.textures.insert(id, Tex { size, px });
            }
            Some([x0, y0]) => {
                let t = self
                    .textures
                    .get_mut(&id)
                    .expect("partial update of unknown texture");
                for y in 0..size[1] {
                    for x in 0..size[0] {
                        t.px[(y0 + y) * t.size[0] + x0 + x] = px[y * size[0] + x];
                    }
                }
            }
        }
    }

    fn sample(&self, id: TextureId, u: f32, v: f32) -> [f32; 4] {
        let Some(t) = self.textures.get(&id) else {
            return [1.0; 4];
        };
        let x = ((u * t.size[0] as f32) as usize).min(t.size[0] - 1);
        let y = ((v * t.size[1] as f32) as usize).min(t.size[1] - 1);
        let c = t.px[y * t.size[0] + x];
        [
            c.r() as f32 / 255.0,
            c.g() as f32 / 255.0,
            c.b() as f32 / 255.0,
            c.a() as f32 / 255.0,
        ]
    }

    fn draw_mesh(&mut self, clip: Rect, mesh: &egui::Mesh) {
        let clip = clip.intersect(Rect::from_min_size(
            Pos2::ZERO,
            vec2(self.w as f32, self.h as f32),
        ));
        for tri in mesh.indices.chunks_exact(3) {
            let v = [
                mesh.vertices[tri[0] as usize],
                mesh.vertices[tri[1] as usize],
                mesh.vertices[tri[2] as usize],
            ];
            let (p0, p1, p2) = (v[0].pos, v[1].pos, v[2].pos);
            let area = (p1.x - p0.x) * (p2.y - p0.y) - (p2.x - p0.x) * (p1.y - p0.y);
            if area.abs() < 1e-6 {
                continue;
            }
            let x0 = p0.x.min(p1.x).min(p2.x).max(clip.left()).floor() as i32;
            let x1 = p0.x.max(p1.x).max(p2.x).min(clip.right()).ceil() as i32;
            let y0 = p0.y.min(p1.y).min(p2.y).max(clip.top()).floor() as i32;
            let y1 = p0.y.max(p1.y).max(p2.y).min(clip.bottom()).ceil() as i32;
            for y in y0.max(0)..y1.min(self.h as i32) {
                for x in x0.max(0)..x1.min(self.w as i32) {
                    let p = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((p1.x - p.x) * (p2.y - p.y) - (p2.x - p.x) * (p1.y - p.y)) / area;
                    let w1 = ((p2.x - p.x) * (p0.y - p.y) - (p0.x - p.x) * (p2.y - p.y)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let mut src = [0.0f32; 4];
                    for (k, s) in src.iter_mut().enumerate() {
                        let c = |vx: &egui::epaint::Vertex| vx.color.to_array()[k] as f32 / 255.0;
                        *s = c(&v[0]) * w0 + c(&v[1]) * w1 + c(&v[2]) * w2;
                    }
                    let u = v[0].uv.x * w0 + v[1].uv.x * w1 + v[2].uv.x * w2;
                    let vv = v[0].uv.y * w0 + v[1].uv.y * w1 + v[2].uv.y * w2;
                    let t = self.sample(mesh.texture_id, u, vv);
                    let dst = &mut self.px[y as usize * self.w + x as usize];
                    let a = src[3] * t[3];
                    for k in 0..3 {
                        dst[k] = src[k] * t[k] + dst[k] * (1.0 - a);
                    }
                }
            }
        }
    }

    fn write_png(&self, path: &std::path::Path) {
        let mut raw = Vec::with_capacity((self.w * 3 + 1) * self.h);
        for y in 0..self.h {
            raw.push(0u8);
            for x in 0..self.w {
                let p = self.px[y * self.w + x];
                raw.extend(
                    p[..3]
                        .iter()
                        .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8),
                );
            }
        }
        // zlib with stored (uncompressed) deflate blocks
        let mut z = vec![0x78, 0x01];
        for (i, chunk) in raw.chunks(65535).enumerate() {
            let last = (i + 1) * 65535 >= raw.len();
            z.push(last as u8);
            let n = chunk.len() as u16;
            z.extend(n.to_le_bytes());
            z.extend((!n).to_le_bytes());
            z.extend(chunk);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &c in &raw {
            a = (a + c as u32) % 65521;
            b = (b + a) % 65521;
        }
        z.extend(((b << 16) | a).to_be_bytes());

        let crc = |data: &[u8]| {
            let mut c = 0xffff_ffffu32;
            for &d in data {
                c ^= d as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 {
                        0xedb8_8320 ^ (c >> 1)
                    } else {
                        c >> 1
                    };
                }
            }
            !c
        };
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let mut chunk = |kind: &[u8; 4], data: &[u8]| {
            out.extend((data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend(data);
            out.extend(&body);
            out.extend(crc(&body).to_be_bytes());
        };
        let mut ihdr = Vec::new();
        ihdr.extend((self.w as u32).to_be_bytes());
        ihdr.extend((self.h as u32).to_be_bytes());
        ihdr.extend([8, 2, 0, 0, 0]);
        chunk(b"IHDR", &ihdr);
        chunk(b"IDAT", &z);
        chunk(b"IEND", &[]);
        std::fs::File::create(path)
            .unwrap()
            .write_all(&out)
            .unwrap();
    }
}

struct Snap {
    ctx: egui::Context,
    canvas: Canvas,
    app: App,
    time: f64,
}

impl Snap {
    fn new(app: App) -> Self {
        Self {
            ctx: egui::Context::default(),
            canvas: Canvas::new(1500, 900),
            app,
            time: 0.0,
        }
    }

    /// Runs `n` frames and rasterises the last one.
    fn frames(&mut self, n: usize, events: Vec<egui::Event>) {
        for i in 0..n {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1500.0, 900.0))),
                time: Some(self.time),
                events: if i == 0 { events.clone() } else { vec![] },
                ..Default::default()
            };
            let app = &mut self.app;
            let out = self.ctx.run(input, |ctx| app.show(ctx));
            for (id, delta) in &out.textures_delta.set {
                self.canvas.apply(*id, delta);
            }
            if i + 1 == n {
                self.canvas.px.fill([0.0, 0.0, 0.0, 1.0]);
                for prim in self.ctx.tessellate(out.shapes, out.pixels_per_point) {
                    if let Primitive::Mesh(m) = &prim.primitive {
                        self.canvas.draw_mesh(prim.clip_rect, m);
                    }
                }
            }
        }
    }

    fn save(&self, dir: &std::path::Path, name: &str) {
        let p = dir.join(format!("{name}.png"));
        self.canvas.write_png(&p);
        println!("wrote {}", p.display());
    }
}

#[test]
#[ignore]
fn snapshots() {
    let Ok(dir) = std::env::var("GDDBG_SNAP_DIR") else {
        println!("set GDDBG_SNAP_DIR to write snapshots");
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let level = |f: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../gdlib/test_gmds")
            .join(f);
        crate::load_level(&path, f.ends_with(".gdlvl")).expect("load test level")
    };

    let mut s = Snap::new(App::new(level("Chompstep.gmd")));
    s.frames(3, vec![]);
    s.save(&dir, "graph_fit");
    s.app.navigate(Nav::Group(12), true);
    s.app.g_zoom = 1.0;
    s.app.center_on(Nav::Group(12));
    s.frames(2, vec![]);
    s.save(&dir, "graph_selected");

    if let Some((&g, _)) = s
        .app
        .flow
        .iter()
        .filter(|(g, k)| **g != ROOT && **k >= FlowKind::Event)
        .min_by_key(|(g, _)| **g)
    {
        s.app.navigate(Nav::Group(g), true);
        s.frames(2, vec![]);
        s.save(&dir, "graph_branch");
    }

    // detailed graph around the branch
    s.app.graph_detail = true;
    s.frames(2, vec![]);
    s.app.g_zoom = 0.9;
    if let Some(g) = s.app.sel.group {
        s.app.center_on(Nav::Group(g));
    }
    s.frames(2, vec![]);
    s.save(&dir, "graph_detail");
    s.app.graph_detail = false;

    s.app.tab = Tab::Map;
    s.frames(2, vec![]);
    s.app.navigate(Nav::Group(12), true);
    s.app.navigate(Nav::Obj(7587), true);
    s.app.m_zoom = 1.2;
    s.app.center_on(Nav::Obj(7587));
    s.frames(3, vec![]);
    s.save(&dir, "map_selected");
    s.app.map_detail = true;
    s.frames(2, vec![]);
    s.save(&dir, "map_detail");

    // small levels: the whole flow at once
    for (f, name) in [
        ("generated_adv_random.gmd", "graph_advrand"),
        ("advrand test.gmd", "graph_advrand2"),
        ("generated_itemcompare.gmd", "graph_itemcompare"),
    ] {
        let mut s = Snap::new(App::new(level(f)));
        s.app.graph_detail = true;
        s.frames(3, vec![]);
        s.save(&dir, name);
    }

    // listing modes, on a level compiled by tasmc (if the tasm-lang repository sits next to this one)
    let tasm_level = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tasm-lang/example_programs/is_c1_prime.tasm.gmd");
    if let Ok(m) = crate::load_level(&tasm_level, false) {
        let mut s = Snap::new(App::new(m));
        s.app.graph_detail = true;
        s.app.listing_mode = Mode::Tasm;
        s.frames(3, vec![]);
        s.app.navigate(Nav::Group(4), true);
        s.app.g_zoom = 1.0;
        s.app.center_on(Nav::Group(4));
        s.frames(2, vec![]);
        s.save(&dir, "graph_tasm");
        s.app.listing_mode = Mode::Raw;
        s.frames(2, vec![]);
        s.save(&dir, "graph_raw");
        s.app.tab = Tab::Listing;
        for mode in Mode::ALL {
            s.app.listing_mode = mode;
            s.frames(2, vec![]);
            s.save(&dir, &format!("listing_{}", mode.name()));
        }
    }

    // the first level with text objects, zoomed on one of them
    for f in [
        "Chompstep.gmd",
        "level.gmd",
        "advrand test.gmd",
        "Jesse Pinkman 2.gdlvl",
        "Unnamed 45.gdlvl",
        "generated_sample_level.gmd",
    ] {
        let app = App::new(level(f));
        // lowest index, so every run renders the same text object
        let Some((&i, t)) = app.texts.iter().min_by_key(|(i, _)| **i) else {
            continue;
        };
        println!("{f}: {} text objects, e.g. #{i} {t:?}", app.texts.len());
        let mut s = Snap::new(app);
        s.app.tab = Tab::Map;
        s.frames(2, vec![]);
        s.app.m_zoom = 1.0;
        s.app.center_on(Nav::Obj(i));
        s.app.sel.obj = None;
        s.frames(3, vec![]);
        s.save(&dir, "map_text");
        break;
    }
}
