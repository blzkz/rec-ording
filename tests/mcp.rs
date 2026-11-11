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
    // What an agent needs to discover: marks instead of times, and the project to work on.
    let zoom = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "add_zoom").unwrap();
    assert!(zoom["inputSchema"]["properties"]["mark"].is_object() && zoom["inputSchema"]["properties"]["project"].is_object());
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
    assert!(msg.contains("2 shots") && !msg.contains("replace"), "{msg}");
    mcp.call("add_device_motion", json!({"preset": "hop", "time": 1.0}));
    mcp.call("set_style", json!({"aspect": "9:16", "background_preset": "Atardecer", "padding": 0.1}));
    mcp.call("set_angle", json!({"turn": -15, "straighten_on_zoom": true}));
    mcp.call("set_device", json!({"model": "Galaxy S26 Ultra"}));
    mcp.call("set_model_options", json!({"video_fit": "fill", "fit_pan": 0.3}));

    let p = mcp.project();
    let kinds: Vec<&str> = p["camera"].as_array().unwrap().iter().map(|c| c["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"zoom") && kinds.contains(&"move") && kinds.iter().filter(|k| **k == "director").count() == 2, "{kinds:?}");
    assert_eq!(p["style"]["aspect"], "9:16");
    assert_eq!(p["style"]["frame"], "model");
    assert_eq!(p["device_motion"].as_array().unwrap().len(), 1);

    // Titles beside the device, and a double spin that hides the cut between the clips.
    let added = mcp.text("add_text", json!({"time": 0.3, "title": "Camina.", "subtitle": "Tus pasos te mueven.", "duration": 2.5}));
    let text_id: u64 = added.rsplit("(text ").next().unwrap().trim_end_matches(')').parse().unwrap();
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
    // Turn it the other way, slower, a bit later.
    let spin_id = spin["id"].as_u64().unwrap();
    mcp.call("update_device_motion", json!({"clip_id": spin_id, "degrees": -360, "duration": 1.5, "start": 3.5}));
    let p = mcp.project();
    let spin = p["device_motion"].as_array().unwrap().iter().find(|d| d["id"] == spin_id).unwrap().clone();
    assert_eq!(spin["spin"]["degrees"], -360.0);
    assert!((spin["start"].as_f64().unwrap() - 3.5).abs() < 1e-4 && (spin["end"].as_f64().unwrap() - 5.0).abs() < 1e-4, "{spin}");
    let hop = p["device_motion"].as_array().unwrap().iter().find(|d| d["spin"].is_null()).unwrap()["id"].as_u64().unwrap();
    let bad = mcp.request("tools/call", json!({"name": "update_device_motion", "arguments": {"clip_id": hop, "degrees": 360}}));
    assert_eq!(bad["result"]["isError"], true);
    let hw: Value = serde_json::from_str(&mcp.text("get_hardware", json!({}))).unwrap();
    assert!(hw.get("graphics_card").is_some() && hw.get("encoder").is_some(), "{hw}");
    assert_eq!(p["export"]["hardware"], true);

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
    let strip = mcp.call("render_strip", json!({"start": 0.0, "end": 6.0, "count": 6, "width": 160}));
    assert_eq!(strip["content"][0]["type"], "image");
    let r = mcp.text("render_preview", json!({"start": 3.0, "end": 4.5, "width": 200, "fps": 12}));
    let path = r.split(" at ").nth(1).and_then(|s| s.rsplit_once(" (")).map(|(p, _)| p.to_string()).expect(&r);
    let frames = Command::new("ffprobe").args(["-v", "error", "-count_frames", "-show_entries", "stream=nb_read_frames,width", "-of", "csv=p=0"]).arg(&path).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&frames.stdout).trim(), "200,18", "{r}");
    let _ = std::fs::remove_file(&path);

    // Screen changes as marks to anchor things to.
    let marks = mcp.text("detect_marks", json!({}));
    assert!(marks.contains("screen changes"), "{marks}");
    let n = mcp.project()["marks"].as_array().unwrap().len();
    let r = mcp.request("tools/call", json!({"name": "add_zoom", "arguments": {"mark": n + 5}}));
    assert_eq!(r["result"]["isError"], true);

    // A logo with transparency and a sound effect, on their tracks.
    let logo = dir.join("logo.png");
    let ok = Command::new("ffmpeg")
        .args(["-nostdin", "-y", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=white@0.6:s=64x32,format=rgba", "-frames:v", "1"])
        .arg(&logo)
        .status()
        .unwrap()
        .success();
    let pop = dir.join("pop.wav");
    let ok = ok
        && Command::new("ffmpeg")
            .args(["-nostdin", "-y", "-loglevel", "error", "-f", "lavfi", "-i", "sine=frequency=880:duration=0.4"])
            .arg(&pop)
            .status()
            .unwrap()
            .success();
    assert!(ok);
    let added = mcp.text("add_image", json!({"path": logo.to_string_lossy(), "time": 0.5, "duration": 2, "x": 0.8, "y": 0.1, "width": 0.2}));
    assert!(added.contains("image"), "{added}");
    let added = mcp.text("add_sound", json!({"path": pop.to_string_lossy(), "track": "effects", "time": 1.0, "volume": 0.8}));
    assert!(added.contains("effects"), "{added}");
    let p = mcp.project();
    assert_eq!((p["images"].as_array().unwrap().len(), p["sounds"].as_array().unwrap().len()), (1, 1));
    assert_eq!(p["sounds"][0]["track"], "effects");
    let r = mcp.request("tools/call", json!({"name": "add_sound", "arguments": {"path": video, "track": "nope", "time": 0}}));
    assert_eq!(r["result"]["isError"], true);

    // The lightest finish: CI machines export with two cores and no graphics card (blur and
    // edges are checked against the GPU in the unit tests).
    mcp.call("set_style", json!({"motion_blur": "off", "antialias": "normal"}));
    mcp.call("export", json!({"resolution": "720p", "fps": 30, "quality": "standard", "folder": dir.to_string_lossy()}));
    let started = Instant::now();
    let status = loop {
        let s = mcp.text("export_status", json!({}));
        if s.starts_with("Done") || s.starts_with("Error") {
            break s;
        }
        assert!(started.elapsed() < Duration::from_secs(600), "export too slow: {s}");
        std::thread::sleep(Duration::from_millis(300));
    };
    assert!(status.starts_with("Done"), "{status}");
    let out = status.trim_start_matches("Done: ").to_string();
    let probe = Command::new("ffprobe").args(["-v", "error", "-show_entries", "stream=codec_type,width,height", "-of", "csv=p=0"]).arg(&out).output().unwrap();
    let probe = String::from_utf8_lossy(&probe.stdout);
    assert!(probe.contains("video,720,1280") && probe.contains("audio"), "{probe}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn batches_projects_and_title_files() {
    let dir = std::env::temp_dir().join(format!("rec-ording-mcp-b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut mcp = Mcp::start(&dir);
    mcp.request("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}));
    mcp.call("create_project", json!({"name": "Uno"}));
    mcp.call("create_project", json!({"name": "Dos"}));

    // Every answer names the project it acted on; `project` picks another one.
    let r = mcp.call("add_text", json!({"time": 0.0, "title": "En Uno", "project": "Uno"}));
    assert_eq!(r["content"][1]["text"], "Project: “Uno”");
    let r = mcp.call("get_project", json!({}));
    assert_eq!(r["content"][1]["text"], "Project: “Dos”");
    let uno: Value = serde_json::from_str(mcp.call("get_project", json!({"project": "Uno"}))["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(uno["texts"].as_array().unwrap().len(), 1);
    assert_eq!(mcp.project()["texts"].as_array().unwrap().len(), 0);

    // A batch is all or nothing, and one undo step.
    let bad = mcp.request("tools/call", json!({"name": "batch", "arguments": {"operations": [
        {"name": "add_text", "arguments": {"time": 0.0, "title": "A"}},
        {"name": "delete_text", "arguments": {"text_id": 999}}
    ]}}));
    assert_eq!(bad["result"]["isError"], true);
    assert_eq!(mcp.project()["texts"].as_array().unwrap().len(), 0);
    mcp.call("batch", json!({"operations": [
        {"name": "add_text", "arguments": {"time": 0.0, "title": "A", "duration": 2}},
        {"name": "add_text", "arguments": {"time": 5.0, "title": "B", "place": "right"}},
        {"name": "rename_project", "arguments": {"name": "Dos bis"}}
    ]}));
    let p = mcp.project();
    assert_eq!((p["texts"].as_array().unwrap().len(), p["name"].as_str()), (2, Some("Dos bis")));
    mcp.call("undo", json!({}));
    let p = mcp.project();
    assert_eq!((p["texts"].as_array().unwrap().len(), p["name"].as_str()), (0, Some("Dos")));
    mcp.call("redo", json!({}));

    // Titles out to JSON and back in.
    let texts: Value = serde_json::from_str(&mcp.text("export_texts", json!({}))).unwrap();
    assert_eq!(texts[1]["place"], "right");
    let mut edited = texts.clone();
    edited[1]["title"] = json!("B editado");
    mcp.call("import_texts", json!({"texts": edited}));
    let p = mcp.project();
    assert_eq!(p["texts"][1]["title"], "B editado");
    assert_eq!(p["texts"].as_array().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}
