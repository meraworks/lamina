//! `lamina-viewer shot`: the viewer without a window. It opens a board,
//! plays a list of input steps (keys, mouse moves, clicks, scrolls) and
//! saves the screen as PNG images, like a headless browser.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, Vec2};
use eframe::egui_wgpu;
use egui_kittest::Harness;

use crate::Shell;

pub const USAGE: &str = "Usage: lamina-viewer shot BOARD_DIR [options] [STEP]...

Runs the viewer without a window and saves PNG images of it. The steps run
in order. Coordinates are in points from the top-left corner of the window.

Options:
  --size WxH     the window size in points (default 1600x900)
  --scale F      pixels for each point (default 1)
  --3d           start in the 3D view
  --out FILE     the PNG image to save after the last step (default shot.png)
  --edit         let the steps change the board files. Without it, the
                 viewer does not write to the board: a drag or a delete
                 shows in the image only

Steps:
  key:K          press a key, for example key:2, key:F, key:Escape, key:ctrl+z
  type:TEXT      type text into the focused field
  hover:X,Y      move the mouse
  click:X,Y      click the left button
  rclick:X,Y     click the right button
  drag:X,Y,X2,Y2 drag with the left button
  scroll:X,Y,N   turn the mouse wheel N notches at X,Y (up is positive)
  wait:MS        let the viewer run for MS milliseconds
  shot:FILE      save an image now";

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("lamina-viewer shot: {e}");
            if e.starts_with("usage") {
                eprintln!("\n{USAGE}");
            }
            2
        }
    }
}

struct Opts {
    root: PathBuf,
    size: Vec2,
    scale: f32,
    three_d: bool,
    out: PathBuf,
    edit: bool,
    steps: Vec<String>,
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut size = Vec2::new(1600.0, 900.0);
    let mut scale = 1.0;
    let mut three_d = false;
    let mut edit = false;
    let mut out = PathBuf::from("shot.png");
    let mut root = None;
    let mut steps = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or(format!("usage: {a} needs a value"))
        };
        match a.as_str() {
            "-h" | "--help" => return Err("usage".into()),
            "--size" => {
                let v = value()?;
                let (w, h) = v
                    .split_once(['x', 'X'])
                    .ok_or(format!("usage: bad size {v}"))?;
                size = Vec2::new(positive(w)?, positive(h)?);
            }
            "--scale" => scale = positive(&value()?)?,
            "--3d" => three_d = true,
            "--edit" => edit = true,
            "--out" => out = png(&value()?)?,
            s if s.starts_with('-') => return Err(format!("usage: {s} is not a known option")),
            // The first argument that is not a step is the board. A Windows
            // path has a colon too, so a folder that exists is the board.
            s if root.is_none() && (is_step(s).is_none() || Path::new(s).exists()) => {
                root = Some(PathBuf::from(s))
            }
            s => {
                check(s)?;
                steps.push(s.to_string())
            }
        }
    }
    Ok(Opts {
        root: root.ok_or("usage: give a board folder")?,
        size,
        scale,
        three_d,
        out,
        edit,
        steps,
    })
}

/// A number that is more than zero.
fn positive(s: &str) -> Result<f32, String> {
    let v = num(s)?;
    if !(v > 0.0 && v.is_finite()) {
        return Err(format!("usage: {s} must be more than 0"));
    }
    Ok(v)
}

fn png(s: &str) -> Result<PathBuf, String> {
    if !s.to_ascii_lowercase().ends_with(".png") {
        return Err(format!("usage: {s}: the image must be a .png file"));
    }
    Ok(PathBuf::from(s))
}

/// Find the errors in a step before the board loads, so that a typing error
/// does not wait for the load.
fn check(step: &str) -> Result<(), String> {
    let (verb, arg) = is_step(step).ok_or(format!("usage: {step} is not a known step"))?;
    match verb {
        "key" => {
            let name = modifiers(arg).1;
            Key::from_name(name).ok_or(format!("usage: {name} is not a key name"))?;
        }
        "hover" | "click" | "rclick" => {
            nums(arg, 2)?;
        }
        "drag" => {
            nums(arg, 4)?;
        }
        "scroll" => {
            nums(arg, 3)?;
        }
        "wait" => {
            let ms = num(arg)?;
            if !(0.0..=600_000.0).contains(&ms) {
                return Err(format!("usage: wait:{arg} must be 0 to 600000 ms"));
            }
        }
        "shot" => {
            png(arg)?;
        }
        _ => {}
    }
    Ok(())
}

