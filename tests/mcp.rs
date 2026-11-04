//! Drives `rec-ording --mcp` like an agent would, over JSON-RPC on stdio, against a throwaway
//! data folder. Needs ffmpeg (skipped otherwise).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    id: u64,
}

impl Mcp {
    fn start(data: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rec-ording"))
            .arg("--mcp")
            .env("XDG_DATA_HOME", data.join("data"))
            .env("XDG_CACHE_HOME", data.join("cache"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start rec-ording --mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Mcp { child, stdin, stdout, id: 0 }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params});
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad reply {line:?}: {e}"));
        assert_eq!(v["id"], self.id, "{v}");
        v
    }

    /// Calls a tool; panics with its message if it fails.
    fn call(&mut self, name: &str, args: Value) -> Value {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        let result = &r["result"];
        assert_eq!(result["isError"], false, "{name}: {}", result["content"][0]["text"]);
        result.clone()
    }

    fn text(&mut self, name: &str, args: Value) -> String {
        self.call(name, args)["content"][0]["text"].as_str().unwrap().to_string()
    }

    fn project(&mut self) -> Value {
        serde_json::from_str(&self.text("get_project", json!({}))).unwrap()
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn an_agent_builds_and_exports_a_video() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("ffmpeg no está instalado: se omite la prueba");
        return;
    }
    let dir = std::env::temp_dir().join(format!("rec-ording-mcp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("app.mp4");
    let ok = Command::new("ffmpeg")
        .args(["-nostdin", "-y", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc2=size=360x800:rate=30:duration=4"])
        .args(["-f", "lavfi", "-i", "sine=frequency=330:duration=4", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
        .arg(&video)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let video = video.to_string_lossy().into_owned();

    let mut mcp = Mcp::start(&dir);
    let init = mcp.request("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}));
    assert_eq!(init["result"]["serverInfo"]["name"], "rec-ording");
    let tools = mcp.request("tools/list", json!({}));
    assert!(tools["result"]["tools"].as_array().unwrap().len() > 30);
    // Unknown methods are errors, not silence.
    assert!(mcp.request("nope", json!({}))["error"]["code"] == -32601);

    mcp.call("create_project", json!({"name": "Prueba agente"}));
    mcp.call("import_video", json!({"path": video}));
    mcp.call("import_video", json!({"path": video}));
    let p = mcp.project();
    assert_eq!(p["name"], "Prueba agente");
    assert_eq!(p["clips"].as_array().unwrap().len(), 2);
    assert!((p["duration"].as_f64().unwrap() - 8.0).abs() < 0.1);

    mcp.call("add_zoom", json!({"start": 0.5, "end": 2.0, "zoom": 2.2, "x": 0.5, "y": 0.3}));
    // A turn in the middle of the video; it fits after the zoom without touching it.
    mcp.call("add_camera_keyframe", json!({"time": 3.4, "turn": 35, "tilt": 10, "ease": "smooth"}));
    mcp.call("director_add_target", json!({"time": 7.0, "x0": 0.1, "y0": 0.1, "x1": 0.5, "y1": 0.3}));
    mcp.call("director_add_target", json!({"time": 7.8, "x0": 0.4, "y0": 0.6, "x1": 0.9, "y1": 0.9}));
    mcp.call("director_settings", json!({"motion": "3d", "intensity": 0.8}));
    let msg = mcp.text("director_generate", json!({}));
    assert!(msg.contains("2 planos") && !msg.contains("sustituyen"), "{msg}");
    mcp.call("add_device_motion", json!({"preset": "hop", "time": 1.0}));
    mcp.call("set_style", json!({"aspect": "9:16", "background_preset": "Atardecer", "padding": 0.1}));
    mcp.call("set_angle", json!({"turn": -15, "straighten_on_zoom": true}));
    mcp.call("set_device", json!({"model": "Moderno"}));
    mcp.call("set_model_options", json!({"video_fit": "fill", "fit_pan": 0.3}));

    let p = mcp.project();
    let kinds: Vec<&str> = p["camera"].as_array().unwrap().iter().map(|c| c["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"Zoom") && kinds.contains(&"Movimiento") && kinds.iter().filter(|k| **k == "Director").count() == 2, "{kinds:?}");
    assert_eq!(p["style"]["aspect"], "9:16");
    assert_eq!(p["style"]["frame"], "model");
    assert_eq!(p["device_motion"].as_array().unwrap().len(), 1);

    // Titles beside the device, and a double spin that hides the cut between the clips.
    let added = mcp.text("add_text", json!({"time": 0.3, "title": "Camina.", "subtitle": "Tus pasos te mueven.", "duration": 2.5}));
    let text_id: u64 = added.rsplit("texto ").next().unwrap().trim_end_matches(')').parse().unwrap();
    mcp.call("update_text", json!({"text_id": text_id, "place": "right", "reveal": "words"}));
    let bad = mcp.request("tools/call", json!({"name": "update_text", "arguments": {"text_id": text_id, "place": "fuera"}}));
    assert_eq!(bad["result"]["isError"], true);
    mcp.call("add_text", json!({"time": 5.0, "title": "100", "subtitle": "ciudades reales.", "size": "large", "reveal": "lines"}));
    mcp.call("add_device_motion", json!({"preset": "full_turn", "time": 3.8, "degrees": 720}));
    let p = mcp.project();
    let texts = p["texts"].as_array().unwrap();
    assert_eq!(texts.len(), 2);
    assert_eq!((texts[0]["place"].as_str(), texts[0]["reveal"].as_str()), (Some("right"), Some("words")));
    let spin = p["device_motion"].as_array().unwrap().iter().find(|d| !d["spin"].is_null()).unwrap().clone();
    assert_eq!(spin["spin"]["degrees"], 720.0);
    // Centred on the cut at 4 s.
    let (a, b) = (spin["start"].as_f64().unwrap(), spin["end"].as_f64().unwrap());
    assert!(((a + b) / 2.0 - 4.0).abs() < 0.01, "{a}..{b}");

    // Split and undo it.
    mcp.call("split_clip", json!({"time": 1.0}));
    assert_eq!(mcp.project()["clips"].as_array().unwrap().len(), 3);
    mcp.call("undo", json!({}));
    assert_eq!(mcp.project()["clips"].as_array().unwrap().len(), 2);
    mcp.call("redo", json!({}));
    assert_eq!(mcp.project()["clips"].as_array().unwrap().len(), 3);
    mcp.call("undo", json!({}));

    // Bad input comes back as a tool error the agent can read.
    let bad = mcp.request("tools/call", json!({"name": "trim_clip", "arguments": {"clip_id": 999999}}));
    assert_eq!(bad["result"]["isError"], true);

    // The agent can look at the result.
    let img = mcp.call("render_frame", json!({"time": 4.0, "width": 320}));
    assert_eq!(img["content"][0]["type"], "image");
    let data = img["content"][0]["data"].as_str().unwrap();
    assert!(data.starts_with("iVBORw0KGgo"), "PNG expected");

    mcp.call("export", json!({"resolution": "720p", "fps": 30, "quality": "standard", "folder": dir.to_string_lossy()}));
    let started = Instant::now();
    let status = loop {
        let s = mcp.text("export_status", json!({}));
        if s.starts_with("Terminado") || s.starts_with("Error") {
            break s;
        }
        assert!(started.elapsed() < Duration::from_secs(240), "export too slow: {s}");
        std::thread::sleep(Duration::from_millis(300));
    };
    assert!(status.starts_with("Terminado"), "{status}");
    let out = status.trim_start_matches("Terminado: ").to_string();
    let probe = Command::new("ffprobe").args(["-v", "error", "-show_entries", "stream=codec_type,width,height", "-of", "csv=p=0"]).arg(&out).output().unwrap();
    let probe = String::from_utf8_lossy(&probe.stdout);
    assert!(probe.contains("video,720,1280") && probe.contains("audio"), "{probe}");
    let _ = std::fs::remove_dir_all(&dir);
}
