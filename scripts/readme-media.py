#!/usr/bin/env python3
"""Make the screenshots and the demo GIF of the README from the real programs.

1. `examples/c3-sensor/build.py` builds the example board through `lamina mcp`
   and keeps a copy of the board after each step.
2. `lamina-viewer shot` (the viewer without a window) takes the screenshots.
3. Pillow puts a caption on each GIF frame and ffmpeg makes the GIF.

Needs: a release build of both programs, the viewer with the `headless`
feature, Python 3 with Pillow, ffmpeg and a network.

    cargo build --release -p lamina-mcp
    cargo build --release -p lamina-viewer --features headless
    python3 scripts/readme-media.py

On a computer without a GPU, a software Vulkan driver works (Mesa lavapipe,
`mesa-vulkan-drivers` on Debian and Ubuntu).
"""
import argparse
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
IMAGES = os.path.join(ROOT, "docs", "images")
SIZE = "1280x760"
ORANGE = (232, 141, 56)


def run(*cmd):
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--bin", default=os.path.join(ROOT, "target", "release"),
                    help="the folder with lamina and lamina-viewer")
    ap.add_argument("--out", default=IMAGES, help="where to write the images")
    a = ap.parse_args()
    lamina = os.path.join(a.bin, "lamina")
    viewer = os.path.join(a.bin, "lamina-viewer")

    work = tempfile.mkdtemp(prefix="lamina-media-")
    stages = os.path.join(work, "stages")
    # The viewer shows the board folder in its header, so use a short path.
    board = os.path.join(tempfile.gettempdir(), "lamina", "c3-sensor")
    frames = os.path.join(work, "frames")
    os.makedirs(frames)

    print("Building the example board ...")
    run(sys.executable, os.path.join(ROOT, "examples", "c3-sensor", "build.py"),
        "--lamina", lamina, "--out", os.path.join(work, "board"), "--stages", stages)
    names = sorted(os.listdir(stages))

    print("Rendering what the agent sees ...")
    sys.dont_write_bytecode = True
    sys.path.insert(0, os.path.join(ROOT, "examples", "c3-sensor"))
    from build import Lamina
    agent = Lamina(lamina)
    agent("open", path=os.path.join(work, "board"))
    r = agent("render", view="layers", width=1200, height=800)
    os.makedirs(a.out, exist_ok=True)
    shutil.copy(os.path.abspath(r["path"]), f"{a.out}/render-layers.png")

    def shot(stage, out, *args):
        shutil.rmtree(board, ignore_errors=True)
        shutil.copytree(os.path.join(stages, stage), board)
        run(viewer, "shot", board, "--out", out, *args)

    final, placed = names[-1], next(n for n in names if n.endswith("-placed"))
    zoom = ["scroll:760,470,3"] * 6
    print("Taking the screenshots ...")
    os.makedirs(a.out, exist_ok=True)
    big = ["--size", SIZE, "--scale", "2"]
    shot(final, f"{a.out}/viewer-3d.png", *big, "--3d", "wait:300", "scroll:560,400,2",
         "hover:1270,700", "wait:300")
    shot(final, f"{a.out}/viewer-2d.png", *big, "hover:1270,700", "wait:200")
    shot(final, f"{a.out}/viewer-pins.png", *big, *zoom, "wait:300")
    shot(placed, f"{a.out}/viewer-checks.png", *big, "hover:1270,700", "wait:200")
    shot(final, f"{a.out}/viewer-notes.png", *big, "click:470,340", "wait:100", "key:Escape",
         "hover:1124,300", "wait:100", "click:1124,400", "wait:100",
         "type:Move C1 right next to U2 pin 3 (VIN). Keep the RGB LED away from the "
         "SHT40 so it does not heat it.", "hover:1124,454", "wait:200")

    print("Taking the GIF frames ...")
    for n in names:
        shot(n, f"{frames}/{n}.png", "--size", SIZE, "hover:1270,700")
    orbit = []
    for i in range(36):
        orbit += ["drag:560,560,594,560", f"shot:{frames}/orbit-{i:02d}.png"]
    shot(final, f"{frames}/3d.png", "--size", SIZE, "--3d", "wait:300", "scroll:560,400,2",
         "wait:200", f"shot:{frames}/orbit-start.png", *orbit)

    print("Making the GIF ...")
    labels = ["USB-C", "regulator", "ESP32-C3", "buttons, LED", "sensor, RGB LED, header"]
    seq = [(names[0], "1", "New board", "set_outline · add_hole", 1.8)]
    seq += [(n, "2", f"Place {labels[i]}", "place_many", 0.75)
            for i, n in enumerate(n for n in names if n.endswith("-place"))]
    seq += [(placed, "3", "Check pins and rules", "pins · check", 1.6),
            (next(n for n in names if n.endswith("-routed")), "4", "Pour GND and route",
             "add_zone · autoroute", 2.4),
            (final, "5", "DRC clean", "check · exclude_violation", 1.6),
            ("orbit-start", "6", "Inspect it in 3D", "lamina-viewer", 1.0)]
    seq += [(f"orbit-{i:02d}", "6", "Inspect it in 3D", "lamina-viewer", 0.07) for i in range(36)]
    seq += [("orbit-35", "6", "Inspect it in 3D", "lamina-viewer", 1.2)]
    gif(frames, seq, f"{a.out}/demo.gif")

    shutil.rmtree(work)
    shutil.rmtree(board, ignore_errors=True)
    print(f"Done: {a.out}")