const VERBS: [&str; 9] = [
    "key", "type", "hover", "click", "rclick", "drag", "scroll", "wait", "shot",
];

/// The verb and the argument of a step, if the text is a step.
fn is_step(s: &str) -> Option<(&str, &str)> {
    s.split_once(':').filter(|(v, _)| VERBS.contains(v))
}

fn num(s: &str) -> Result<f32, String> {
    s.trim().parse().map_err(|_| format!("{s} is not a number"))
}

fn nums(s: &str, n: usize) -> Result<Vec<f32>, String> {
    let v: Vec<f32> = s.split(',').map(num).collect::<Result<_, _>>()?;
    if v.len() != n {
        return Err(format!("{s}: give {n} numbers"));
    }
    Ok(v)
}

fn run(args: &[String]) -> Result<(), String> {
    let o = parse(args)?;
    if !lamina_model::ProjectData::is_project(&o.root) {
        return Err(format!(
            "{} is not a Lamina project folder",
            o.root.display()
        ));
    }
    let (root, three_d) = (o.root.clone(), o.three_d);
    let mut h = Harness::builder()
        .with_size(o.size)
        .with_pixels_per_point(o.scale)
        .wgpu_setup(gpu_setup())
        // A missing glyph shows as a box in the image, as in the window.
        .allow_missing_glyphs()
        .build_eframe(move |cc| {
            let mut s = Shell::new(cc, vec![root], three_d);
            s.remember = false;
            s
        });
    if !o.edit {
        // Without an editor, the viewer does not write to the board files.
        for b in &mut h.state_mut().boards {
            b.editor = None;
        }
    }
    if let Err(e) = settle(&mut h, Duration::from_secs(20)) {
        // Save the image anyway: it shows the error panel of the viewer.
        save(&mut h, &o.out)?;
        return Err(e);
    }
    for step in &o.steps {
        play(&mut h, step)?;
    }
    idle(&mut h);
    save(&mut h, &o.out)
}

/// A wgpu setup without a window that prefers a real GPU. The default
/// setup of the test harness prefers a software rasterizer, which is too
/// slow for a large board.
fn gpu_setup() -> egui_wgpu::WgpuSetup {
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    setup
        .instance_descriptor
        .backends
        .remove(egui_wgpu::wgpu::Backends::BROWSER_WEBGPU);
    setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters, _surface| {
        use egui_wgpu::wgpu::DeviceType;
        adapters
            .iter()
            .min_by_key(|a| match a.get_info().device_type {
                DeviceType::DiscreteGpu => 0,
                DeviceType::IntegratedGpu => 1,
                DeviceType::Other | DeviceType::VirtualGpu => 2,
                DeviceType::Cpu => 3,
            })
            .cloned()
            .ok_or_else(|| "No GPU adapter found".to_owned())
    }));
    egui_wgpu::WgpuSetup::CreateNew(setup)
}

/// Run frames until the boards are loaded, or the time is over.
fn settle(h: &mut Harness<'_, Shell>, max: Duration) -> Result<(), String> {
    let end = Instant::now() + max;
    let mut quiet = 0;
    while Instant::now() < end {
        frames(h, 1);
        if let Some(e) = h.state().boards.iter().find_map(|b| b.error.clone()) {
            return Err(format!("the board did not load: {e}"));
        }
        let ready = h.state().boards.iter().all(|b| b.loaded.is_some());
        quiet = if ready { quiet + 1 } else { 0 };
        // A few more frames after the load, for the fit and the GPU images.
        if quiet >= 6 {
            return Ok(());
        }
    }
    Err("the board did not load in time".into())
}

/// Run frames until the editor has written all the changes and the viewer
/// has read the board again, so that the image shows the result of the steps.
fn idle(h: &mut Harness<'_, Shell>) {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        let busy = h
            .state()
            .boards
            .iter()
            .any(|b| b.editor.as_ref().is_some_and(|e| e.busy > 0));
        if !busy {
            break;
        }
        frames(h, 1);
    }
    frames(h, 3);
}

