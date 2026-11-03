//! Custom device models from glTF / GLB files: loading, finding and framing the screen, and a
//! small software rasterizer so the model renders through the same compositor as everything else.

use rayon::prelude::*;
use std::sync::Arc;

pub struct Texture {
    w: usize,
    h: usize,
    data: Vec<u8>,
}

pub struct Material {
    pub name: String,
    /// sRGB, 0..255.
    base: [f32; 3],
    alpha: f32,
    texture: Option<usize>,
    tex_coord: u32,
    metallic: f32,
    roughness: f32,
    emissive: [f32; 3],
    blend: bool,
    cutoff: Option<f32>,
}

/// A model as loaded: world-space triangles with their materials.
pub struct ModelSource {
    pub file_name: String,
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    tris: Vec<[u32; 3]>,
    tri_mat: Vec<u32>,
    pub materials: Arc<Vec<Material>>,
    textures: Arc<Vec<Texture>>,
    /// Best guess for the screen material.
    pub guess: Option<usize>,
}

impl ModelSource {
    pub fn triangle_count(&self) -> usize {
        self.tris.len()
    }

    /// Materials that are actually used, for the "screen" picker.
    pub fn used_materials(&self) -> Vec<usize> {
        let mut used = vec![false; self.materials.len()];
        for m in &self.tri_mat {
            used[*m as usize] = true;
        }
        (0..used.len()).filter(|i| used[*i]).collect()
    }
}

type M4 = [[f32; 4]; 4];

fn mul4(a: &M4, b: &M4) -> M4 {
    // Column-major, as glTF stores them.
    let mut m = [[0.0; 4]; 4];
    for c in 0..4 {
        for r in 0..4 {
            m[c][r] = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    m
}

fn xf_point(m: &M4, p: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0],
        m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1],
        m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2],
    ]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn to_srgb(linear: f32) -> f32 {
    linear.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0
}

fn to_rgba(img: &gltf::image::Data) -> Texture {
    use gltf::image::Format;
    let n = (img.width * img.height) as usize;
    let mut data = Vec::with_capacity(n * 4);
    let px = &img.pixels;
    for i in 0..n {
        let (r, g, b, a) = match img.format {
            Format::R8 => (px[i], px[i], px[i], 255),
            Format::R8G8 => (px[2 * i], px[2 * i], px[2 * i], px[2 * i + 1]),
            Format::R8G8B8 => (px[3 * i], px[3 * i + 1], px[3 * i + 2], 255),
            Format::R8G8B8A8 => (px[4 * i], px[4 * i + 1], px[4 * i + 2], px[4 * i + 3]),
            Format::R16G16B16 => (px[6 * i + 1], px[6 * i + 3], px[6 * i + 5], 255),
            Format::R16G16B16A16 => (px[8 * i + 1], px[8 * i + 3], px[8 * i + 5], px[8 * i + 7]),
            _ => (200, 200, 200, 255),
        };
        data.extend_from_slice(&[r, g, b, a]);
    }
    Texture { w: img.width as usize, h: img.height as usize, data }
}

const SCREEN_WORDS: &[&str] = &["screen", "display", "pantalla", "lcd", "oled", "wallpaper", "bildschirm", "ecran", "écran"];

fn looks_like_screen(name: Option<&str>) -> bool {
    name.map(|n| {
        let n = n.to_lowercase();
        SCREEN_WORDS.iter().any(|w| n.contains(w))
    })
    .unwrap_or(false)
}

