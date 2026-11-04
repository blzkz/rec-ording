//! Custom device models from glTF / GLB files: loading (with node animations), finding and
//! framing the screen, and a small software rasterizer so the model renders through the same
//! compositor as everything else.

use rayon::prelude::*;
use std::sync::Arc;

pub struct Texture {
    pub(crate) w: usize,
    pub(crate) h: usize,
    pub(crate) data: Vec<u8>,
}

pub struct Material {
    pub name: String,
    /// sRGB, 0..255.
    pub(crate) base: [f32; 3],
    pub(crate) alpha: f32,
    pub(crate) texture: Option<usize>,
    tex_coord: u32,
    /// KHR_texture_transform, as a 2×3 matrix applied to the texture coordinates.
    uv_xf: Option<[f32; 6]>,
    pub(crate) metallic: f32,
    pub(crate) roughness: f32,
    pub(crate) emissive: [f32; 3],
    pub(crate) blend: bool,
    pub(crate) cutoff: Option<f32>,
}

/// A selectable piece of the model: a material, or one colour of a palette-textured material
/// (low-poly models often paint the whole phone, screen included, from one small palette).
pub struct Part {
    pub name: String,
    color: [f32; 3],
}

#[derive(Clone, Copy)]
struct NodeRest {
    parent: Option<usize>,
    t: [f32; 3],
    r: [f32; 4],
    s: [f32; 3],
}

#[derive(Clone, Copy, PartialEq)]
enum Target {
    Translation,
    Rotation,
    Scale,
}

struct Channel {
    node: usize,
    target: Target,
    times: Vec<f32>,
    values: Vec<[f32; 4]>,
    step: bool,
    cubic: bool,
}

pub struct Animation {
    pub name: String,
    pub duration: f32,
    channels: Vec<Channel>,
}

/// Node hierarchy and vertices in their nodes' local space, for posing animated models.
struct Rig {
    nodes: Vec<NodeRest>,
    /// Parents before children.
    order: Vec<usize>,
    local: Vec<[f32; 3]>,
    local_n: Vec<[f32; 3]>,
    vnode: Vec<u32>,
}

/// A model as loaded: triangles in their rest pose, with materials and parts.
pub struct ModelSource {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Arc<Vec<[f32; 2]>>,
    tris: Arc<Vec<[u32; 3]>>,
    tri_mat: Arc<Vec<u32>>,
    tri_part: Vec<u32>,
    pub parts: Vec<Part>,
    materials: Arc<Vec<Material>>,
    textures: Arc<Vec<Texture>>,
    pub animations: Arc<Vec<Animation>>,
    rig: Arc<Rig>,
    /// Best guess for the screen part.
    pub guess: Option<usize>,
}

// ---------- Small math ----------

type M4 = [[f32; 4]; 4];

const IDENTITY: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

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

fn trs(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> M4 {
    let [x, y, z, w] = q;
    [
        [(1.0 - 2.0 * (y * y + z * z)) * s[0], (2.0 * (x * y + w * z)) * s[0], (2.0 * (x * z - w * y)) * s[0], 0.0],
        [(2.0 * (x * y - w * z)) * s[1], (1.0 - 2.0 * (x * x + z * z)) * s[1], (2.0 * (y * z + w * x)) * s[1], 0.0],
        [(2.0 * (x * z + w * y)) * s[2], (2.0 * (y * z - w * x)) * s[2], (1.0 - 2.0 * (x * x + y * y)) * s[2], 0.0],
        [t[0], t[1], t[2], 1.0],
    ]
}

fn xf_point(m: &M4, p: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0],
        m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1],
        m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2],
    ]
}

/// Cofactors of the 3×3 part: transforms normals like the inverse transpose (up to scale).
fn cofactors(m: &M4) -> ([[f32; 3]; 3], f32) {
    let c = [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
    let cof = [cross(c[1], c[2]), cross(c[2], c[0]), cross(c[0], c[1])];
    (cof, dot(c[0], cof[0]))
}

fn xf_normal(cof: &[[f32; 3]; 3], det: f32, n: [f32; 3]) -> [f32; 3] {
    let v = [
        n[0] * cof[0][0] + n[1] * cof[1][0] + n[2] * cof[2][0],
        n[0] * cof[0][1] + n[1] * cof[1][1] + n[2] * cof[2][1],
        n[0] * cof[0][2] + n[1] * cof[1][2] + n[2] * cof[2][2],
    ];
    let v = if det < 0.0 { [-v[0], -v[1], -v[2]] } else { v };
    norm(v)
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

// ---------- Animation ----------

fn sample_channel(ch: &Channel, t: f32) -> [f32; 4] {
    let n = ch.times.len();
    let value = |k: usize| if ch.cubic { ch.values[3 * k + 1] } else { ch.values[k] };
    if n == 0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    if t <= ch.times[0] {
        return value(0);
    }
    if t >= ch.times[n - 1] {
        return value(n - 1);
    }
    let k = ch.times.partition_point(|&x| x <= t) - 1;
    if ch.step {
        return value(k);
    }
    let span = (ch.times[k + 1] - ch.times[k]).max(1e-6);
    let f = (t - ch.times[k]) / span;
    let (a, mut b) = (value(k), value(k + 1));
    if ch.target == Target::Rotation {
        // Shortest way round, then normalized lerp.
        if a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] < 0.0 {
            b = [-b[0], -b[1], -b[2], -b[3]];
        }
        let q = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f, a[3] + (b[3] - a[3]) * f];
        let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-9);
        return [q[0] / l, q[1] / l, q[2] / l, q[3] / l];
    }
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f, 0.0]
}