def gif(frames, seq, out, width=960):
    from PIL import Image, ImageDraw, ImageFont

    def font(names, size):
        for n in names:
            try:
                return ImageFont.truetype(n, size)
            except OSError:
                pass
        return ImageFont.load_default(size)

    sans = font(["Inter-SemiBold.otf", "/usr/share/fonts/opentype/inter/Inter-SemiBold.otf",
                 "DejaVuSans-Bold.ttf", "Arial Bold.ttf", "arialbd.ttf"], 26)
    mono = font(["DejaVuSansMono.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
                 "Menlo.ttc", "consola.ttf"], 25)
    tmp = os.path.join(frames, "out")
    os.makedirs(tmp)
    lines = []
    for i, (name, step, label, tools, secs) in enumerate(seq):
        img = Image.open(f"{frames}/{name}.png").convert("RGB")
        d = ImageDraw.Draw(img, "RGBA")
        parts = [(f"{step}  ", sans, ORANGE), (label + "   ", sans, (240, 240, 245)),
                 (tools, mono, (170, 178, 190))]
        widths = [d.textlength(t, font=f) for t, f, _ in parts]
        pad, h = 22, 54
        # The middle of the board canvas of the viewer at 1280 x 760.
        x0, y0 = 602 - sum(widths) / 2 - pad, 636
        d.rounded_rectangle([x0, y0, x0 + sum(widths) + 2 * pad, y0 + h], radius=h // 2,
                            fill=(14, 16, 21, 235), outline=ORANGE + (160,), width=2)
        x = x0 + pad
        for (t, f, col), w in zip(parts, widths):
            d.text((x, y0 + h / 2), t, font=f, fill=col, anchor="lm")
            x += w
        img = img.resize((width, round(img.height * width / img.width)), Image.LANCZOS)
        p = f"{tmp}/{i:03d}.png"
        img.save(p)
        lines.append(f"file '{p}'\nduration {secs}\n")
    lines.append(f"file '{tmp}/{len(seq) - 1:03d}.png'\n")
    with open(f"{tmp}/list.txt", "w") as f:
        f.write("".join(lines))
    run("ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0",
        "-i", f"{tmp}/list.txt", "-vf",
        "fps=14,split[a][b];[a]palettegen=max_colors=192:stats_mode=diff[p];"
        "[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle",
        "-loop", "0", out)


if __name__ == "__main__":
    main()