pub fn load(path: &str) -> Result<ModelSource, String> {
    let (doc, buffers, images) = gltf::import(path).map_err(|e| {
        let msg = e.to_string();
        if msg.to_lowercase().contains("draco") {
            "El modelo usa compresión Draco, que no se puede leer. Expórtalo de nuevo sin compresión (en Blender, desmarca «Compresión» al exportar a glTF).".to_string()
        } else {
            format!("No se pudo abrir el modelo: {msg}")
        }
    })?;
    let textures: Vec<Texture> = images.iter().map(to_rgba).collect();

    let mut materials: Vec<Material> = doc
        .materials()
        .enumerate()
        .map(|(i, m)| {
            let pbr = m.pbr_metallic_roughness();
            let f = pbr.base_color_factor();
            let tex = pbr.base_color_texture();
            let e = m.emissive_factor();
            Material {
                name: m.name().map(str::to_string).unwrap_or_else(|| format!("Material {}", i + 1)),
                base: [to_srgb(f[0]), to_srgb(f[1]), to_srgb(f[2])],
                alpha: f[3],
                texture: tex.as_ref().map(|t| t.texture().source().index()),
                tex_coord: tex.as_ref().map(|t| t.tex_coord()).unwrap_or(0),
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
                emissive: [to_srgb(e[0]), to_srgb(e[1]), to_srgb(e[2])],
                blend: m.alpha_mode() == gltf::material::AlphaMode::Blend,
                cutoff: (m.alpha_mode() == gltf::material::AlphaMode::Mask).then(|| m.alpha_cutoff().unwrap_or(0.5)),
            }
        })
        .collect();
    let default_mat = materials.len();
    materials.push(Material {
        name: "Sin material".into(),
        base: [200.0, 200.0, 200.0],
        alpha: 1.0,
        texture: None,
        tex_coord: 0,
        metallic: 0.0,
        roughness: 0.6,
        emissive: [0.0; 3],
        blend: false,
        cutoff: None,
    });

    let mut src = ModelSource {
        file_name: std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        pos: Vec::new(),
        nrm: Vec::new(),
        uv: Vec::new(),
        tris: Vec::new(),
        tri_mat: Vec::new(),
        materials: Arc::new(Vec::new()),
        textures: Arc::new(Vec::new()),
        guess: None,
    };
    let mut named_screen_area = vec![0.0f32; materials.len()];

    let scene = doc.default_scene().or_else(|| doc.scenes().next()).ok_or("El archivo no tiene ninguna escena")?;
    let identity: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let mut stack: Vec<(gltf::Node, M4)> = scene.nodes().map(|n| (n, identity)).collect();
    while let Some((node, parent)) = stack.pop() {
        let m = mul4(&parent, &node.transform().matrix());
        for child in node.children() {
            stack.push((child, m));
        }
        let Some(mesh) = node.mesh() else { continue };
        // Normals transform with the inverse transpose (cofactors); mirrored nodes flip winding.
        let c = [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
        let cof = [cross(c[1], c[2]), cross(c[2], c[0]), cross(c[0], c[1])];
        let det = dot(c[0], cof[0]);
        let flip = det < 0.0;

        for prim in mesh.primitives() {
            let reader = prim.reader(|b| Some(&buffers[b.index()]));
            let Some(positions) = reader.read_positions() else { continue };
            let positions: Vec<[f32; 3]> = positions.collect();
            let mat_idx = prim.material().index().unwrap_or(default_mat);
            let tex_set = materials[mat_idx].tex_coord;
            let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
            let uvs: Option<Vec<[f32; 2]>> = reader.read_tex_coords(tex_set).map(|t| t.into_f32().collect());
            let idx: Vec<u32> = reader
                .read_indices()
                .map(|i| i.into_u32().collect())
                .unwrap_or_else(|| (0..positions.len() as u32).collect());

            let mut tri_idx: Vec<[u32; 3]> = Vec::new();
            match prim.mode() {
                gltf::mesh::Mode::Triangles => tri_idx.extend(idx.chunks_exact(3).map(|t| [t[0], t[1], t[2]])),
                gltf::mesh::Mode::TriangleStrip => {
                    for i in 2..idx.len() {
                        tri_idx.push(if i % 2 == 0 { [idx[i - 2], idx[i - 1], idx[i]] } else { [idx[i - 1], idx[i - 2], idx[i]] });
                    }
                }
                gltf::mesh::Mode::TriangleFan => {
                    for i in 2..idx.len() {
                        tri_idx.push([idx[0], idx[i - 1], idx[i]]);
                    }
                }
                _ => continue,
            }

            let base = src.pos.len() as u32;
            for (i, p) in positions.iter().enumerate() {
                src.pos.push(xf_point(&m, *p));
                let n = normals.as_ref().map(|n| n[i]).unwrap_or([0.0; 3]);
                let tn = [
                    n[0] * cof[0][0] + n[1] * cof[1][0] + n[2] * cof[2][0],
                    n[0] * cof[0][1] + n[1] * cof[1][1] + n[2] * cof[2][1],
                    n[0] * cof[0][2] + n[1] * cof[1][2] + n[2] * cof[2][2],
                ];
                let tn = if flip { [-tn[0], -tn[1], -tn[2]] } else { tn };
                src.nrm.push(if normals.is_some() { norm(tn) } else { [0.0; 3] });
                src.uv.push(uvs.as_ref().map(|u| u[i]).unwrap_or([0.0; 2]));
            }
            let named = looks_like_screen(prim.material().name()) || looks_like_screen(mesh.name()) || looks_like_screen(node.name());
            for t in tri_idx {
                if t.iter().any(|&i| i as usize >= positions.len()) {
                    continue;
                }
                let t = if flip { [t[0], t[2], t[1]] } else { t };
                let tri = [base + t[0], base + t[1], base + t[2]];
                // Missing normals: use the face normal.
                if normals.is_none() {
                    let (a, b, c) = (src.pos[tri[0] as usize], src.pos[tri[1] as usize], src.pos[tri[2] as usize]);
                    let fnrm = norm(cross(sub(b, a), sub(c, a)));
                    for &v in &tri {
                        src.nrm[v as usize] = fnrm;
                    }
                }
                if named {
                    let (a, b, c) = (src.pos[tri[0] as usize], src.pos[tri[1] as usize], src.pos[tri[2] as usize]);
                    named_screen_area[mat_idx] += dot(cross(sub(b, a), sub(c, a)), cross(sub(b, a), sub(c, a))).sqrt();
                }
                src.tris.push(tri);
                src.tri_mat.push(mat_idx as u32);
            }
        }
    }
    if src.tris.is_empty() {
        return Err("El modelo no tiene triángulos".into());
    }

    src.guess = named_screen_area
        .iter()
        .enumerate()
        .filter(|(_, a)| **a > 0.0)
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, _)| i)
        .or_else(|| guess_planar_dark(&src, &materials));
    src.materials = Arc::new(materials);
    src.textures = Arc::new(textures);
    Ok(src)
}

