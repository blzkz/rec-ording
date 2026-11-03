use crate::model::*;
use rayon::prelude::*;

#[derive(Debug, Clone, Copy)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    #[inline(always)]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    #[inline(always)]
    pub fn dot(self, o: Self) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    #[inline(always)]
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    #[inline(always)]
    pub fn normalize(self) -> Self {
        let l = self.length().max(1e-6);
        Self::new(self.x / l, self.y / l, self.z / l)
    }

    #[inline(always)]
    pub fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }

    #[inline(always)]
    pub fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }

    #[inline(always)]
    pub fn scale(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }

    #[inline(always)]
    pub fn rotate_euler_deg(self, rx_deg: f32, ry_deg: f32, rz_deg: f32) -> Self {
        let rx = rx_deg.to_radians();
        let ry = ry_deg.to_radians();
        let rz = rz_deg.to_radians();

        let (sx, cx) = rx.sin_cos();
        let (sy, cy) = ry.sin_cos();
        let (sz, cz) = rz.sin_cos();

        // Rotate Z
        let x1 = self.x * cz - self.y * sz;
        let y1 = self.x * sz + self.y * cz;
        let z1 = self.z;

        // Rotate X
        let x2 = x1;
        let y2 = y1 * cx - z1 * sx;
        let z2 = y1 * sx + z1 * cx;

        // Rotate Y
        let x3 = x2 * cy + z2 * sy;
        let y3 = y2;
        let z3 = -x2 * sy + z2 * cy;

        Self::new(x3, y3, z3)
    }

    #[inline(always)]
    pub fn inv_rotate_euler_deg(self, rx_deg: f32, ry_deg: f32, rz_deg: f32) -> Self {
        let rx = (-rx_deg).to_radians();
        let ry = (-ry_deg).to_radians();
        let rz = (-rz_deg).to_radians();

        let (sx, cx) = rx.sin_cos();
        let (sy, cy) = ry.sin_cos();
        let (sz, cz) = rz.sin_cos();

        // Inverse Y
        let x1 = self.x * cy + self.z * sy;
        let y1 = self.y;
        let z1 = -self.x * sy + self.z * cy;

        // Inverse X
        let x2 = x1;
        let y2 = y1 * cx - z1 * sx;
        let z2 = y1 * sx + z1 * cx;

        // Inverse Z
        let x3 = x2 * cz - y2 * sz;
        let y3 = x2 * sz + y2 * cz;
        let z3 = z2;

        Self::new(x3, y3, z3)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CameraPose {
    pub focus_x: f32,
    pub focus_y: f32,
    pub zoom: f32,
    pub rot_x_deg: f32,
    pub rot_y_deg: f32,
    pub rot_z_deg: f32,
}

#[derive(Debug, Clone)]
pub struct GlbModelMetadata {
    pub mesh_count: usize,
    pub vertex_count: usize,
    pub primary_mesh_name: String,
    pub aspect_ratio: f32,
}

pub fn inspect_glb_file(path: &str) -> Option<GlbModelMetadata> {
    let (doc, _buffers, _images) = gltf::import(path).ok()?;
    let mesh_count = doc.meshes().count();
    let mut vertex_count = 0usize;
    let mut primary_mesh_name = String::from("GLB_Screen_Mesh");
    for mesh in doc.meshes() {
        if let Some(name) = mesh.name() {
            primary_mesh_name = name.to_string();
        }
        for prim in mesh.primitives() {
            if let Some(acc) = prim.get(&gltf::Semantic::Positions) {
                vertex_count += acc.count();
            }
        }
    }
    Some(GlbModelMetadata {
        mesh_count,
        vertex_count,
        primary_mesh_name,
        aspect_ratio: 0.48,
    })
}

pub fn parse_hex_rgb(hex: &str, default: [u8; 3]) -> [u8; 3] {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() >= 6 {
        let r = u8::from_str_radix(&clean[0..2], 16).unwrap_or(default[0]);
        let g = u8::from_str_radix(&clean[2..4], 16).unwrap_or(default[1]);
        let b = u8::from_str_radix(&clean[4..6], 16).unwrap_or(default[2]);
        [r, g, b]
    } else {
        default
    }
}

#[inline(always)]
fn ease_value(t: f32, mode: &str) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match mode {
        "spring" => {
            let c4 = (2.0 * std::f32::consts::PI) / 4.5;
            if t <= 0.0 || t >= 1.0 {
                t
            } else {
                (2.0_f32.powf(-9.0 * t) * ((t * 5.0 - 0.75) * c4).sin() + 1.0).clamp(0.0, 1.08)
            }
        }
        "easeInOutCubic" => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
            }
        }
        _ => t,
    }
}