impl Rig {
    fn world_matrices(&self, anim: Option<(&Animation, f32)>) -> Vec<M4> {
        let mut local: Vec<NodeRest> = self.nodes.clone();
        if let Some((a, t)) = anim {
            for ch in &a.channels {
                let v = sample_channel(ch, t);
                let n = &mut local[ch.node];
                match ch.target {
                    Target::Translation => n.t = [v[0], v[1], v[2]],
                    Target::Rotation => n.r = v,
                    Target::Scale => n.s = [v[0], v[1], v[2]],
                }
            }
        }
        let mut world = vec![IDENTITY; self.nodes.len()];
        for &i in &self.order {
            let l = trs(local[i].t, local[i].r, local[i].s);
            world[i] = match local[i].parent {
                Some(p) => mul4(&world[p], &l),
                None => l,
            };
        }
        world
    }
}

// ---------- Loading ----------

/// Extensions that change how geometry is stored: without them the model cannot be read at all.
const GEOMETRY_EXTENSIONS: &[&str] = &["KHR_draco_mesh_compression", "EXT_meshopt_compression", "KHR_meshopt_compression", "KHR_mesh_quantization"];

/// Reads and validates a glTF/GLB document. Unlike `gltf::import`, extensions the file marks as
/// required but we do not know are tolerated (the model is drawn with what we understand),
/// except the ones that compress the geometry.
pub fn read_document(path: &std::path::Path) -> Result<(gltf::Document, Option<Vec<u8>>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("No se pudo leer el modelo: {e}"))?;
    let bad = |e: &dyn std::fmt::Display| format!("El archivo no es un modelo glTF válido: {e}");
    let (mut root, blob) = if bytes.starts_with(b"glTF") {
        let mut glb = gltf::Glb::from_slice(&bytes).map_err(|e| bad(&e))?;
        let root = gltf::json::Root::from_slice(&glb.json).map_err(|e| bad(&e))?;
        (root, glb.bin.take().map(|b| b.into_owned()))
    } else {
        (gltf::json::Root::from_slice(&bytes).map_err(|e| bad(&e))?, None)
    };
    if let Some(ext) = root.extensions_required.iter().find(|e| GEOMETRY_EXTENSIONS.contains(&e.as_str())) {
        return Err(if ext.contains("draco") {
            "El modelo usa compresión Draco, que no se puede leer. Expórtalo de nuevo sin compresión (en Blender, desmarca «Compresión» al exportar a glTF).".to_string()
        } else {
            format!("El modelo usa geometría comprimida ({ext}), que no se puede leer. Expórtalo de nuevo sin compresión.")
        });
    }
    root.extensions_required.retain(|e| gltf::json::extensions::ENABLED_EXTENSIONS.contains(&e.as_str()));
    let doc = gltf::Document::from_json(root).map_err(|e| bad(&e))?;
    Ok((doc, blob))
}