fn frames(h: &mut Harness<'_, Shell>, n: usize) {
    for _ in 0..n {
        h.step();
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn play(h: &mut Harness<'_, Shell>, step: &str) -> Result<(), String> {
    let (verb, arg) = is_step(step).ok_or(format!("usage: {step} is not a known step"))?;
    match verb {
        "key" => {
            let (mods, name) = modifiers(arg);
            let key = Key::from_name(name).ok_or(format!("{name} is not a key name"))?;
            h.key_press_modifiers(mods, key);
        }
        "type" => h.event(Event::Text(arg.to_string())),
        "hover" => {
            let v = nums(arg, 2)?;
            h.hover_at(Pos2::new(v[0], v[1]));
        }
        "click" | "rclick" => {
            let v = nums(arg, 2)?;
            let pos = Pos2::new(v[0], v[1]);
            let button = if verb == "click" {
                PointerButton::Primary
            } else {
                PointerButton::Secondary
            };
            h.hover_at(pos);
            frames(h, 1);
            for pressed in [true, false] {
                h.event(Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers: Modifiers::NONE,
                });
                frames(h, 1);
            }
        }
        "drag" => {
            let v = nums(arg, 4)?;
            let (a, b) = (Pos2::new(v[0], v[1]), Pos2::new(v[2], v[3]));
            h.hover_at(a);
            frames(h, 1);
            h.drag_at(a);
            frames(h, 1);
            for i in 1..=8 {
                h.hover_at(a + (b - a) * (i as f32 / 8.0));
                frames(h, 1);
            }
            h.drop_at(b);
        }
        "scroll" => {
            let v = nums(arg, 3)?;
            h.hover_at(Pos2::new(v[0], v[1]));
            frames(h, 1);
            h.event(Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                delta: Vec2::new(0.0, v[2]),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE,
                source: egui::MouseWheelSource::Wheel,
            });
        }
        "wait" => {
            let end = Instant::now() + Duration::from_millis(num(arg)? as u64);
            while Instant::now() < end {
                frames(h, 1);
            }
        }
        "shot" => {
            idle(h);
            return save(h, Path::new(arg));
        }
        _ => unreachable!("is_step returns known verbs only"),
    }
    frames(h, 4);
    Ok(())
}

/// `ctrl+shift+z` gives the modifiers and `z`.
fn modifiers(s: &str) -> (Modifiers, &str) {
    let mut m = Modifiers::NONE;
    let mut rest = s;
    while let Some((head, tail)) = rest.split_once('+') {
        match head.to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" => m |= Modifiers::COMMAND,
            "shift" => m |= Modifiers::SHIFT,
            "alt" => m |= Modifiers::ALT,
            _ => break,
        }
        rest = tail;
    }
    (m, rest)
}

fn save(h: &mut Harness<'_, Shell>, path: &Path) -> Result<(), String> {
    let img = h.render()?;
    img.save(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    println!("{}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_windows_path_is_the_board_and_steps_follow() {
        let o = parse(&args(&[r"C:\boards\a", "--3d", "key:2", "hover:10,20"])).unwrap();
        assert_eq!(o.root, PathBuf::from(r"C:\boards\a"));
        assert!(o.three_d);
        assert_eq!(o.steps, ["key:2", "hover:10,20"]);
    }

    #[test]
    fn modifiers_split_from_the_key() {
        assert_eq!(
            modifiers("ctrl+shift+z"),
            (Modifiers::COMMAND | Modifiers::SHIFT, "z")
        );
        assert_eq!(modifiers("2"), (Modifiers::NONE, "2"));
        assert_eq!(modifiers("+"), (Modifiers::NONE, "+"));
    }

    #[test]
    fn bad_input_is_refused() {
        assert!(parse(&args(&["key:2"])).is_err());
        assert!(parse(&args(&["b", "--size", "10"])).is_err());
        assert!(parse(&args(&["b", "--size", "0x10"])).is_err());
        assert!(parse(&args(&["b", "--out", "a.jpg"])).is_err());
        assert!(parse(&args(&["b", "key:NoSuchKey"])).is_err());
        assert!(parse(&args(&["b", "wait:inf"])).is_err());
        assert!(parse(&args(&["b", "hover:1"])).is_err());
        assert!(parse(&args(&["b", "--size", "800X600"])).is_ok());
        assert!(nums("1,2", 3).is_err());
    }
}