pub fn evaluate_camera_pose(
    project: &ProjectFile,
    time_sec: f32,
    orbit_override: (f32, f32, f32),
) -> CameraPose {
    let mut pose = CameraPose {
        focus_x: 0.5,
        focus_y: 0.5,
        zoom: 1.0,
        rot_x_deg: 6.0,
        rot_y_deg: -14.0,
        rot_z_deg: 1.0,
    };

    if !project.director_keyframes.is_empty() {
        let mut sorted = project.director_keyframes.clone();
        sorted.sort_by(|a, b| a.time_sec.partial_cmp(&b.time_sec).unwrap_or(std::cmp::Ordering::Equal));

        let mut prev = &sorted[0];
        let mut next = &sorted[0];

        for kf in &sorted {
            if kf.time_sec <= time_sec {
                prev = kf;
            }
            if kf.time_sec > time_sec {
                next = kf;
                break;
            }
            next = kf;
        }

        if std::ptr::eq(prev, next) || (next.time_sec - prev.time_sec).abs() < 1e-4 {
            pose.focus_x = prev.focus_x;
            pose.focus_y = prev.focus_y;
            pose.zoom = prev.zoom;
            pose.rot_x_deg = prev.rot_x_deg;
            pose.rot_y_deg = prev.rot_y_deg;
            pose.rot_z_deg = prev.rot_z_deg;
        } else {
            let span = (next.time_sec - prev.time_sec).max(0.1);
            let raw_t = ((time_sec - prev.time_sec) / span).clamp(0.0, 1.0);
            let e = ease_value(raw_t, &next.easing);
            pose.focus_x = prev.focus_x + (next.focus_x - prev.focus_x) * e;
            pose.focus_y = prev.focus_y + (next.focus_y - prev.focus_y) * e;
            pose.zoom = prev.zoom + (next.zoom - prev.zoom) * e;
            pose.rot_x_deg = prev.rot_x_deg + (next.rot_x_deg - prev.rot_x_deg) * e;
            pose.rot_y_deg = prev.rot_y_deg + (next.rot_y_deg - prev.rot_y_deg) * e;
            pose.rot_z_deg = prev.rot_z_deg + (next.rot_z_deg - prev.rot_z_deg) * e;
        }
    }

    if project.device_3d.floating_animation {
        let amp = project.device_3d.floating_amplitude;
        pose.rot_x_deg += (time_sec * 1.7).sin() * 2.5 * amp;
        pose.rot_y_deg += (time_sec * 1.2).cos() * 3.5 * amp;
    }

    pose.rot_x_deg += orbit_override.0;
    pose.rot_y_deg += orbit_override.1;
    pose.zoom = (pose.zoom * orbit_override.2).clamp(0.4, 3.5);

    pose
}