pub fn load(path: &str) -> Result<ModelSource, String> {
    let file = std::path::Path::new(path);
    let (doc, blob) = read_document(file)?;
    let base = file.parent();
    let buffers = gltf::import_buffers(&doc, base, blob).map_err(|e| format!("No se pudieron leer los datos del modelo: {e}"))?;
    let images = gltf::import_images(&doc, base, &buffers).map_err(|e| {
        format!("No se pudieron leer las texturas del modelo ({e}). Comprueba que están junto al archivo.")
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
                tex_coord: tex
                    .as_ref()
                    .map(|t| t.texture_transform().and_then(|x| x.tex_coord()).unwrap_or(t.tex_coord()))
                    .unwrap_or(0),
                uv_xf: tex.as_ref().and_then(|t| t.texture_transform()).map(|x| {
                    let ([ox, oy], r, [sx, sy]) = (x.offset(), x.rotation(), x.scale());
                    let (sn, cs) = r.sin_cos();
                    [cs * sx, sn * sy, ox, -sn * sx, cs * sy, oy]
                }),
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
        uv_xf: None,
        metallic: 0.0,
        roughness: 0.6,
        emissive: [0.0; 3],
        blend: false,
        cutoff: None,
    });

    // Node hierarchy (rest pose).
    let mut nodes: Vec<NodeRest> = doc
        .nodes()
        .map(|n| {
            let (t, r, s) = n.transform().decomposed();
            NodeRest { parent: None, t, r, s }
        })
        .collect();
    for n in doc.nodes() {
        for c in n.children() {
            nodes[c.index()].parent = Some(n.index());
        }
    }
    let scene = doc.default_scene().or_else(|| doc.scenes().next()).ok_or("El archivo no tiene ninguna escena")?;
    let mut order = Vec::new();
    let mut stack: Vec<gltf::Node> = scene.nodes().collect();
    while let Some(n) = stack.pop() {
        order.push(n.index());
        stack.extend(n.children());
    }

    let mut rig = Rig { nodes, order, local: Vec::new(), local_n: Vec::new(), vnode: Vec::new() };
    let rest = rig.world_matrices(None);
    let doc_nodes: Vec<gltf::Node> = doc.nodes().collect();

    let mut pos = Vec::new();
    let mut nrm = Vec::new();
    let mut uv = Vec::new();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut tri_mat: Vec<u32> = Vec::new();
    let mut tri_named: Vec<bool> = Vec::new();

    for &ni in &rig.order.clone() {
        let node = &doc_nodes[ni];
        let Some(mesh) = node.mesh() else { continue };
        let m = &rest[ni];
        let (cof, det) = cofactors(m);
        let flip = det < 0.0;
        for prim in mesh.primitives() {
            let reader = prim.reader(|b| Some(&buffers[b.index()]));
            let Some(positions) = reader.read_positions() else { continue };
            let positions: Vec<[f32; 3]> = positions.collect();
            let mat_idx = prim.material().index().unwrap_or(default_mat);
            let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
            let xf = materials[mat_idx].uv_xf;
            let uvs: Option<Vec<[f32; 2]>> = reader.read_tex_coords(materials[mat_idx].tex_coord).map(|t| {
                t.into_f32()
                    .map(|[u, v]| match xf {
                        Some(m) => [m[0] * u + m[1] * v + m[2], m[3] * u + m[4] * v + m[5]],
                        None => [u, v],
                    })
                    .collect()
            });
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

            let base = pos.len() as u32;
            for (i, p) in positions.iter().enumerate() {
                rig.local.push(*p);
                rig.vnode.push(ni as u32);
                pos.push(xf_point(m, *p));
                let n = normals.as_ref().map(|n| n[i]).unwrap_or([0.0, 0.0, 1.0]);
                rig.local_n.push(n);
                nrm.push(xf_normal(&cof, det, n));
                uv.push(uvs.as_ref().map(|u| u[i]).unwrap_or([0.0; 2]));
            }
            let named = looks_like_screen(prim.material().name()) || looks_like_screen(mesh.name()) || looks_like_screen(node.name());
            for t in tri_idx {
                if t.iter().any(|&i| i as usize >= positions.len()) {
                    continue;
                }
                let t = if flip { [t[0], t[2], t[1]] } else { t };
                let tri = [base + t[0], base + t[1], base + t[2]];
                if normals.is_none() {
                    // Flat shading from the face normal (in both spaces).
                    let (a, b, c) = (pos[tri[0] as usize], pos[tri[1] as usize], pos[tri[2] as usize]);
                    let fw = norm(cross(sub(b, a), sub(c, a)));
                    let (la, lb, lc) = (rig.local[tri[0] as usize], rig.local[tri[1] as usize], rig.local[tri[2] as usize]);
                    let fl = norm(cross(sub(lb, la), sub(lc, la)));
                    let fl = if flip { [-fl[0], -fl[1], -fl[2]] } else { fl };
                    for &v in &tri {
                        nrm[v as usize] = fw;
                        rig.local_n[v as usize] = fl;
                    }
                }
                tris.push(tri);
                tri_mat.push(mat_idx as u32);
                tri_named.push(named);
            }
        }
    }
    if tris.is_empty() {
        return Err("El modelo no tiene triángulos".into());
    }

    let animations: Vec<Animation> = doc
        .animations()
        .enumerate()
        .filter_map(|(i, a)| {
            let mut channels = Vec::new();
            let mut duration = 0.0f32;
            for ch in a.channels() {
                let reader = ch.reader(|b| Some(&buffers[b.index()]));
                let (Some(inputs), Some(outputs)) = (reader.read_inputs(), reader.read_outputs()) else { continue };
                let times: Vec<f32> = inputs.collect();
                use gltf::animation::util::ReadOutputs;
                let (target, values): (Target, Vec<[f32; 4]>) = match outputs {
                    ReadOutputs::Translations(v) => (Target::Translation, v.map(|x| [x[0], x[1], x[2], 0.0]).collect()),
                    ReadOutputs::Rotations(v) => (Target::Rotation, v.into_f32().collect()),
                    ReadOutputs::Scales(v) => (Target::Scale, v.map(|x| [x[0], x[1], x[2], 0.0]).collect()),
                    ReadOutputs::MorphTargetWeights(_) => continue,
                };
                let interp = ch.sampler().interpolation();
                duration = duration.max(times.last().copied().unwrap_or(0.0));
                channels.push(Channel {
                    node: ch.target().node().index(),
                    target,
                    times,
                    values,
                    step: interp == gltf::animation::Interpolation::Step,
                    cubic: interp == gltf::animation::Interpolation::CubicSpline,
                });
            }
            (!channels.is_empty() && duration > 0.0).then(|| Animation {
                name: a.name().map(str::to_string).unwrap_or_else(|| format!("Animación {}", i + 1)),
                duration,
                channels,
            })
        })
        .collect();

    let (parts, tri_part) = split_parts(&materials, &textures, &tris, &tri_mat, &uv);
    let mut src = ModelSource {
        pos,
        nrm,
        uv: Arc::new(uv),
        tris: Arc::new(tris),
        tri_mat: Arc::new(tri_mat),
        tri_part,
        parts,
        materials: Arc::new(materials),
        textures: Arc::new(textures),
        animations: Arc::new(animations),
        rig: Arc::new(rig),
        guess: None,
    };
    src.guess = guess_screen(&src, &tri_named);
    Ok(src)
}

