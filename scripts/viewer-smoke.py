#!/usr/bin/env python3
"""Smoke test of `lamina-viewer shot`, the viewer without a window.

1. `lamina mcp` makes a small board in a temporary folder. The footprints
   come from `footprint_from_package`, so no network is needed.
2. `lamina-viewer shot` plays steps of each kind on the board, in 2D and
   in 3D, and saves PNG images.
3. The test fails if a run does not exit with 0, takes too long, uses too
   much memory or loads the board more than once (a reload loop), or if an
   image has the wrong size, is blank, or the 2D and 3D images are the same.

Needs: Python 3 (no other packages), both programs, the viewer with the
`headless` feature, and a GPU or a software Vulkan driver (Mesa lavapipe,
`mesa-vulkan-drivers` on Debian and Ubuntu). It measures the memory on
Linux only.

    cargo build --release -p lamina-mcp
    cargo build --release -p lamina-viewer --features headless
    python3 scripts/viewer-smoke.py --bin target/release
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = ".exe" if os.name == "nt" else ""

# Window size in points and pixels for each point. The scale is not a whole
# number, to test the rounding of the image size.
SIZE = (960, 600)
SCALE = 1.25


class Lamina:
    """A small MCP client: JSON-RPC over the stdin and stdout of `lamina mcp`."""

    def __init__(self, exe):
        self.p = subprocess.Popen([exe, "mcp"], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True, bufsize=1)
        self.next_id = 0
        self.request("initialize", {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "viewer-smoke", "version": "1"},
        })
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def send(self, msg):
        self.p.stdin.write(json.dumps(msg) + "\n")
        self.p.stdin.flush()

    def request(self, method, params):
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params})
        while True:
            line = self.p.stdout.readline()
            if not line:
                fail("lamina mcp stopped")
            msg = json.loads(line)
            if msg.get("id") == self.next_id:
                if "error" in msg:
                    fail(f"{method}: {msg['error']}")
                return msg["result"]

    def __call__(self, tool, **args):
        r = self.request("tools/call", {"name": tool, "arguments": args})
        text = "\n".join(c.get("text", "") for c in r.get("content", []) if c.get("type") == "text")
        if r.get("isError"):
            fail(f"{tool}: {text}")
        return text

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=30)


def fail(msg):
    print(f"FAIL: {msg}", file=sys.stderr)
    sys.exit(1)


def make_board(lamina, path):
    """A board of 30 x 20 mm with three parts, tracks and a ground pour."""
    m = Lamina(lamina)
    m("open", path=path, create=True)
    m("use_fab_preset", name="jlcpcb-2layer")
    m("set_outline", rect={"width": 30, "height": 20, "corner_radius": 1})
    m("footprint_from_package", package="R_0603")
    m("footprint_from_package", package="SOIC-8")
    m("add_parts", parts=[
        {"refdes": "R1", "footprint": "R_0603", "value": "10k"},
        {"refdes": "R2", "footprint": "R_0603", "value": "10k"},
        {"refdes": "U1", "footprint": "SOIC-8", "value": "NE555"},
    ])
    m("place_many", placements={
        "R1": {"x": 6, "y": 10, "rotation": 90},
        "R2": {"x": 24, "y": 10},
        "U1": {"x": 15, "y": 10},
    })
    m("set_nets", nets={
        "SIG": {"pads": ["R1.2", "U1.2"]},
        "OUT": {"pads": ["U1.3", "R2.1"]},
        "GND": {"pads": ["U1.1", "R1.1", "R2.2"]},
    })
    m("autoroute", skip=["GND"])
    m("add_zone", net="GND", layers=["top", "bottom"])
    m.close()


def peak_rss_mb(pid):
    """The peak memory of a running process in MB (Linux), or None."""
    try:
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                if line.startswith("VmHWM:"):
                    return int(line.split()[1]) / 1024
    except OSError:
        pass
    return None


def run_shot(viewer, args, max_s, max_mb):
    """Run `lamina-viewer shot --stats`. Stop it when it is over the time or
    the memory limit. Return (seconds, peak MB or None, stats)."""
    cmd = [viewer, "shot", "--stats", *args]
    print("$", " ".join(cmd), flush=True)
    t0 = time.monotonic()
    # The output is a few lines, so the pipes do not fill while it runs.
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    peak = None
    problem = None
    while p.poll() is None:
        mb = peak_rss_mb(p.pid)
        if mb is not None:
            peak = max(peak or 0, mb)
        if peak is not None and peak > max_mb:
            problem = f"uses {peak:.0f} MB, more than {max_mb} MB (a reload loop?)"
        elif time.monotonic() - t0 > max_s:
            problem = f"did not finish in {max_s} s (a hang or a reload loop?)"
        if problem:
            p.kill()
            break
        time.sleep(0.05)
    out, err = p.communicate()
    took = time.monotonic() - t0
    print(out + err, end="")
    if problem:
        fail(problem)
    if p.returncode != 0:
        fail(f"exit code {p.returncode}")
    stats = [line for line in err.splitlines() if line.startswith("{")]
    if not stats:
        fail("no --stats line")
    stats = json.loads(stats[-1])
    print(f"  {took:.1f} s, peak {'?' if peak is None else f'{peak:.0f}'} MB, "
          f"{stats['loads']} board load(s)", flush=True)
    # Nothing writes the board during the run, so it loads once only. More
    # loads are a reload loop, which also grows the memory on a large board.
    if stats["loads"] != 1:
        fail(f"the board loaded {stats['loads']} times, not once (a reload loop?)")
    return took, peak, stats


def read_png(path):
    """Decode an 8-bit, not interlaced PNG. Return (width, height, channels,
    rows of bytes)."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        fail(f"{path} is not a PNG")
    pos, idat, head = 8, [], None
    while pos < len(data):
        n = int.from_bytes(data[pos:pos + 4], "big")
        kind = data[pos + 4:pos + 8]
        body = data[pos + 8:pos + 8 + n]
        pos += 12 + n
        if kind == b"IHDR":
            head = body
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    w, h = int.from_bytes(head[0:4], "big"), int.from_bytes(head[4:8], "big")
    depth, color, interlace = head[8], head[9], head[12]
    channels = {0: 1, 2: 3, 4: 2, 6: 4}.get(color)
    if depth != 8 or channels is None or interlace != 0:
        fail(f"{path}: depth {depth}, color type {color}, interlace {interlace} not supported")
    raw = zlib.decompress(b"".join(idat))
    stride = w * channels
    rows, prev = [], bytearray(stride)
    for y in range(h):
        i = y * (stride + 1)
        kind, line = raw[i], bytearray(raw[i + 1:i + 1 + stride])
        if kind == 1:
            for x in range(channels, stride):
                line[x] = (line[x] + line[x - channels]) & 255
        elif kind == 2:
            line = bytearray((a + b) & 255 for a, b in zip(line, prev))
        elif kind == 3:
            for x in range(stride):
                left = line[x - channels] if x >= channels else 0
                line[x] = (line[x] + ((left + prev[x]) >> 1)) & 255
        elif kind == 4:
            for x in range(stride):
                a = line[x - channels] if x >= channels else 0
                b, c = prev[x], prev[x - channels] if x >= channels else 0
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[x] = (line[x] + pred) & 255
        rows.append(line)
        prev = line
    return w, h, channels, rows


