//! Video encoding on the graphics card: NVIDIA (NVENC), AMD (AMF on Windows, VAAPI on Linux) and
//! Intel (Quick Sync, or VAAPI on Linux). Each candidate is tried once with a tiny clip, so only
//! encoders that really work on this machine are offered. Frames are still drawn on the CPU;
//! the card takes over the encoding, which otherwise keeps every core busy.

use crate::platform::command;
use std::process::Stdio;
use std::sync::OnceLock;
use crate::i18n::lz;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    Nvenc,
    Amf,
    Qsv,
    Vaapi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    Hevc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoder {
    pub api: Api,
    /// VAAPI render node (`/dev/dri/renderD128`).
    pub device: Option<String>,
    pub h264: bool,
    pub hevc: bool,
    /// For people: "AMD (VAAPI)", "NVIDIA (NVENC)"…
    pub name: String,
}

impl Encoder {
    pub fn supports(&self, codec: Codec) -> bool {
        match codec {
            Codec::H264 => self.h264,
            Codec::Hevc => self.hevc,
        }
    }

    /// Options that go before the inputs.
    pub fn input_args(&self) -> Vec<String> {
        match (&self.api, &self.device) {
            (Api::Vaapi, Some(dev)) => vec!["-vaapi_device".into(), dev.clone()],
            _ => Vec::new(),
        }
    }

    /// Video encoding options for the output, at a quality comparable to x264's `crf`.
    /// `yuv`: the input is already YUV 4:2:0 BT.709 (from the GPU compositor), not RGBA.
    pub fn output_args(&self, codec: Codec, crf: u32, yuv: bool) -> Vec<String> {
        let q = crf.to_string();
        let name = |base: &str| {
            let c = if codec == Codec::H264 { "h264" } else { "hevc" };
            format!("{c}_{base}")
        };
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        let mut a = match self.api {
            Api::Nvenc => s(&["-c:v", &name("nvenc"), "-preset", "p5", "-tune", "hq", "-rc", "vbr", "-cq", &q, "-b:v", "0"]),
            Api::Amf => s(&["-c:v", &name("amf"), "-quality", "quality", "-rc", "cqp", "-qp_i", &q, "-qp_p", &q]),
            Api::Qsv => s(&["-c:v", &name("qsv"), "-preset", "medium", "-global_quality", &q]),
            Api::Vaapi => s(&["-c:v", &name("vaapi"), "-rc_mode", "CQP", "-qp", &q]),
        };
        let pix = if self.api == Api::Qsv { "nv12" } else { "yuv420p" };
        a.extend(match (self.api, yuv) {
            (Api::Vaapi, true) => s(&["-filter:v", "format=nv12,hwupload"]),
            // Upload the RGBA frames and convert them on the card too.
            (Api::Vaapi, false) => s(&["-filter:v", "format=rgba,hwupload,scale_vaapi=format=nv12:out_range=tv:out_color_matrix=bt709"]),
            (_, true) => s(&["-pix_fmt", pix]),
            (_, false) => to_yuv(pix),
        });
        if codec == Codec::Hevc {
            a.extend(["-tag:v".to_string(), "hvc1".to_string()]);
        }
        a
    }
}

/// Converts RGBA input to `pix` with BT.709 colours, TV range.
pub fn to_yuv(pix: &str) -> Vec<String> {
    let tags = "setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv";
    vec!["-filter:v".into(), format!("scale=out_color_matrix=bt709:out_range=tv,format={pix},{tags}"), "-pix_fmt".into(), pix.into()]
}

/// Tags the output as BT.709, TV range (what every path produces), so players show the right colours.
pub fn color_tags() -> Vec<String> {
    ["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"].iter().map(|x| x.to_string()).collect()
}

/// Encodes a short test clip with `enc`.
fn works(enc: &Encoder, codec: Codec) -> bool {
    command("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-y"])
        .args(enc.input_args())
        .args(["-f", "lavfi", "-i", "testsrc2=s=640x480:r=30:d=0.2,format=rgba"])
        .args(enc.output_args(codec, 23, false))
        .args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Graphics card maker of a Linux render node, from its PCI vendor id.
fn vendor_of(node: &str) -> &'static str {
    let id = node.rsplit('/').next().unwrap_or_default();
    let vendor = std::fs::read_to_string(format!("/sys/class/drm/{id}/device/vendor")).unwrap_or_default();
    match vendor.trim() {
        "0x1002" => "AMD",
        "0x8086" => "Intel",
        "0x10de" => "NVIDIA",
        _ => lz("Gráfica"),
    }
}

fn candidates() -> Vec<Encoder> {
    let make = |api, device: Option<String>, name: String| Encoder { api, device, h264: false, hevc: false, name };
    let mut list = vec![make(Api::Nvenc, None, "NVIDIA (NVENC)".into())];
    if cfg!(windows) {
        list.push(make(Api::Amf, None, "AMD (AMF)".into()));
        list.push(make(Api::Qsv, None, "Intel (Quick Sync)".into()));
    } else {
        let mut nodes: Vec<String> = std::fs::read_dir("/dev/dri")
            .map(|rd| rd.flatten().map(|e| e.path().to_string_lossy().into_owned()).filter(|p| p.contains("renderD")).collect())
            .unwrap_or_default();
        nodes.sort();
        for node in nodes {
            let name = format!("{} (VAAPI)", vendor_of(&node));
            list.push(make(Api::Vaapi, Some(node), name));
        }
        list.push(make(Api::Qsv, None, "Intel (Quick Sync)".into()));
    }
    list
}

fn probe() -> Option<Encoder> {
    candidates().into_iter().find_map(|mut e| {
        e.h264 = works(&e, Codec::H264);
        e.h264.then(|| {
            e.hevc = works(&e, Codec::Hevc);
            e
        })
    })
}

static FOUND: OnceLock<Option<Encoder>> = OnceLock::new();

/// The graphics card encoder to use, if any (tries them the first time; takes a moment).
pub fn detect() -> Option<Encoder> {
    FOUND.get_or_init(probe).clone()
}

/// What `detect` found, or `None` while it has not run yet.
pub fn detected() -> Option<Option<Encoder>> {
    FOUND.get().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_arguments_for_every_card() {
        let vaapi = Encoder { api: Api::Vaapi, device: Some("/dev/dri/renderD128".into()), h264: true, hevc: true, name: "AMD (VAAPI)".into() };
        assert_eq!(vaapi.input_args(), ["-vaapi_device", "/dev/dri/renderD128"]);
        let a = vaapi.output_args(Codec::Hevc, 21, false).join(" ");
        assert!(a.contains("hevc_vaapi") && a.contains("-qp 21") && a.contains("hvc1") && a.contains("scale_vaapi"), "{a}");
        assert!(vaapi.output_args(Codec::H264, 21, true).join(" ").contains("format=nv12,hwupload"));
        let nv = Encoder { api: Api::Nvenc, device: None, ..vaapi.clone() };
        assert!(nv.input_args().is_empty());
        assert!(nv.output_args(Codec::H264, 18, false).join(" ").contains("h264_nvenc -preset p5"));
        assert!(nv.output_args(Codec::H264, 18, false).join(" ").contains("out_color_matrix=bt709"));
        assert!(nv.output_args(Codec::H264, 18, true).join(" ").ends_with("-pix_fmt yuv420p"));
    }

    /// Needs ffmpeg and a graphics card: cargo test finds_a_card -- --ignored --nocapture
    #[test]
    #[ignore]
    fn finds_a_card() {
        let e = detect().expect("a working hardware encoder");
        println!("{e:?}");
        assert!(e.h264);
    }
}