/// Without a telling name: among the large flat materials, the screen is the one that sits
/// furthest out along its own normal (in front of the bezel), preferably dark or textured.
fn guess_planar_dark(src: &ModelSource, materials: &[Material]) -> Option<usize> {
    let n = materials.len();
    let mut area = vec![0.0f32; n];
    let mut nsum = vec![[0.0f32; 3]; n];
    let mut csum = vec![[0.0f32; 3]; n];
    for (t, m) in src.tris.iter().zip(&src.tri_mat) {
        let (a, b, c) = (src.pos[t[0] as usize], src.pos[t[1] as usize], src.pos[t[2] as usize]);
        let cr = cross(sub(b, a), sub(c, a));
        let ar = dot(cr, cr).sqrt();
        let m = *m as usize;
        area[m] += ar;
        for k in 0..3 {
            nsum[m][k] += cr[k];
            csum[m][k] += ar * (a[k] + b[k] + c[k]) / 3.0;
        }
    }
    let count = src.pos.len() as f32;
    let centroid = src.pos.iter().fold([0.0f32; 3], |acc, p| [acc[0] + p[0] / count, acc[1] + p[1] / count, acc[2] + p[2] / count]);
    let planar: Vec<usize> = (0..n).filter(|&i| area[i] > 0.0 && dot(nsum[i], nsum[i]).sqrt() / area[i] > 0.95).collect();
    let biggest = planar.iter().map(|&i| area[i]).fold(0.0f32, f32::max);
    planar
        .into_iter()
        .filter(|&i| area[i] >= 0.3 * biggest)
        .max_by(|&a, &b| {
            let score = |i: usize| {
                let c = [csum[i][0] / area[i], csum[i][1] / area[i], csum[i][2] / area[i]];
                let out = dot(sub(c, centroid), norm(nsum[i]));
                let col = materials[i].base;
                let dark = col[0] + col[1] + col[2] < 180.0 || materials[i].texture.is_some();
                out * if dark { 1.0 } else { 0.8 }
            };
            score(a).total_cmp(&score(b))
        })
}