fn tri_area_normal(pos: &[[f32; 3]], t: &[u32; 3]) -> ([f32; 3], f32) {
    let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
    let cr = cross(sub(b, a), sub(c, a));
    let l = dot(cr, cr).sqrt();
    (if l > 0.0 { [cr[0] / l, cr[1] / l, cr[2] / l] } else { [0.0; 3] }, l)
}

/// One part per material, except in palette-textured low-poly models, where a material is split
/// by the colour each triangle samples (when there are only a handful of colours).
fn split_parts(
    materials: &[Material],
    textures: &[Texture],
    tris: &[[u32; 3]],
    tri_mat: &[u32],
    uv: &[[f32; 2]],
) -> (Vec<Part>, Vec<u32>) {
    let mut tri_part = vec![0u32; tris.len()];
    let mut parts = Vec::new();
    // Low-poly models paint the whole device from one palette texture, so the screen is only a
    // colour within it. Detailed models already give each piece its own material.
    let mut used: Vec<u32> = tri_mat.to_vec();
    used.sort_unstable();
    used.dedup();
    let palette_model = used.len() <= 2;
    for (mi, mat) in materials.iter().enumerate() {
        let mine: Vec<usize> = (0..tris.len()).filter(|&i| tri_mat[i] as usize == mi).collect();
        if mine.is_empty() {
            continue;
        }
        let tex = mat.texture.and_then(|t| textures.get(t)).filter(|_| palette_model);
        let mut split: Option<Vec<([u8; 3], Vec<usize>)>> = None;
        if let Some(tex) = tex {
            let mut buckets: Vec<([u8; 3], Vec<usize>)> = Vec::new();
            for &i in &mine {
                let t = tris[i];
                let u = (uv[t[0] as usize][0] + uv[t[1] as usize][0] + uv[t[2] as usize][0]) / 3.0;
                let v = (uv[t[0] as usize][1] + uv[t[1] as usize][1] + uv[t[2] as usize][1]) / 3.0;
                let c = sample_tex(tex, u, v);
                let key = [(c[0] as u8) >> 4, (c[1] as u8) >> 4, (c[2] as u8) >> 4];
                match buckets.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, list)) => list.push(i),
                    None => buckets.push((key, vec![i])),
                }
                if buckets.len() > 16 {
                    break;
                }
            }
            if buckets.len() > 1 && buckets.len() <= 16 {
                split = Some(buckets);
            }
        }
        match split {
            Some(buckets) => {
                for (key, list) in buckets {
                    let color = [key[0] as f32 * 17.0, key[1] as f32 * 17.0, key[2] as f32 * 17.0];
                    let id = parts.len() as u32;
                    parts.push(Part {
                        name: format!("{} · #{:02x}{:02x}{:02x}", mat.name, key[0] * 17, key[1] * 17, key[2] * 17),
                        color,
                    });
                    for i in list {
                        tri_part[i] = id;
                    }
                }
            }
            None => {
                let id = parts.len() as u32;
                parts.push(Part { name: mat.name.clone(), color: mat.base });
                for i in mine {
                    tri_part[i] = id;
                }
            }
        }
    }
    (parts, tri_part)
}