def gray(img, x0=0.0, x1=1.0, y0=0.0, y1=1.0):
    """Gray levels of the pixels in a part of the image (in fractions of the
    size), with every second pixel in x and y."""
    w, h, ch, rows = img
    out = []
    for y in range(int(h * y0), int(h * y1), 2):
        r = rows[y]
        for x in range(int(w * x0), int(w * x1), 2):
            i = x * ch
            out.append(r[i] if ch < 3 else (r[i] * 3 + r[i + 1] * 6 + r[i + 2]) // 10)
    return out


def check_image(path, want):
    if not os.path.isfile(path):
        fail(f"{path} was not written")
    img = read_png(path)
    w, h = img[0], img[1]
    if (w, h) != want:
        fail(f"{path} is {w}x{h} pixels, not {want[0]}x{want[1]}")
    g = gray(img)
    mean = sum(g) / len(g)
    var = sum((v - mean) ** 2 for v in g) / len(g)
    # The middle of the window shows the board. It must not be one color.
    _, _, ch, rows = img
    mid = set()
    for y in range(h // 4, 3 * h // 4, 3):
        for x in range(w // 4, 3 * w // 4, 3):
            mid.add(bytes(rows[y][x * ch:x * ch + ch]))
    print(f"  {os.path.basename(path)}: {w}x{h}, gray variance {var:.0f}, "
          f"{len(mid)} colors in the middle")
    if var < 100:
        fail(f"{path} is almost blank (gray variance {var:.0f})")
    if len(mid) < 16:
        fail(f"{path}: the middle has {len(mid)} colors only, the board is not drawn")
    # The middle, where the board view is.
    return gray(img, 0.25, 0.75, 0.25, 0.75)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--bin", default=os.path.join(ROOT, "target", "release"),
                    help="the folder with lamina and lamina-viewer")
    ap.add_argument("--max-seconds", type=float, default=90,
                    help="the time limit of each viewer run")
    ap.add_argument("--max-mb", type=float, default=600,
                    help="the memory limit of each viewer run (Linux)")
    ap.add_argument("--keep", help="copy the board and the images to this folder")
    a = ap.parse_args()
    lamina = os.path.join(a.bin, "lamina" + EXE)
    viewer = os.path.join(a.bin, "lamina-viewer" + EXE)
    for exe in (lamina, viewer):
        if not os.path.isfile(exe):
            fail(f"{exe} does not exist")

    work = tempfile.mkdtemp(prefix="lamina-smoke-")
    try:
        board = os.path.join(work, "board")
        print("Making the board ...", flush=True)
        make_board(lamina, board)
        want = (round(SIZE[0] * SCALE), round(SIZE[1] * SCALE))
        common = ["--size", f"{SIZE[0]}x{SIZE[1]}", "--scale", str(SCALE)]
        cx, cy = SIZE[0] // 2, SIZE[1] // 2
        img = {k: os.path.join(work, f"{k}.png")
               for k in ("2d", "2d-mid", "3d", "3d-mid")}

        print("2D ...", flush=True)
        # The waits give a reload loop time to show in the memory.
        run_shot(viewer, [board, *common, "--out", img["2d"],
                          "wait:1500", "key:F", f"hover:{cx},{cy}", f"scroll:{cx},{cy},2",
                          f"shot:{img['2d-mid']}", f"click:{cx},{cy}", f"rclick:{cx},{cy}",
                          "key:Escape", "key:Escape", f"drag:{cx},{cy},{cx + 30},{cy + 10}",
                          "key:ctrl+z", "key:Slash", "type:R1", "key:Escape", "key:F",
                          "wait:1500"],
                 a.max_seconds, a.max_mb)
        print("3D ...", flush=True)
        run_shot(viewer, [board, *common, "--3d", "--out", img["3d"],
                          "wait:1500", f"drag:{cx},{cy},{cx + 60},{cy - 20}",
                          f"shot:{img['3d-mid']}", f"scroll:{cx},{cy},1", f"hover:{cx},{cy}",
                          "key:shift+F", "wait:1500"],
                 a.max_seconds, a.max_mb)

        g = {k: check_image(p, want) for k, p in img.items()}
        diff = sum(abs(x - y) for x, y in zip(g["2d"], g["3d"])) / len(g["2d"])
        print(f"  mean gray difference of 2D and 3D in the middle: {diff:.1f}")
        if diff < 8:
            fail("the 2D and the 3D images are the same")
        print("OK")
    finally:
        # Keep the images of a failed run too, to look at them.
        if a.keep:
            shutil.copytree(work, a.keep, dirs_exist_ok=True)
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