/// Size of the model around its screen, in units of the screen width, with the screen centred
/// on the origin, facing +z, its top towards -y.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelMeta {
    pub screen_aspect: f32,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
    pub z0: f32,
    pub z1: f32,
}

/// A model normalized around its screen and ready to draw.
pub struct PreparedModel {
    pub meta: ModelMeta,
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    tris: Vec<[u32; 3]>,
    tri_mat: Vec<u32>,
    materials: Arc<Vec<Material>>,
    textures: Arc<Vec<Texture>>,
    screen: u32,
    screen_h: f32,
}

/// Puts the model in device space: finds the screen's plane and long axis, turns the model so
/// the screen faces the camera upright, and scales it so the screen is 1 unit wide.
/// `quarter_turns` rotates the screen by 90° steps for models whose "up" was guessed wrong.
pub fn prepare(src: &ModelSource, screen: usize, quarter_turns: u8) -> Result<PreparedModel, String> {
    let mut nsum = [0.0f32; 3];
    let mut csum = [0.0f32; 3];
    let mut area = 0.0f32;
    let mut screen_verts: Vec<u32> = Vec::new();
    for (t, m) in src.tris.iter().zip(&src.tri_mat) {
        if *m as usize != screen {
            continue;
        }
        let (a, b, c) = (src.pos[t[0] as usize], src.pos[t[1] as usize], src.pos[t[2] as usize]);
        let cr = cross(sub(b, a), sub(c, a));
        let ar = dot(cr, cr).sqrt();
        for k in 0..3 {
            nsum[k] += cr[k];
            csum[k] += ar * (a[k] + b[k] + c[k]) / 3.0;
        }
        area += ar;
        screen_verts.extend_from_slice(t);
    }
    if area <= 0.0 {
        return Err("Ese material no tiene superficie visible".into());
    }
    let center = [csum[0] / area, csum[1] / area, csum[2] / area];
    let mut z = norm(nsum);

    // The rest of the phone should sit behind the screen.
    let all_c = src.pos.iter().fold([0.0f32; 3], |acc, p| [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]]);
    let n = src.pos.len() as f32;
    if dot(sub([all_c[0] / n, all_c[1] / n, all_c[2] / n], center), z) > 0.0 {
        z = [-z[0], -z[1], -z[2]];
    }

    // Long axis of the screen (principal component within its plane).
    let helper = if z[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let a = norm(cross(helper, z));
    let b = cross(z, a);
    let (mut sxx, mut sxy, mut syy) = (0.0f32, 0.0f32, 0.0f32);
    for &v in &screen_verts {
        let d = sub(src.pos[v as usize], center);
        let (u, w) = (dot(d, a), dot(d, b));
        sxx += u * u;
        sxy += u * w;
        syy += w * w;
    }
    let theta = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let mut long = norm([
        a[0] * theta.cos() + b[0] * theta.sin(),
        a[1] * theta.cos() + b[1] * theta.sin(),
        a[2] * theta.cos() + b[2] * theta.sin(),
    ]);
    // glTF is Y-up: the top of the screen is the end that points up.
    let up_ref = if long[1].abs() > 0.2 { [0.0, 1.0, 0.0] } else { [0.0, 0.0, -1.0] };
    if dot(long, up_ref) < 0.0 {
        long = [-long[0], -long[1], -long[2]];
    }
    let mut y = [-long[0], -long[1], -long[2]];
    let mut x = cross(y, z);
    for _ in 0..(quarter_turns % 4) {
        let (nx, ny) = (y, [-x[0], -x[1], -x[2]]);
        x = nx;
        y = ny;
    }

    let to_dev = |p: [f32; 3]| {
        let d = sub(p, center);
        [dot(d, x), dot(d, y), dot(d, z)]
    };
    let mut pos: Vec<[f32; 3]> = src.pos.par_iter().map(|p| to_dev(*p)).collect();
    let nrm: Vec<[f32; 3]> = src.nrm.par_iter().map(|n| [dot(*n, x), dot(*n, y), dot(*n, z)]).collect();

    let (mut sx0, mut sx1, mut sy0, mut sy1, mut sz1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN, f32::MIN);
    for &v in &screen_verts {
        let p = pos[v as usize];
        sx0 = sx0.min(p[0]);
        sx1 = sx1.max(p[0]);
        sy0 = sy0.min(p[1]);
        sy1 = sy1.max(p[1]);
        sz1 = sz1.max(p[2]);
    }
    let sw = (sx1 - sx0).max(1e-9);
    let sh = (sy1 - sy0).max(1e-9);
    let (cx, cy) = ((sx0 + sx1) * 0.5, (sy0 + sy1) * 0.5);
    let mut bounds = [f32::MAX, f32::MIN, f32::MAX, f32::MIN, f32::MAX, f32::MIN];
    for p in pos.iter_mut() {
        *p = [(p[0] - cx) / sw, (p[1] - cy) / sw, (p[2] - sz1) / sw];
        bounds[0] = bounds[0].min(p[0]);
        bounds[1] = bounds[1].max(p[0]);
        bounds[2] = bounds[2].min(p[1]);
        bounds[3] = bounds[3].max(p[1]);
        bounds[4] = bounds[4].min(p[2]);
        bounds[5] = bounds[5].max(p[2]);
    }

    Ok(PreparedModel {
        meta: ModelMeta {
            screen_aspect: sw / sh,
            x0: bounds[0],
            x1: bounds[1],
            y0: bounds[2],
            y1: bounds[3],
            z0: bounds[4],
            z1: bounds[5],
        },
        pos,
        nrm,
        uv: src.uv.clone(),
        tris: src.tris.clone(),
        tri_mat: src.tri_mat.clone(),
        materials: src.materials.clone(),
        textures: src.textures.clone(),
        screen: screen as u32,
        screen_h: sh / sw,
    })
}