/// Screen guess: a part whose name says so; otherwise, among the large flat parts, the one that
/// sits furthest out along its own normal (in front of the bezel), preferably dark.
fn guess_screen(src: &ModelSource, tri_named: &[bool]) -> Option<usize> {
    let n = src.parts.len();
    let mut named = vec![0.0f32; n];
    let mut area = vec![0.0f32; n];
    let mut nsum = vec![[0.0f32; 3]; n];
    let mut csum = vec![[0.0f32; 3]; n];
    for (i, t) in src.tris.iter().enumerate() {
        let p = src.tri_part[i] as usize;
        let (nn, ar) = tri_area_normal(&src.pos, t);
        if tri_named[i] {
            named[p] += ar;
        }
        area[p] += ar;
        for k in 0..3 {
            nsum[p][k] += nn[k] * ar;
            csum[p][k] += ar * (src.pos[t[0] as usize][k] + src.pos[t[1] as usize][k] + src.pos[t[2] as usize][k]) / 3.0;
        }
    }
    if let Some((i, _)) = named.iter().enumerate().filter(|(_, a)| **a > 0.0).max_by(|a, b| a.1.total_cmp(b.1)) {
        return Some(i);
    }

    // Area of each part that faces its main direction (an inset screen also has thin sides).
    let mut front = vec![0.0f32; n];
    for (i, t) in src.tris.iter().enumerate() {
        let p = src.tri_part[i] as usize;
        let (nn, ar) = tri_area_normal(&src.pos, t);
        if dot(nn, norm(nsum[p])) > 0.9 {
            front[p] += ar;
        }
    }
    let count = src.pos.len() as f32;
    let centroid = src.pos.iter().fold([0.0f32; 3], |acc, p| [acc[0] + p[0] / count, acc[1] + p[1] / count, acc[2] + p[2] / count]);
    let flat: Vec<usize> = (0..n).filter(|&i| area[i] > 0.0 && front[i] / area[i] > 0.75).collect();
    let biggest = flat.iter().map(|&i| front[i]).fold(0.0f32, f32::max);
    flat.into_iter().filter(|&i| front[i] >= 0.3 * biggest).max_by(|&a, &b| {
        let score = |i: usize| {
            let c = [csum[i][0] / area[i], csum[i][1] / area[i], csum[i][2] / area[i]];
            let out = dot(sub(c, centroid), norm(nsum[i]));
            let col = src.parts[i].color;
            out * if col[0] + col[1] + col[2] < 200.0 { 1.0 } else { 0.7 }
        };
        score(a).total_cmp(&score(b))
    })
}

// ---------- Framing the screen ----------

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

/// World → device space: `((p - origin) · [x y z] - shift) * scale`.
#[derive(Clone, Copy)]
struct Framing {
    origin: [f32; 3],
    x: [f32; 3],
    y: [f32; 3],
    z: [f32; 3],
    shift: [f32; 3],
    scale: f32,
}

impl Framing {
    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let d = sub(p, self.origin);
        [
            (dot(d, self.x) - self.shift[0]) * self.scale,
            (dot(d, self.y) - self.shift[1]) * self.scale,
            (dot(d, self.z) - self.shift[2]) * self.scale,
        ]
    }
    fn normal(&self, n: [f32; 3]) -> [f32; 3] {
        [dot(n, self.x), dot(n, self.y), dot(n, self.z)]
    }
}

/// A model normalized around its screen and ready to draw.
pub struct PreparedModel {
    pub meta: ModelMeta,
    pub(crate) pos: Vec<[f32; 3]>,
    pub(crate) nrm: Vec<[f32; 3]>,
    /// Where each screen vertex falls on the screen (0..1), from the rest pose.
    pub(crate) suv: Vec<[f32; 2]>,
    pub(crate) tri_screen: Vec<bool>,
    pub(crate) uv: Arc<Vec<[f32; 2]>>,
    pub(crate) tris: Arc<Vec<[u32; 3]>>,
    pub(crate) tri_mat: Arc<Vec<u32>>,
    pub(crate) materials: Arc<Vec<Material>>,
    pub(crate) textures: Arc<Vec<Texture>>,
    animations: Arc<Vec<Animation>>,
    rig: Arc<Rig>,
    framing: Framing,
}