#[inline(always)]
fn sample_android_screen_rgba(u: f32, v: f32, time_sec: f32, model: &DeviceModelType) -> [u8; 4] {
    // High-contrast Material 3 Android 16 App Simulation Texture
    let scroll = (time_sec * 0.14).sin() * 0.08 + (time_sec * 0.05);

    // Top punch-hole camera cutout
    let punch_y = 0.032;
    let dx_p = (u - 0.5) * 1.0;
    let dy_p = (v - punch_y) * 2.1;
    if dx_p * dx_p + dy_p * dy_p < 0.00075 && *model != DeviceModelType::DesktopWindow {
        return [8, 10, 15, 255];
    }

    // Status bar
    if v < 0.055 {
        return [13, 19, 33, 255];
    }

    // App Header Banner
    if v < 0.14 {
        let grad = ((u * 45.0) as u8).saturating_add(25);
        return [18, 26 + grad / 3, 52 + grad / 2, 255];
    }

    // Sensitive API Token / Email Card at v in 0.17..0.28 (demonstrating live Privacy Mask)
    if v >= 0.16 && v <= 0.27 && u >= 0.08 && u <= 0.92 {
        let border = u < 0.095 || u > 0.905 || v < 0.168 || v > 0.262;
        if border {
            return [99, 102, 241, 255];
        }
        // Simulated text stripes inside token box
        let stripe_y = ((v - 0.18) * 120.0) as i32;
        let stripe_x = ((u - 0.12) * 60.0) as i32;
        if (stripe_y == 2 || stripe_y == 6) && (stripe_x % 5 != 0) && u < 0.84 {
            return [248, 113, 113, 255];
        }
        return [30, 27, 75, 255];
    }

    // Interactive NPU / Performance Spline Chart Card
    if v >= 0.30 && v <= 0.56 && u >= 0.08 && u <= 0.92 {
        let local_u = (u - 0.08) / 0.84;
        let local_v = (v - 0.30) / 0.26;
        let wave = 0.52 + 0.28 * ((local_u * 6.5 + time_sec * 2.8).sin() * (local_u * 3.0 - time_sec).cos());
        let dist = (local_v - wave).abs();
        if dist < 0.032 {
            return [56, 189, 248, 255];
        } else if local_v > wave {
            let glow = ((1.0 - (local_v - wave)).clamp(0.0, 1.0) * 65.0) as u8;
            return [15, 35 + glow / 2, 65 + glow, 255];
        }
        return [17, 24, 39, 255];
    }

    // Scrolling Feature Cards
    let scrolled_v = v + scroll * 0.25;
    let card_row = ((scrolled_v - 0.60) * 7.5).floor() as i32;
    let row_frac = ((scrolled_v - 0.60) * 7.5).fract();
    if v >= 0.59 && v <= 0.89 && u >= 0.08 && u <= 0.92 && row_frac > 0.12 && row_frac < 0.88 {
        if u < 0.24 {
            let accent = if card_row % 2 == 0 {
                [16, 185, 129, 255]
            } else {
                [139, 92, 246, 255]
            };
            return accent;
        }
        if row_frac > 0.32 && row_frac < 0.48 && u > 0.28 && u < 0.78 {
            return [226, 232, 240, 255];
        }
        if row_frac > 0.58 && row_frac < 0.68 && u > 0.28 && u < 0.62 {
            return [100, 116, 139, 255];
        }
        return [30, 41, 59, 255];
    }

    // Bottom Navigation Pill Bar
    if v > 0.91 {
        if v > 0.965 && u > 0.35 && u < 0.65 {
            return [203, 213, 225, 255];
        }
        return [15, 23, 42, 255];
    }

    [9, 13, 22, 255]
}