// ---------- Rasterizer ----------

/// Light used for the model (directions in device/world space, unit length).
pub struct Lighting {
    pub light: [f32; 3],
    pub half: [f32; 3],
}

/// The model projected for one frame, with triangles sorted into horizontal bands.
pub struct Projected<'a> {
    model: &'a PreparedModel,
    sx: Vec<f32>,
    sy: Vec<f32>,
    iw: Vec<f32>,
    nw: Vec<[f32; 3]>,
    bins: Vec<Vec<u32>>,
    ss: usize,
    band_rows: usize,
    w_ss: usize,
}

impl PreparedModel {
    /// `place` maps a model point (screen-width units) to (output x, output y, 1/depth) and
    /// `turn` rotates a normal into world space. `ss` is the supersampling factor.
    pub fn project<'a>(
        &'a self,
        place: impl Fn([f32; 3]) -> (f32, f32, f32) + Sync,
        turn: impl Fn([f32; 3]) -> [f32; 3] + Sync,
        ow: usize,
        oh: usize,
        ss: usize,
        band_rows: usize,
    ) -> Projected<'a> {
        let ssf = ss as f32;
        let projected: Vec<(f32, f32, f32)> = self
            .pos
            .par_iter()
            .map(|p| {
                let (x, y, iw) = place(*p);
                (x * ssf, y * ssf, iw)
            })
            .collect();
        let nw: Vec<[f32; 3]> = self.nrm.par_iter().map(|n| turn(*n)).collect();
        let (sx, rest): (Vec<f32>, Vec<(f32, f32)>) = projected.into_iter().map(|(x, y, w)| (x, (y, w))).unzip();
        let (sy, iw): (Vec<f32>, Vec<f32>) = rest.into_iter().unzip();

        let band_h = (band_rows * ss) as f32;
        let n_bands = oh.div_ceil(band_rows);
        let (w_ss, h_ss) = ((ow * ss) as f32, (oh * ss) as f32);

        // Opaque triangles first, then see-through ones back to front.
        let mut order: Vec<u32> = (0..self.tris.len() as u32).collect();
        let depth = |t: &[u32; 3]| t.iter().map(|&i| iw[i as usize]).sum::<f32>();
        order.sort_by(|&a, &b| {
            let (ma, mb) = (&self.materials[self.tri_mat[a as usize] as usize], &self.materials[self.tri_mat[b as usize] as usize]);
            ma.blend.cmp(&mb.blend).then(depth(&self.tris[a as usize]).total_cmp(&depth(&self.tris[b as usize])))
        });

        let mut bins = vec![Vec::new(); n_bands];
        for ti in order {
            let t = self.tris[ti as usize];
            let ys = t.map(|i| sy[i as usize]);
            let xs = t.map(|i| sx[i as usize]);
            let (y0, y1) = (ys[0].min(ys[1]).min(ys[2]), ys[0].max(ys[1]).max(ys[2]));
            let (x0, x1) = (xs[0].min(xs[1]).min(xs[2]), xs[0].max(xs[1]).max(xs[2]));
            if y1 < 0.0 || y0 >= h_ss || x1 < 0.0 || x0 >= w_ss {
                continue;
            }
            let b0 = (y0.max(0.0) / band_h) as usize;
            let b1 = ((y1.min(h_ss - 1.0)) / band_h) as usize;
            for b in bins.iter_mut().take(b1.min(n_bands - 1) + 1).skip(b0) {
                b.push(ti);
            }
        }
        Projected { model: self, sx, sy, iw, nw, bins, ss, band_rows, w_ss: ow * ss }
    }
}