/// Puts the model in device space: finds the screen's plane and the smallest rectangle around
/// it, turns the model so the screen faces the camera upright, and scales it so the screen is
/// 1 unit wide. `quarter_turns` rotates the screen by 90° for models whose "up" was guessed wrong.
pub fn prepare(src: &ModelSource, screen_part: usize, quarter_turns: u8) -> Result<PreparedModel, String> {
    // Main direction of the part, then only the triangles facing it (not the sides of an inset screen).
    let mut nsum = [0.0f32; 3];
    for (i, t) in src.tris.iter().enumerate() {
        if src.tri_part[i] as usize == screen_part {
            let (n, a) = tri_area_normal(&src.pos, t);
            for k in 0..3 {
                nsum[k] += n[k] * a;
            }
        }
    }
    if dot(nsum, nsum) <= 0.0 {
        return Err("Esa pieza no tiene superficie visible".into());
    }
    let main = norm(nsum);
    // Some models put the glass of the back cameras in the same part, facing the same way: keep
    // only the triangles near the plane where most of the screen lies.
    let centroid = |t: &[u32; 3]| {
        let (p0, p1, p2) = (src.pos[t[0] as usize], src.pos[t[1] as usize], src.pos[t[2] as usize]);
        [(p0[0] + p1[0] + p2[0]) / 3.0, (p0[1] + p1[1] + p2[1]) / 3.0, (p0[2] + p1[2] + p2[2]) / 3.0]
    };
    let mut facing: Vec<(f32, f32, usize)> = Vec::new();
    for (i, t) in src.tris.iter().enumerate() {
        if src.tri_part[i] as usize != screen_part {
            continue;
        }
        let (n, a) = tri_area_normal(&src.pos, t);
        if dot(n, main) >= 0.85 {
            facing.push((dot(centroid(t), main), a, i));
        }
    }
    facing.sort_by(|x, y| x.0.total_cmp(&y.0));
    let half = facing.iter().map(|f| f.1).sum::<f32>() * 0.5;
    let mut acc = 0.0;
    let plane = facing.iter().find(|f| {
        acc += f.1;
        acc >= half
    });
    let plane = plane.map(|f| f.0).unwrap_or(0.0);
    let (lo, hi) = src.pos.iter().map(|p| dot(*p, main)).fold((f32::MAX, f32::MIN), |(lo, hi), d| (lo.min(d), hi.max(d)));
    let tolerance = 0.25 * (hi - lo).max(1e-6);
    let mut tri_screen = vec![false; src.tris.len()];
    let mut csum = [0.0f32; 3];
    let mut area = 0.0f32;
    for &(depth, _, i) in &facing {
        if (depth - plane).abs() > tolerance {
            continue;
        }
        let t = &src.tris[i];
        let (_, a) = tri_area_normal(&src.pos, t);
        tri_screen[i] = true;
        area += a;
        for k in 0..3 {
            csum[k] += a * (src.pos[t[0] as usize][k] + src.pos[t[1] as usize][k] + src.pos[t[2] as usize][k]) / 3.0;
        }
    }
    let center = [csum[0] / area, csum[1] / area, csum[2] / area];
    let mut z = main;
    // The rest of the device should sit behind the screen.
    let count = src.pos.len() as f32;
    let all_c = src.pos.iter().fold([0.0f32; 3], |acc, p| [acc[0] + p[0] / count, acc[1] + p[1] / count, acc[2] + p[2] / count]);
    if dot(sub(all_c, center), z) > 0.0 {
        z = [-z[0], -z[1], -z[2]];
    }

    // Smallest rectangle around the screen (robust to uneven vertex density, e.g. a camera hole).
    let screen_verts: Vec<[f32; 3]> = src
        .tris
        .iter()
        .zip(&tri_screen)
        .filter(|(_, s)| **s)
        .flat_map(|(t, _)| t.map(|i| src.pos[i as usize]))
        .collect();
    let helper = if z[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let a = norm(cross(helper, z));
    let b = cross(z, a);
    let flat: Vec<(f32, f32)> = screen_verts.iter().map(|p| (dot(sub(*p, center), a), dot(sub(*p, center), b))).collect();
    let mut best = (f32::MAX, 0.0f32, 0.0f32, 0.0f32);
    for step in 0..360 {
        let th = (step as f32 * 0.5).to_radians();
        let (s, c) = th.sin_cos();
        let (mut u0, mut u1, mut v0, mut v1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for &(u, v) in &flat {
            let (ru, rv) = (u * c + v * s, -u * s + v * c);
            u0 = u0.min(ru);
            u1 = u1.max(ru);
            v0 = v0.min(rv);
            v1 = v1.max(rv);
        }
        let ar = (u1 - u0) * (v1 - v0);
        if ar < best.0 - 1e-9 {
            best = (ar, th, u1 - u0, v1 - v0);
        }
    }
    let (_, th, du, dv) = best;
    let th = if dv > du { th + std::f32::consts::FRAC_PI_2 } else { th };
    let mut long = norm([
        a[0] * th.cos() + b[0] * th.sin(),
        a[1] * th.cos() + b[1] * th.sin(),
        a[2] * th.cos() + b[2] * th.sin(),
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

    let mut framing = Framing { origin: center, x, y, z, shift: [0.0; 3], scale: 1.0 };
    let (mut sx0, mut sx1, mut sy0, mut sy1, mut sz1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN, f32::MIN);
    for p in &screen_verts {
        let d = framing.point(*p);
        sx0 = sx0.min(d[0]);
        sx1 = sx1.max(d[0]);
        sy0 = sy0.min(d[1]);
        sy1 = sy1.max(d[1]);
        sz1 = sz1.max(d[2]);
    }
    let sw = (sx1 - sx0).max(1e-9);
    let sh = (sy1 - sy0).max(1e-9);
    framing.shift = [(sx0 + sx1) * 0.5, (sy0 + sy1) * 0.5, sz1];
    framing.scale = 1.0 / sw;
    let screen_h = sh / sw;

    let pos: Vec<[f32; 3]> = src.pos.par_iter().map(|p| framing.point(*p)).collect();
    let nrm: Vec<[f32; 3]> = src.nrm.par_iter().map(|n| framing.normal(*n)).collect();
    let mut suv = vec![[0.0f32; 2]; pos.len()];
    for (t, s) in src.tris.iter().zip(&tri_screen) {
        if *s {
            for &v in t {
                let p = pos[v as usize];
                suv[v as usize] = [p[0] + 0.5, p[1] / screen_h + 0.5];
            }
        }
    }
    let mut bounds = [f32::MAX, f32::MIN, f32::MAX, f32::MIN, f32::MAX, f32::MIN];
    for p in &pos {
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
        suv,
        tri_screen,
        uv: src.uv.clone(),
        tris: src.tris.clone(),
        tri_mat: src.tri_mat.clone(),
        materials: src.materials.clone(),
        textures: src.textures.clone(),
        animations: src.animations.clone(),
        rig: src.rig.clone(),
        framing,
    })
}

/// Vertex positions and normals (device space) at one instant of an animation.
pub struct Pose {
    pub(crate) pos: Vec<[f32; 3]>,
    pub(crate) nrm: Vec<[f32; 3]>,
}

impl PreparedModel {
    /// Poses the model at `t` seconds of animation `anim`.
    pub fn pose(&self, anim: usize, t: f32) -> Option<Pose> {
        let a = self.animations.get(anim)?;
        let world = self.rig.world_matrices(Some((a, t)));
        let cofs: Vec<([[f32; 3]; 3], f32)> = world.iter().map(cofactors).collect();
        let rig = &self.rig;
        let (pos, nrm): (Vec<[f32; 3]>, Vec<[f32; 3]>) = (0..rig.local.len())
            .into_par_iter()
            .map(|i| {
                let ni = rig.vnode[i] as usize;
                let (cof, det) = &cofs[ni];
                (
                    self.framing.point(xf_point(&world[ni], rig.local[i])),
                    self.framing.normal(xf_normal(cof, *det, rig.local_n[i])),
                )
            })
            .unzip();
        Some(Pose { pos, nrm })
    }

    pub fn animation_duration(&self, anim: usize) -> Option<f32> {
        self.animations.get(anim).map(|a| a.duration)
    }
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
    /// `turn` rotates a normal into world space. `pose` overrides the rest pose (animation).
    /// `ss` is the supersampling factor.
    #[allow(clippy::too_many_arguments)]
    pub fn project<'a>(
        &'a self,
        pose: Option<&Pose>,
        place: impl Fn([f32; 3]) -> (f32, f32, f32) + Sync,
        turn: impl Fn([f32; 3]) -> [f32; 3] + Sync,
        ow: usize,
        oh: usize,
        ss: usize,
        band_rows: usize,
    ) -> Projected<'a> {
        let ssf = ss as f32;
        let (pos, nrm) = match pose {
            Some(p) => (&p.pos, &p.nrm),
            None => (&self.pos, &self.nrm),
        };
        let projected: Vec<(f32, f32, f32)> = pos
            .par_iter()
            .map(|p| {
                let (x, y, iw) = place(*p);
                (x * ssf, y * ssf, iw)
            })
            .collect();
        let nw: Vec<[f32; 3]> = nrm.par_iter().map(|n| turn(*n)).collect();
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
    /// the screen's colour at a point of the screen (u, v in 0..1).
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
            let is_screen = m.tri_screen[ti as usize];

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
                    let lerp2 = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| [a[0] * b0 + b[0] * b1 + c[0] * b2, a[1] * b0 + b[1] * b1 + c[1] * b2];

                    let (rgb, alpha) = if is_screen {
                        let s = lerp2(m.suv[i0], m.suv[i1], m.suv[i2]);
                        (screen(s[0], s[1]), 1.0)
                    } else {
                        let mut base = mat.base;
                        let mut a = mat.alpha;
                        if let Some(tex) = mat.texture.and_then(|i| m.textures.get(i)) {
                            let u = lerp2(m.uv[i0], m.uv[i1], m.uv[i2]);
                            let c = sample_tex(tex, u[0], u[1]);
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
                        let (n0, n1, n2) = (self.nw[i0], self.nw[i1], self.nw[i2]);
                        let mut n = norm([
                            n0[0] * b0 + n1[0] * b1 + n2[0] * b2,
                            n0[1] * b0 + n1[1] * b1 + n2[1] * b2,
                            n0[2] * b0 + n1[2] * b1 + n2[2] * b2,
                        ]);
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
            assert_eq!(src.parts[guess].name, screen_name, "{file}");
            let m = prepare(&src, guess, 0).expect("prepare");
            assert!((m.meta.screen_aspect - 0.066 / 0.148).abs() < 0.01, "{:?}", m.meta);
            // The camera bump (last material) sits behind the screen, near its top (-y).
            let cam = src.materials.len() - 2;
            let pts: Vec<[f32; 3]> = m
                .tris
                .iter()
                .zip(m.tri_mat.iter())
                .filter(|(_, mt)| **mt as usize == cam)
                .flat_map(|(t, _)| t.map(|i| m.pos[i as usize]))
                .collect();
            assert!(pts.iter().all(|p| p[2] < 0.0 && p[1] < 0.0), "{file}: camera at {:?}", pts[0]);
            assert!(m.meta.z1 < 0.02 && m.meta.z0 < -0.1, "{:?}", m.meta);
        }
    }

    #[test]
    #[ignore]
    fn plays_node_animations() {
        let Ok(dir) = std::env::var("MODEL_DIR") else { return };
        let src = load(&format!("{dir}/phone_animated.glb")).expect("load");
        let names: Vec<&str> = src.animations.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Giro", "Salto"]);
        assert!((src.animations[0].duration - 4.0).abs() < 1e-4);
        let m = prepare(&src, src.guess.unwrap(), 0).expect("prepare");
        // A quarter of the turn: the screen now faces sideways instead of the camera.
        let start = m.pose(0, 0.0).unwrap();
        let quarter = m.pose(0, 1.0).unwrap();
        let screen_normal = |p: &Pose| {
            let i = m.tris.iter().zip(&m.tri_screen).find(|(_, s)| **s).unwrap().0[0] as usize;
            p.nrm[i]
        };
        assert!(screen_normal(&start)[2] > 0.99, "{:?}", screen_normal(&start));
        assert!(screen_normal(&quarter)[2].abs() < 0.05, "{:?}", screen_normal(&quarter));
        // "Salto" lifts the phone (towards the camera, as it lies on its back) and puts it back:
        // 0.4 world units on a 0.66-wide screen.
        let dist = |a: [f32; 3], b: [f32; 3]| dot(sub(a, b), sub(a, b)).sqrt();
        let up = m.pose(1, 0.5).unwrap();
        let down = m.pose(1, 1.0).unwrap();
        assert!((dist(up.pos[0], m.pos[0]) - 0.4 / 0.66).abs() < 0.01, "{}", dist(up.pos[0], m.pos[0]));
        assert!(dist(down.pos[0], m.pos[0]) < 1e-3);
    }

    #[test]
    fn samples_rotation_keys() {
        let ch = Channel {
            node: 0,
            target: Target::Rotation,
            times: vec![0.0, 1.0],
            values: vec![[0.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 0.0]],
            step: false,
            cubic: false,
        };
        let q = sample_channel(&ch, 0.5);
        // Halfway through a 180° turn about Y is 90°.
        assert!((q[1] - (0.5f32).sqrt()).abs() < 1e-4 && (q[3] - (0.5f32).sqrt()).abs() < 1e-4, "{q:?}");
        assert_eq!(sample_channel(&ch, 2.0), [0.0, 1.0, 0.0, 0.0]);
    }
}