#[inline(always)]
fn sample_pc_window_rgba(u: f32, v: f32, time_sec: f32) -> [u8; 4] {
    // Window Titlebar with traffic light dots
    if v < 0.08 {
        if (u - 0.04).hypot((v - 0.04) * 1.8) < 0.014 {
            return [239, 68, 68, 255];
        }
        if (u - 0.075).hypot((v - 0.04) * 1.8) < 0.014 {
            return [245, 158, 11, 255];
        }
        if (u - 0.11).hypot((v - 0.04) * 1.8) < 0.014 {
            return [16, 185, 129, 255];
        }
        return [30, 41, 59, 255];
    }

    // Left sidebar
    if u < 0.18 {
        return [15, 23, 42, 255];
    }

    // Code lines / ADB logcat simulation
    let line = ((v + time_sec * 0.04) * 28.0) as i32;
    let line_frac = ((v + time_sec * 0.04) * 28.0).fract();
    if line_frac > 0.25 && line_frac < 0.72 {
        let len = 0.35 + ((line * 37) % 45) as f32 * 0.01;
        if u > 0.22 && u < (0.22 + len).min(0.94) {
            return match line.rem_euclid(4) {
                0 => [56, 189, 248, 255],
                1 => [167, 139, 250, 255],
                2 => [52, 211, 153, 255],
                _ => [148, 163, 184, 255],
            };
        }
    }

    [11, 15, 25, 255]
}