/// Rendered samples of one band, premultiplied: `col` already includes coverage.
pub struct Band {
    ss: usize,
    w_ss: usize,
    col: Vec<[f32; 3]>,
    cov: Vec<f32>,
}

impl Band {
    /// Averages the samples of one output pixel: (premultiplied colour, coverage).
    #[inline(always)]
    pub fn pixel(&self, x: usize, row: usize) -> ([f32; 3], f32) {
        let mut c = [0.0f32; 3];
        let mut a = 0.0;
        for sy in 0..self.ss {
            let base = (row * self.ss + sy) * self.w_ss + x * self.ss;
            for sx in 0..self.ss {
                let i = base + sx;
                c[0] += self.col[i][0];
                c[1] += self.col[i][1];
                c[2] += self.col[i][2];
                a += self.cov[i];
            }
        }
        let k = 1.0 / (self.ss * self.ss) as f32;
        ([c[0] * k, c[1] * k, c[2] * k], a * k)
    }
}

fn sample_tex(t: &Texture, u: f32, v: f32) -> [f32; 4] {
    let x = (u - u.floor()) * t.w as f32 - 0.5;
    let y = (v - v.floor()) * t.h as f32 - 0.5;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let wrap = |i: f32, n: usize| (i as i64).rem_euclid(n as i64) as usize;
    let px = |xx: f32, yy: f32| {
        let i = (wrap(yy, t.h) * t.w + wrap(xx, t.w)) * 4;
        [t.data[i] as f32, t.data[i + 1] as f32, t.data[i + 2] as f32, t.data[i + 3] as f32]
    };
    let (a, b, c, d) = (px(x0, y0), px(x0 + 1.0, y0), px(x0, y0 + 1.0), px(x0 + 1.0, y0 + 1.0));
    let mut out = [0.0; 4];
    for k in 0..4 {
        out[k] = (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy;
    }
    out
}

impl Projected<'_> {
    /// Rasterizes the triangles of `band` (output rows `band * band_rows ..`). `screen` returns
    /// the recording's colour at a point of the screen (u, v in 0..1).
    pub fn raster(&self, band: usize, rows: usize, screen: &dyn Fn(f32, f32) -> [f32; 3], light: &Lighting) -> Band {
        let m = self.model;
        let ss = self.ss;
        let h = rows * ss;
        let w = self.w_ss;
        let y_off = (band * self.band_rows * ss) as f32;
        let mut col = vec![[0.0f32; 3]; w * h];
        let mut cov = vec![0.0f32; w * h];
        let mut zbuf = vec![0.0f32; w * h];

        for &ti in &self.bins[band] {
            let t = m.tris[ti as usize];
            let [i0, i1, i2] = t.map(|i| i as usize);
            let (x0, y0) = (self.sx[i0], self.sy[i0] - y_off);
            let (x1, y1) = (self.sx[i1], self.sy[i1] - y_off);
            let (x2, y2) = (self.sx[i2], self.sy[i2] - y_off);
            let area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
            if area.abs() < 1e-9 {
                continue;
            }
            let back = area < 0.0;
            let inv_area = 1.0 / area;
            let mat = &m.materials[m.tri_mat[ti as usize] as usize];
            let is_screen = m.tri_mat[ti as usize] == m.screen;

            let minx = x0.min(x1).min(x2).floor().max(0.0) as usize;
            let maxx = (x0.max(x1).max(x2).ceil() as isize).min(w as isize - 1);
            let miny = y0.min(y1).min(y2).floor().max(0.0) as usize;
            let maxy = (y0.max(y1).max(y2).ceil() as isize).min(h as isize - 1);
            if maxx < minx as isize || maxy < miny as isize {
                continue;
            }
            let (w0i, w1i, w2i) = (self.iw[i0], self.iw[i1], self.iw[i2]);

            for py in miny..=maxy as usize {
                let fy = py as f32 + 0.5;
                for px in minx..=maxx as usize {
                    let fx = px as f32 + 0.5;
                    let e0 = ((x2 - x1) * (fy - y1) - (y2 - y1) * (fx - x1)) * inv_area;
                    let e1 = ((x0 - x2) * (fy - y2) - (y0 - y2) * (fx - x2)) * inv_area;
                    let e2 = 1.0 - e0 - e1;
                    if e0 < 0.0 || e1 < 0.0 || e2 < 0.0 {
                        continue;
                    }
                    let iw = e0 * w0i + e1 * w1i + e2 * w2i;
                    let idx = py * w + px;
                    if iw <= zbuf[idx] {
                        continue;
                    }
                    // Perspective-correct weights.
                    let (b0, b1, b2) = (e0 * w0i / iw, e1 * w1i / iw, e2 * w2i / iw);
                    let lerp3 = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| {
                        [a[0] * b0 + b[0] * b1 + c[0] * b2, a[1] * b0 + b[1] * b1 + c[1] * b2, a[2] * b0 + b[2] * b1 + c[2] * b2]
                    };

                    let (rgb, alpha) = if is_screen {
                        let p = lerp3(m.pos[i0], m.pos[i1], m.pos[i2]);
                        (screen(p[0] + 0.5, p[1] / m.screen_h + 0.5), 1.0)
                    } else {
                        let mut base = mat.base;
                        let mut a = mat.alpha;
                        if let Some(tex) = mat.texture.and_then(|i| m.textures.get(i)) {
                            let (ua, ub, uc) = (m.uv[i0], m.uv[i1], m.uv[i2]);
                            let u = ua[0] * b0 + ub[0] * b1 + uc[0] * b2;
                            let v = ua[1] * b0 + ub[1] * b1 + uc[1] * b2;
                            let c = sample_tex(tex, u, v);
                            base = [base[0] * c[0] / 255.0, base[1] * c[1] / 255.0, base[2] * c[2] / 255.0];
                            a *= c[3] / 255.0;
                        }
                        if let Some(cut) = mat.cutoff {
                            if a < cut {
                                continue;
                            }
                            a = 1.0;
                        } else if !mat.blend {
                            a = 1.0;
                        }
                        let mut n = norm(lerp3(self.nw[i0], self.nw[i1], self.nw[i2]));
                        if back {
                            n = [-n[0], -n[1], -n[2]];
                        }
                        (shade(base, mat, n, light), a)
                    };

                    if alpha >= 0.999 {
                        col[idx] = rgb;
                        cov[idx] = 1.0;
                        zbuf[idx] = iw;
                    } else if alpha > 0.0 {
                        let k = 1.0 - alpha;
                        col[idx] = [col[idx][0] * k + rgb[0] * alpha, col[idx][1] * k + rgb[1] * alpha, col[idx][2] * k + rgb[2] * alpha];
                        cov[idx] = cov[idx] * k + alpha;
                    }
                }
            }
        }
        Band { ss, w_ss: w, col, cov }
    }
}

