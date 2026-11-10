#!/usr/bin/env python3
"""Build the ESP32-C3 sensor board of the README with the Lamina MCP tools.

This script does what an AI agent does: it starts `lamina mcp`, then calls the
tools one by one. It needs Python 3.8 or later and a network (the parts come
from LCSC). Nothing else.

    python3 examples/c3-sensor/build.py                 # makes ./c3-sensor
    python3 examples/c3-sensor/build.py --out /tmp/c3   # another folder
    lamina-viewer c3-sensor                             # look at the result

Open the viewer before you run the script to watch the board grow.
`--stages DIR` also keeps a copy of the board after each step (for the
README images, see `scripts/readme-media.py`).
"""
import argparse
import json
import os
import shutil
import subprocess
import sys


class Lamina:
    """A small MCP client: JSON-RPC over the stdin and stdout of `lamina mcp`."""

    def __init__(self, exe):
        self.p = subprocess.Popen([exe, "mcp"], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True, bufsize=1)
        self.next_id = 0
        self.request("initialize", {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "c3-sensor-example", "version": "1"},
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
                sys.exit("lamina mcp stopped")
            msg = json.loads(line)
            if msg.get("id") == self.next_id:
                if "error" in msg:
                    sys.exit(f"{method}: {msg['error']}")
                return msg["result"]

    def __call__(self, tool, **args):
        """Call a tool. Return its JSON result, or stop on an error."""
        r = self.request("tools/call", {"name": tool, "arguments": args})
        text = "\n".join(c.get("text", "") for c in r.get("content", []) if c.get("type") == "text")
        if r.get("isError"):
            sys.exit(f"{tool}: {text}")
        try:
            return json.loads(text)
        except ValueError:
            return text

    def close(self):
        """Stop `lamina mcp`."""
        self.p.stdin.close()
        self.p.wait()
        self.p.stdout.close()


# The parts. `provider:id` names a real part: footprint, 3D model, pin names.
PARTS = {
    "U1": "lcsc:C2838502",   # ESP32-C3-MINI-1-N4 module
    "U2": "lcsc:C6186",      # AMS1117-3.3 regulator
    "U3": "lcsc:C2848306",   # SHT40 temperature and humidity sensor
    "J1": "lcsc:C165948",    # USB-C receptacle
    "J2": "kicad:Connector_PinHeader_2.54mm:PinHeader_1x04_P2.54mm_Vertical",
    "D1": "lcsc:C2286",      # red LED 0603
    "D2": "lcsc:C114586",    # WS2812B RGB LED
    "U4": "lcsc:C7484",      # SN74AHCT1G125 buffer: 3.3 V data in, 5 V data out
    "SW1": "lcsc:C318884", "SW2": "lcsc:C318884",                 # reset, boot
    "R1": "lcsc:C23186", "R2": "lcsc:C23186",                     # 5.1k USB-C CC
    "R3": "lcsc:C25804",                                          # 10k EN pull-up
    "R4": "lcsc:C21190",                                          # 1k LED
    "R5": "lcsc:C23162", "R6": "lcsc:C23162",                     # 4.7k I2C
    "C1": "lcsc:C15850", "C3": "lcsc:C15850",                     # 10uF
    "C2": "lcsc:C14663", "C4": "lcsc:C14663", "C5": "lcsc:C14663", "C6": "lcsc:C14663",
    "C7": "lcsc:C14663",                                          # 100nF
}

U1_GND = ["1", "2", "11", "14"] + [str(p) for p in range(36, 54)]
NETS = {
    "VBUS": ["J1.A4B9", "J1.B4A9", "U2.3", "C1.1", "D2.1", "C6.1", "U4.5", "C7.1"],
    "GND": ["J1.A1B12", "J1.B1A12", "J1.1", "J1.2", "J1.3", "J1.4", "U2.1", "C1.2",
            "C2.2", "C3.2", "C4.2", "C5.2", "C6.2", "R1.2", "R2.2", "SW1.3", "SW1.4",
            "SW2.3", "SW2.4", "D1.2", "U3.4", "U3.5", "D2.3", "J2.1",
            "U4.1", "U4.3", "C7.2"]
           + [f"U1.{p}" for p in U1_GND],
    "3V3": ["U2.2", "U2.4", "U1.3", "C2.1", "C3.1", "R3.1", "U3.3", "R5.1", "R6.1",
            "C5.1", "J2.2"],
    # Pads 1 and 2 (A, B) of a button are one contact, 3 and 4 (C, D) the other.
    "EN": ["U1.8", "R3.2", "C4.1", "SW1.1", "SW1.2"],
    "BOOT": ["U1.23", "SW2.1", "SW2.2"],
    "CC1": ["J1.A5", "R1.1"],
    "CC2": ["J1.B5", "R2.1"],
    "USB_DP": ["J1.A6", "J1.B6", "U1.27"],
    "USB_DN": ["J1.A7", "J1.B7", "U1.26"],
    "SDA": ["U1.18", "U3.1", "R5.2"],
    "SCL": ["U1.19", "U3.2", "R6.2"],
    # The WS2812B runs from 5 V, so U4 (VCC 5 V, TTL input) lifts the 3.3 V data.
    # U4.1 is OE, active low: it is on GND above.
    "LED_DATA": ["U1.22", "U4.2"],
    "LED_DIN": ["U4.4", "D2.4"],
    "LED_STAT": ["U1.16", "R4.1"],
    "LED_A": ["R4.2", "D1.1"],
    "TXD": ["U1.31", "J2.3"],
    "RXD": ["U1.30", "J2.4"],
}

# The board in mm: width, height, corner radius.
WIDTH, HEIGHT, CORNER = 56, 36, 2.5

# Placement in mm, origin at the lower-left corner, rotation in degrees.
# Rotation 270 turns the USB-C mouth to the left edge and the antenna to the right.
PLACE = [
    ("USB-C", {"J1": (5.0, 18, 270), "R1": (10.6, 13.2, 90), "R2": (13.4, 13.2, 90)}),
    # C1 is the input capacitor: it is at the VIN pin, on the right side of U2.
    ("regulator", {"U2": (17.5, 26.5, 0), "C1": (24.5, 26.5, 270), "C3": (11.2, 26.5, 90)}),
    ("ESP32-C3", {"U1": (44.4, 18, 270), "C2": (47.5, 26.6, 0),
                  "R3": (39.0, 27.4, 0), "C4": (39.0, 31.2, 0)}),
    ("buttons and LED", {"SW1": (30.5, 30.0, 0), "SW2": (30.5, 6.0, 0),
                         "D1": (31.0, 20.5, 0), "R4": (31.0, 17.5, 0)}),
    ("sensor, RGB LED and header", {"D2": (21.5, 12.0, 0), "C6": (21.5, 7.4, 0),
                                    "U3": (13.5, 6.0, 0), "C5": (13.5, 3.2, 180),
                                    "R5": (9.6, 6.0, 90), "R6": (17.2, 6.0, 90),
                                    "U4": (27.4, 13.3, 0), "C7": (27.4, 9.7, 0),
                                    "J2": (40.6, 4.6, 90)}),
]

# No copper on either layer near the module antenna. U1 at rotation 270 puts the
# antenna area (y 5.6 to 11.0 of the footprint) at x 50.0 to 55.4. Espressif asks for
# 15 mm of clearance on each side of the antenna: here that is the whole board
# height. So the keep-out is the strip from x 50.0 to the right edge.
ANTENNA_KEEPOUT = (50.0, 0.0, WIDTH, HEIGHT)   # min x, min y, max x, max y


def keepout_outline():
    """The keep-out strip, with the rounded corners of the board."""
    x0, y0, x1, y1 = ANTENNA_KEEPOUT
    r = CORNER
    return [{"x": x0, "y": y0}, {"x": x1 - r, "y": y0, "arc": {"center": {"x": x1 - r, "y": y0 + r}, "ccw": True}},
            {"x": x1, "y": y0 + r}, {"x": x1, "y": y1 - r, "arc": {"center": {"x": x1 - r, "y": y1 - r}, "ccw": True}},
            {"x": x1 - r, "y": y1}, {"x": x0, "y": y1}]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", default="c3-sensor", help="the project folder (default ./c3-sensor)")
    ap.add_argument("--lamina", default=shutil.which("lamina") or "lamina",
                    help="the lamina program (default: the one on PATH)")
    ap.add_argument("--stages", help="keep a copy of the board after each step in this folder")
    a = ap.parse_args(argv)

    if os.path.exists(a.out):
        sys.exit(f"{a.out} exists. Remove it or give --out.")
    out = os.path.abspath(a.out)
    step = [0]

    def stage(name):
        if a.stages:
            dst = os.path.join(a.stages, f"{step[0]:02d}-{name}")
            shutil.rmtree(dst, ignore_errors=True)
            shutil.copytree(out, dst, ignore=shutil.ignore_patterns("output", "*.mesh"))
        step[0] += 1

    t = Lamina(a.lamina)
    say = lambda s: print(f"\n> {s}", flush=True)

    say("open a new project and load the JLCPCB 2-layer rules")
    t("open", path=out, create=True, name="lamina-c3-sensor")
    t("use_fab_preset", name="jlcpcb-2layer")

    say("import the parts (footprint, 3D model and pin names of each)")
    r = t("import_parts", parts=[{"part": p, "refdes": ref} for ref, p in PARTS.items()])
    print(f"  {len(PARTS)} parts")

    say("draw a 56 x 36 mm board with four mounting holes")
    w, h = WIDTH, HEIGHT
    t("set_outline", rect={"width": w, "height": h, "corner_radius": CORNER})
    for x, y in [(3.2, 3.2), (w - 3.2, 3.2), (3.2, h - 3.2), (w - 3.2, h - 3.2)]:
        t("add_hole", at={"x": x, "y": y}, diameter=3.2)
    stage("outline")

    say("set the netlist")
    t("set_nets", nets={n: {"pads": pads} for n, pads in NETS.items()})
    roles = t("check", rules=["pin_role"])
    print(f"  {len(NETS)} nets, {roles['total']} pin-role warnings")

    for name, group in PLACE:
        say(f"place the {name}")
        t("place_many", placements={ref: {"x": x, "y": y, "rotation": rot}
                                    for ref, (x, y, rot) in group.items()})
        stage("place")

    say("keep copper away from the antenna")
    t("add_keepout", layers=["top", "bottom"], outline={"vertices": keepout_outline()},
      forbid={"tracks": True, "vias": True, "pours": True})

    say("label the board")
    for text, x, y, size in [("LAMINA  C3 SENSOR", 18.5, 33.2, 1.4), ("RST", 25.6, 30.0, 1.0),
                             ("BOOT", 36.2, 6.0, 1.0), ("G 3V3 TX RX", 44.4, 1.6, 0.9)]:
        t("add_text", layer="top.silk", text=text, at={"x": x, "y": y}, size=size)
    errors = t("check", warnings=False)["total"]
    print(f"  {errors} errors before routing")
    stage("placed")

    say("pour ground on both layers and autoroute")
    t("add_zone", net="GND", layers=["top", "bottom"], pad_connection="solid")
    print("  " + t("autoroute")["summary"])
    stage("routed")

    say("accept the warnings that are there by design")
    why = {"J1": "USB-C receptacle overhangs the board edge by design",
           "U1": "Module antenna sits at the board edge by design"}
    # Only the warnings about the board edge. A different warning for these
    # parts is a defect.
    edge_rules = {"courtyard_outside_board", "silk_outside_board", "silk_to_edge"}
    for v in t("check")["violations"]:
        ref = next((r for r in why if r in v["items"]), None)
        if ref and v["rule"] in edge_rules:
            t("exclude_violation", rule=v["rule"], items=v["items"], reason=why[ref])
    left = t("check")["total"]
    print(f"  {left} violations left")
    stage("done")

    s = t("board_stats")
    print(f"\nDone: {out}\n  {s['width']:g} x {s['height']:g} mm, {s['parts_top']} parts, "
          f"{s['vias']} vias, {s['track_length']:.0f} mm of track")
    print(f"\nLook at it:  lamina-viewer {a.out}")
    t.close()


if __name__ == "__main__":
    main()
