#!/usr/bin/env python3
"""Make the screenshots and the demo GIF of the README from the real programs.

1. `examples/c3-sensor/build.py` builds the example board through `lamina mcp`
   and keeps a copy of the board after each step.
2. `lamina-viewer shot` (the viewer without a window) takes the screenshots.
3. Pillow puts a caption on each GIF frame and ffmpeg makes the GIF.
4. Pillow puts the 3D render of `render` next to the logo and the name for the
   social preview (docs/images/social-preview.png). Upload it again on GitHub in
   Settings → Social preview after it changes.

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

    print("Making the social preview ...")
    # Bigger than needed, for a clean edge when the board is cut out and made smaller.
    view = dict(view="3d", width=1600, height=1200, yaw=-58, pitch=40, zoom=1.08)
    r = agent("render", name="social-board", **view)
    board3d = os.path.join(work, "social-board.png")
    shutil.copy(os.path.abspath(r["path"]), board3d)
    # The same view of an empty place far from the board: only the background.
    far = {"min": {"x": 5000, "y": 5000}, "max": {"x": 5001, "y": 5001}}
    r = agent("render", name="social-bg", region=far, **view)
    social_preview(board3d, os.path.abspath(r["path"]), f"{a.out}/social-preview.png")

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


def font(names, size):
    from PIL import ImageFont

    for n in names:
        try:
            return ImageFont.truetype(n, size)
        except OSError:
            pass
    return ImageFont.load_default(size)


def inter(weight, size):
    return font([f"Inter-{weight}.otf", f"/usr/share/fonts/opentype/inter/Inter-{weight}.otf",
                 "DejaVuSans-Bold.ttf" if weight == "Bold" else "DejaVuSans.ttf",
                 "Arial.ttf", "arial.ttf"], size)


# The social preview of the repository (Settings → Social preview on GitHub).
SOCIAL = (1280, 640)
SOCIAL_BOARD = (650, 80, 1268, 580)  # The box that the board fits in.
SOCIAL_TOP, SOCIAL_BOTTOM = (71, 75, 84), (34, 36, 42)
LOGO = (224, 138, 60)  # #E08A3C, as in assets/logo.svg


def logo(scale):
    """Draw assets/logo.svg: the track from the via at the top to the pad at the
    bottom right. The image covers the viewBox (12 4 44 56) at `scale` px per unit."""
    from PIL import Image, ImageDraw

    ss = 4  # Draw four times bigger and make it smaller, for smooth edges.
    k = scale * ss
    img = Image.new("L", (round(44 * k), round(56 * k)), 0)
    d = ImageDraw.Draw(img)

    def p(x, y):
        return ((x - 12) * k, (y - 4) * k)

    def disc(x, y, r, fill):
        (cx, cy) = p(x, y)
        d.ellipse([cx - r * k, cy - r * k, cx + r * k, cy + r * k], fill=fill)

    track = [(22, 18), (22, 40), (32, 50), (42, 50)]
    d.line([p(*q) for q in track], fill=255, width=round(7 * k), joint="curve")
    for q in track:  # Round caps and joins.
        disc(*q, 3.5, 255)
    disc(22, 14, 7, 255)
    disc(46, 50, 6, 255)
    disc(22, 14, 2.6, 0)
    disc(46, 50, 2.4, 0)
    mask = img.resize((round(44 * scale), round(56 * scale)), Image.LANCZOS)
    out = Image.new("RGBA", mask.size, LOGO + (0,))
    out.putalpha(mask)
    return out


def board_cutout(render, background, tolerance=6):
    """Cut the board out of a 3D render. `background` is a render of the same size
    without the board. A pixel is board where it differs from the background, and
    a hole inside the board (a dark part) stays board. Return an RGBA image."""
    from PIL import Image, ImageChops, ImageDraw, ImageFilter

    render = Image.open(render).convert("RGB")
    background = Image.open(background).convert("RGB")
    if render.size != background.size:
        raise ValueError("the render and the background must have the same size")
    diff = ImageChops.difference(render, background).convert("L")
    # 255 where the background shows, 0 where the board is.
    bg = diff.point(lambda v: 255 if v <= tolerance else 0)
    # Fill the background from each edge pixel. What stays 255 is a hole in the board.
    w, h = bg.size
    edge = [(x, y) for x in range(0, w, 8) for y in (0, h - 1)]
    edge += [(x, y) for y in range(0, h, 8) for x in (0, w - 1)]
    for xy in edge:
        if bg.getpixel(xy) == 255:
            ImageDraw.floodfill(bg, xy, 128)
    mask = bg.point(lambda v: 0 if v == 128 else 255)
    # Drop the one pixel of edge that is mixed with the background.
    mask = mask.filter(ImageFilter.MinFilter(3))
    out = render.convert("RGBA")
    out.putalpha(mask)
    box = mask.getbbox()
    if box is None:
        raise ValueError("the render shows no board")
    return out.crop(box)


def social_preview(render, background, out):
    """Make the 1280 x 640 social preview: the logo, the name and the tagline at
    the left, the 3D board of the example at the right."""
    from PIL import Image, ImageDraw, ImageFilter

    w, h = SOCIAL
    img = Image.new("RGB", SOCIAL)
    d = ImageDraw.Draw(img)
    for y in range(h):
        t = y / (h - 1)
        d.line([(0, y), (w, y)],
               fill=tuple(round(a + (b - a) * t) for a, b in zip(SOCIAL_TOP, SOCIAL_BOTTOM)))
    img = img.convert("RGBA")

    board = board_cutout(render, background)
    x0, y0, x1, y1 = SOCIAL_BOARD
    s = min((x1 - x0) / board.width, (y1 - y0) / board.height)
    board = board.resize((round(board.width * s), round(board.height * s)), Image.LANCZOS)
    pos = (x0 + (x1 - x0 - board.width) // 2, y0 + (y1 - y0 - board.height) // 2)
    # A soft shadow under the board.
    shadow = Image.new("RGBA", board.size, (8, 9, 12, 0))
    shadow.putalpha(board.getchannel("A").point(lambda v: v * 110 // 255))
    pad = 40
    soft = Image.new("RGBA", (board.width + 2 * pad, board.height + 2 * pad), (0, 0, 0, 0))
    soft.paste(shadow, (pad, pad))
    soft = soft.filter(ImageFilter.GaussianBlur(18))
    img.alpha_composite(soft, (pos[0] - pad + 6, pos[1] - pad + 18))
    img.alpha_composite(board, pos)

    img.alpha_composite(logo(4), (84, 96))
    d = ImageDraw.Draw(img)
    d.text((298, 248), "Lamina", font=inter("Bold", 94), fill=(245, 245, 247), anchor="ls")
    d.text((84, 407), "PCB layout for AI coding agents", font=inter("Regular", 32),
           fill=(222, 224, 230), anchor="ls")
    d.text((84, 453), "Open source  ·  Rust  ·  MCP", font=inter("Regular", 25),
           fill=(160, 165, 178), anchor="ls")
    d.text((84, 599), "github.com/meraworks/lamina", font=inter("Regular", 25),
           fill=(160, 165, 178), anchor="ls")
    img.convert("RGB").save(out, optimize=True)


def gif(frames, seq, out, width=960):
    from PIL import Image, ImageDraw

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