#[derive(Clone, Copy)]
struct DeviceBoxSpec {
    center: Vec3,
    half_extents: Vec3,
    rot_x: f32,
    rot_y: f32,
    rot_z: f32,
    corner_radius: f32,
    bezel: f32,
    model: DeviceModelRole,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeviceModelRole {
    AndroidPrimary,
    PcSecondary,
}

#[inline(always)]
fn intersect_oriented_box(
    ray_origin: Vec3,
    ray_dir: Vec3,
    spec: &DeviceBoxSpec,
) -> Option<(f32, Vec3, Vec3)> {
    let ro = ray_origin
        .sub(spec.center)
        .inv_rotate_euler_deg(spec.rot_x, spec.rot_y, spec.rot_z);
    let rd = ray_dir.inv_rotate_euler_deg(spec.rot_x, spec.rot_y, spec.rot_z);

    let inv_x = if rd.x.abs() > 1e-6 { 1.0 / rd.x } else { 1e6 };
    let inv_y = if rd.y.abs() > 1e-6 { 1.0 / rd.y } else { 1e6 };
    let inv_z = if rd.z.abs() > 1e-6 { 1.0 / rd.z } else { 1e6 };

    let t1 = (-spec.half_extents.x - ro.x) * inv_x;
    let t2 = (spec.half_extents.x - ro.x) * inv_x;
    let t3 = (-spec.half_extents.y - ro.y) * inv_y;
    let t4 = (spec.half_extents.y - ro.y) * inv_y;
    let t5 = (-spec.half_extents.z - ro.z) * inv_z;
    let t6 = (spec.half_extents.z - ro.z) * inv_z;

    let tmin = t1.min(t2).max(t3.min(t4)).max(t5.min(t6));
    let tmax = t1.max(t2).min(t3.max(t4)).min(t5.max(t6));

    if tmax < 0.0 || tmin > tmax {
        return None;
    }

    let t = if tmin > 0.0 { tmin } else { tmax };
    let local_p = ro.add(rd.scale(t));

    // Rounded corner check in XY plane
    let r = spec.corner_radius.min(spec.half_extents.x * 0.45);
    let qx = local_p.x.abs() - (spec.half_extents.x - r);
    let qy = local_p.y.abs() - (spec.half_extents.y - r);
    if qx > 0.0 && qy > 0.0 && (qx * qx + qy * qy) > r * r {
        return None;
    }

    // Surface normal in local space
    let dx = (local_p.x.abs() - spec.half_extents.x).abs();
    let dy = (local_p.y.abs() - spec.half_extents.y).abs();
    let dz = (local_p.z.abs() - spec.half_extents.z).abs();

    let local_n = if dz <= dx && dz <= dy {
        Vec3::new(0.0, 0.0, local_p.z.signum())
    } else if dx <= dy {
        Vec3::new(local_p.x.signum(), 0.0, 0.0)
    } else {
        Vec3::new(0.0, local_p.y.signum(), 0.0)
    };

    let world_n = local_n
        .rotate_euler_deg(spec.rot_x, spec.rot_y, spec.rot_z)
        .normalize();

    Some((t, local_p, world_n))
}

/// Renders a full composite frame into an RGBA buffer (`width * height * 4` bytes)
/// using `rayon` parallel scanline execution across all CPU cores.
pub fn render_frame_multicore(
    project: &ProjectFile,
    time_sec: f32,
    width: usize,
    height: usize,
    orbit_override: (f32, f32, f32),
    out_rgba: &mut [u8],
) {
    let expected_len = width * height * 4;
    if out_rgba.len() != expected_len {
        return;
    }

    let pose = evaluate_camera_pose(project, time_sec, orbit_override);
    let pose_prev = evaluate_camera_pose(project, (time_sec - 0.025).max(0.0), orbit_override);
    let cam_vel = ((pose.zoom - pose_prev.zoom).abs() * 2.5
        + (pose.rot_y_deg - pose_prev.rot_y_deg).abs() * 0.05)
        .clamp(0.0, 1.0);

    let bg_a = parse_hex_rgb(&project.background.color_a, [15, 23, 42]);
    let bg_b = parse_hex_rgb(&project.background.color_b, [30, 27, 75]);
    let bg_c = parse_hex_rgb(&project.background.color_c, [4, 47, 46]);
    let body_rgb = parse_hex_rgb(&project.device_3d.body_color, [30, 41, 59]);
    let transparent_bg =
        project.export_settings.transparent_background || project.background.bg_type == "transparent";

    // Build primary & secondary 3D device specs
    let (base_w, base_h, corner_r) = match project.device_3d.primary_model {
        DeviceModelType::Pixel11Pro => (0.82, 1.72, 0.12),
        DeviceModelType::GalaxyS26Ultra => (0.84, 1.75, 0.025),
        DeviceModelType::GalaxyTabS10Ultra => (1.85, 1.18, 0.06),
        DeviceModelType::DesktopWindow => (1.95, 1.22, 0.05),
        DeviceModelType::CustomGlb => (0.86, 1.74, 0.09),
        DeviceModelType::None => (0.90, 1.75, 0.04),
    };

    let (prim_w, prim_h) = match project.device_3d.orientation {
        Orientation::Portrait => (base_w, base_h),
        Orientation::Landscape => {
            if base_w < base_h {
                (base_h, base_w)
            } else {
                (base_w, base_h)
            }
        }
    };

    let pan_x = (0.5 - pose.focus_x) * 1.1;
    let pan_y = (pose.focus_y - 0.5) * 1.1;

    let mut specs = Vec::with_capacity(2);
    match project.device_3d.dual_layout {
        DualLayout::Single => {
            specs.push(DeviceBoxSpec {
                center: Vec3::new(pan_x, pan_y, 0.0),
                half_extents: Vec3::new(prim_w, prim_h, 0.075),
                rot_x: pose.rot_x_deg,
                rot_y: pose.rot_y_deg,
                rot_z: pose.rot_z_deg,
                corner_radius: corner_r,
                bezel: project.device_3d.bezel_width.clamp(0.01, 0.12),
                model: DeviceModelRole::AndroidPrimary,
            });
        }
        DualLayout::SideBySide => {
            specs.push(DeviceBoxSpec {
                center: Vec3::new(pan_x - 1.05, pan_y, -0.15),
                half_extents: Vec3::new(1.35, 0.88, 0.05),
                rot_x: pose.rot_x_deg * 0.7,
                rot_y: pose.rot_y_deg + 12.0,
                rot_z: 0.0,
                corner_radius: 0.05,
                bezel: 0.03,
                model: DeviceModelRole::PcSecondary,
            });
            specs.push(DeviceBoxSpec {
                center: Vec3::new(pan_x + 1.05, pan_y - 0.05, 0.22),
                half_extents: Vec3::new(prim_w * 0.88, prim_h * 0.88, 0.075),
                rot_x: pose.rot_x_deg,
                rot_y: pose.rot_y_deg - 10.0,
                rot_z: pose.rot_z_deg,
                corner_radius: corner_r,
                bezel: project.device_3d.bezel_width.clamp(0.01, 0.12),
                model: DeviceModelRole::AndroidPrimary,
            });
        }
        DualLayout::PipFloating | DualLayout::AngledShowcase => {
            specs.push(DeviceBoxSpec {
                center: Vec3::new(pan_x - 0.35, pan_y + 0.08, -0.35),
                half_extents: Vec3::new(1.65, 1.05, 0.05),
                rot_x: pose.rot_x_deg * 0.5,
                rot_y: pose.rot_y_deg * 0.5 + 8.0,
                rot_z: 0.0,
                corner_radius: 0.05,
                bezel: 0.03,
                model: DeviceModelRole::PcSecondary,
            });
            specs.push(DeviceBoxSpec {
                center: Vec3::new(pan_x + 0.85, pan_y - 0.18, 0.45),
                half_extents: Vec3::new(prim_w * 0.78, prim_h * 0.78, 0.075),
                rot_x: pose.rot_x_deg,
                rot_y: pose.rot_y_deg - 14.0,
                rot_z: pose.rot_z_deg,
                corner_radius: corner_r,
                bezel: project.device_3d.bezel_width.clamp(0.01, 0.12),
                model: DeviceModelRole::AndroidPrimary,
            });
        }
    }

    let aspect = width as f32 / (height.max(1) as f32);
    let cam_z = 3.85 / pose.zoom.max(0.25);
    let ray_origin = Vec3::new(0.0, 0.0, cam_z);
    let light_dir = Vec3::new(0.55, 0.65, 0.85).normalize();

    let active_masks: Vec<&PrivacyMask> = project
        .privacy_masks
        .iter()
        .filter(|m| time_sec >= m.start_sec && time_sec <= m.end_sec)
        .collect();

    let time_ms = (time_sec * 1000.0) as u64;
    let active_pointers: Vec<&TelemetryPointerEvent> = project
        .pointer_events
        .iter()
        .filter(|ev| {
            let dur = ev.duration_ms.unwrap_or(350);
            time_ms >= ev.timestamp_ms && time_ms <= ev.timestamp_ms + dur + 250
        })
        .collect();

    let primary_model = project.device_3d.primary_model.clone();
    let show_shadow = project.device_3d.show_shadow;
    let shadow_intensity = project.device_3d.shadow_intensity;
    let show_ripples = project.cursor_style.show_click_ripples;
    let show_trails = project.cursor_style.show_touch_trails;
    let motion_blur = project.background.motion_blur_enabled && cam_vel > 0.08;

    // Parallel scanline rendering across all CPU cores with Rayon!
    out_rgba
        .par_chunks_mut(width * 4)
        .enumerate()
        .for_each(|(py, row)| {
            let ny = 1.0 - (py as f32 / height as f32) * 2.0;
            let uv_y = py as f32 / height as f32;

            for px in 0..width {
                let nx = ((px as f32 / width as f32) * 2.0 - 1.0) * aspect;
                let uv_x = px as f32 / width as f32;

                // 1. Background Color
                let mut final_rgba = if transparent_bg {
                    let checker = ((px / 16) + (py / 16)) % 2 == 0;
                    if checker {
                        [24, 30, 44, 0]
                    } else {
                        [15, 20, 32, 0]
                    }
                } else {
                    let w1 = (1.0 - (uv_x * 0.75 + uv_y * 0.65)).clamp(0.0, 1.0);
                    let w2 = ((uv_x * 0.8 + (1.0 - uv_y) * 0.5) * 0.8).clamp(0.0, 1.0);
                    let w3 = (1.0 - w1 - w2 * 0.5).clamp(0.0, 1.0);
                    let sum = (w1 + w2 + w3).max(1e-4);
                    let r = ((bg_a[0] as f32 * w1 + bg_b[0] as f32 * w2 + bg_c[0] as f32 * w3) / sum) as u8;
                    let g = ((bg_a[1] as f32 * w1 + bg_b[1] as f32 * w2 + bg_c[1] as f32 * w3) / sum) as u8;
                    let b = ((bg_a[2] as f32 * w1 + bg_b[2] as f32 * w2 + bg_c[2] as f32 * w3) / sum) as u8;
                    [r, g, b, 255]
                };

                // Soft drop shadow behind 3D device
                if show_shadow && !transparent_bg {
                    let sdx = nx * 0.65;
                    let sdy = (ny + 0.15) * 0.55;
                    let sdist = (sdx * sdx + sdy * sdy).sqrt();
                    if sdist < 1.1 {
                        let atten = (1.0 - (1.0 - sdist / 1.1).powi(2) * shadow_intensity * 0.55).clamp(0.2, 1.0);
                        final_rgba[0] = (final_rgba[0] as f32 * atten) as u8;
                        final_rgba[1] = (final_rgba[1] as f32 * atten) as u8;
                        final_rgba[2] = (final_rgba[2] as f32 * atten) as u8;
                    }
                }

                // 2. Raytrace 3D Device Models
                let ray_dir = Vec3::new(nx * 0.55, ny * 0.55, -1.0).normalize();
                let mut closest_t = f32::MAX;

                for spec in &specs {
                    if let Some((t, local_p, world_n)) = intersect_oriented_box(ray_origin, ray_dir, spec) {
                        if t < closest_t {
                            closest_t = t;

                            let is_front_face = (local_p.z - spec.half_extents.z).abs() < 0.008;
                            let screen_w = spec.half_extents.x - spec.bezel;
                            let screen_h = spec.half_extents.y - spec.bezel;

                            if is_front_face && local_p.x.abs() <= screen_w && local_p.y.abs() <= screen_h {
                                let su = ((local_p.x / screen_w) * 0.5 + 0.5).clamp(0.0, 1.0);
                                let sv = (0.5 - (local_p.y / screen_h) * 0.5).clamp(0.0, 1.0);

                                let mut screen_col = match spec.model {
                                    DeviceModelRole::AndroidPrimary => {
                                        sample_android_screen_rgba(su, sv, time_sec, &primary_model)
                                    }
                                    DeviceModelRole::PcSecondary => sample_pc_window_rgba(su, sv, time_sec),
                                };

                                // Subtle glass reflection highlight across the screen
                                let spec_glare = ((su + sv * 0.6 - 0.75).abs() < 0.08) as u8 * 10;
                                screen_col[0] = screen_col[0].saturating_add(spec_glare);
                                screen_col[1] = screen_col[1].saturating_add(spec_glare);
                                screen_col[2] = screen_col[2].saturating_add(spec_glare);

                                final_rgba = screen_col;
                            } else {
                                // Metallic / Titanium 3D Chassis PBR Shading
                                let ndotl = world_n.dot(light_dir).max(0.15);
                                let half_v = light_dir.add( Vec3::new(0.0, 0.0, 1.0) ).normalize();
                                let spec_pow = world_n.dot(half_v).max(0.0).powi(24) * 95.0;

                                // Pixel 11 Pro rear camera visor / S26 Ultra camera rings detail on back face
                                let is_back_face = (local_p.z + spec.half_extents.z).abs() < 0.008;
                                let visor_mod = if is_back_face && local_p.y > spec.half_extents.y * 0.45 && local_p.y < spec.half_extents.y * 0.72 {
                                    0.45
                                } else {
                                    1.0
                                };

                                final_rgba = [
                                    ((body_rgb[0] as f32 * ndotl * visor_mod + spec_pow).clamp(0.0, 255.0)) as u8,
                                    ((body_rgb[1] as f32 * ndotl * visor_mod + spec_pow).clamp(0.0, 255.0)) as u8,
                                    ((body_rgb[2] as f32 * ndotl * visor_mod + spec_pow).clamp(0.0, 255.0)) as u8,
                                    255,
                                ];
                            }
                        }
                    }
                }

                // 3. Privacy Masks (Blur / Pixelate / Solid Redaction)
                for mask in &active_masks {
                    if uv_x >= mask.x
                        && uv_x <= mask.x + mask.width
                        && uv_y >= mask.y
                        && uv_y <= mask.y + mask.height
                    {
                        match mask.mode {
                            PrivacyMaskMode::Solid => {
                                let c = mask
                                    .color
                                    .as_deref()
                                    .map(|h| parse_hex_rgb(h, [15, 23, 42]))
                                    .unwrap_or([15, 23, 42]);
                                final_rgba = [c[0], c[1], c[2], 255];
                            }
                            PrivacyMaskMode::Pixelate | PrivacyMaskMode::Blur => {
                                let step = (mask.intensity.max(6) as usize).max(4);
                                let qx = (px / step) * step;
                                let qy = (py / step) * step;
                                let hash = ((qx * 31 + qy * 17) % 28) as u8;
                                final_rgba = [28 + hash, 36 + hash, 58 + hash, 255];
                            }
                        }
                    }
                }

                // 4. Touch Ripples & Swipe Vector Trails
                if show_ripples || show_trails {
                    for ev in &active_pointers {
                        let age_ms = time_ms.saturating_sub(ev.timestamp_ms) as f32;
                        let dur_ms = ev.duration_ms.unwrap_or(350).max(100) as f32;
                        let prog = (age_ms / (dur_ms + 250.0)).clamp(0.0, 1.0);

                        let (tx, ty) = if ev.event_type == "touch_swipe" {
                            let ex = ev.end_x.unwrap_or(ev.x);
                            let ey = ev.end_y.unwrap_or(ev.y);
                            (ev.x + (ex - ev.x) * prog, ev.y + (ey - ev.y) * prog)
                        } else {
                            (ev.x, ev.y)
                        };

                        let dx = (uv_x - tx) * aspect;
                        let dy = uv_y - ty;
                        let dist = (dx * dx + dy * dy).sqrt();
                        let ring_r = 0.015 + prog * 0.055;
                        if (dist - ring_r).abs() < 0.0045 || dist < 0.011 {
                            let alpha = (1.0 - prog).clamp(0.15, 1.0);
                            final_rgba[0] = ((final_rgba[0] as f32 * (1.0 - alpha)) + 56.0 * alpha) as u8;
                            final_rgba[1] = ((final_rgba[1] as f32 * (1.0 - alpha)) + 189.0 * alpha) as u8;
                            final_rgba[2] = ((final_rgba[2] as f32 * (1.0 - alpha)) + 248.0 * alpha) as u8;
                            final_rgba[3] = 255;
                        }
                    }
                }

                // 5. Velocity Motion Blur vignette accent during fast camera transitions
                if motion_blur {
                    let edge = ((uv_x - 0.5).abs() + (uv_y - 0.5).abs()).clamp(0.0, 1.0);
                    let mb = (edge * cam_vel * 0.35).clamp(0.0, 0.4);
                    final_rgba[0] = (final_rgba[0] as f32 * (1.0 - mb) + 30.0 * mb) as u8;
                    final_rgba[1] = (final_rgba[1] as f32 * (1.0 - mb) + 41.0 * mb) as u8;
                    final_rgba[2] = (final_rgba[2] as f32 * (1.0 - mb) + 75.0 * mb) as u8;
                }

                let idx = px * 4;
                row[idx] = final_rgba[0];
                row[idx + 1] = final_rgba[1];
                row[idx + 2] = final_rgba[2];
                row[idx + 3] = final_rgba[3];
            }
        });
}