/// Simple studio lighting: soft sky from above, a key light from the top-left and a highlight
/// whose sharpness follows the material's roughness.
fn shade(base: [f32; 3], mat: &Material, n: [f32; 3], l: &Lighting) -> [f32; 3] {
    let diffuse = dot(n, l.light).max(0.0);
    let sky = 0.5 - 0.5 * n[1];
    let metal = mat.metallic.clamp(0.0, 1.0);
    let rough = mat.roughness.clamp(0.04, 1.0);
    let spec_pow = 6.0 + (1.0 - rough).powi(2) * 160.0;
    let spec = dot(n, l.half).max(0.0).powf(spec_pow) * (0.06 + 0.9 * metal) * (1.15 - rough);
    let mut out = [0.0; 3];
    for k in 0..3 {
        let dif = base[k] * (0.3 * sky + 0.8 * diffuse) * (1.0 - 0.8 * metal);
        let refl = base[k] * metal * (0.25 + 0.55 * sky);
        let tint = if metal > 0.5 { base[k] } else { 255.0 };
        out[k] = (dif + refl + tint * spec + mat.emissive[k]).min(255.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run with: MODEL_DIR=<dir with phone_named.glb / phone_unnamed.glb> cargo test model3d -- --ignored
    #[test]
    #[ignore]
    fn finds_and_frames_the_screen() {
        let Ok(dir) = std::env::var("MODEL_DIR") else { return };
        for (file, screen_name) in [("phone_named.glb", "Screen"), ("phone_unnamed.glb", "Material.003")] {
            let src = load(&format!("{dir}/{file}")).expect("load");
            let guess = src.guess.expect("guess");
            assert_eq!(src.materials[guess].name, screen_name, "{file}");
            let m = prepare(&src, guess, 0).expect("prepare");
            assert!((m.meta.screen_aspect - 0.066 / 0.148).abs() < 0.01, "{:?}", m.meta);
            // The camera bump (last material) sits behind the screen, near its top (-y).
            let cam = src.materials.len() - 2;
            let pts: Vec<[f32; 3]> = m.tris.iter().zip(&m.tri_mat).filter(|(_, mt)| **mt as usize == cam).flat_map(|(t, _)| t.map(|i| m.pos[i as usize])).collect();
            assert!(pts.iter().all(|p| p[2] < 0.0 && p[1] < 0.0), "{file}: camera at {:?}", pts[0]);
            assert!(m.meta.z1 < 0.02 && m.meta.z0 < -0.1, "{:?}", m.meta);
        }
    }
}
