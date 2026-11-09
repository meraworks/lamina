#!/usr/bin/env python3
"""Tests for the ESP32-C3 sensor example.

Two kinds of tests, with the Python standard library only:

- `NetlistTest` and `PlacementTest` read the tables of `build.py` (PARTS, NETS,
  PLACE, the antenna keep-out). They need nothing else and take a moment.
- `BuiltBoardTest` builds the board with `build.py` into a temporary folder, then
  asks `lamina mcp` about it: DRC, routing, pins, copper near the antenna, and
  where the parts are. It needs the `lamina` program and a network (the parts come
  from LCSC). Without them, it is skipped and says why.

    python3 examples/c3-sensor/test_build.py              # all the tests
    python3 examples/c3-sensor/test_build.py -v NetlistTest
    LAMINA=target/release/lamina python3 examples/c3-sensor/test_build.py

The `lamina` program is `$LAMINA`, else the one on PATH.
"""
import contextlib
import io
import json
import math
import os
import re
import shutil
import sys
import tempfile
import unittest
import urllib.error
import urllib.request

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import build  # noqa: E402
from build import (ANTENNA_KEEPOUT, CORNER, HEIGHT, NETS, PARTS, PLACE,  # noqa: E402
                   WIDTH, keepout_outline)

EPS = 1e-3


# The nets of the tables, as {"U1.3": "3V3", ...}.
def pad_nets():
    out = {}
    for net, pads in NETS.items():
        for pad in pads:
            out.setdefault(pad, []).append(net)
    return out


def placements():
    out = {}
    for _, group in PLACE:
        for ref, xyr in group.items():
            out.setdefault(ref, []).append(xyr)
    return out


# Geometry. A rectangle is (min x, min y, max x, max y).

def transform(place, x, y):
    """A point of a footprint, on the board (rotation counter-clockwise)."""
    px, py, rot = place
    a = math.radians(rot)
    return (px + x * math.cos(a) - y * math.sin(a), py + x * math.sin(a) + y * math.cos(a))


def point_rect_distance(p, r):
    dx = max(r[0] - p[0], 0, p[0] - r[2])
    dy = max(r[1] - p[1], 0, p[1] - r[3])
    return math.hypot(dx, dy)


def segment_enters(a, b, r):
    """True if the segment a-b has a point strictly inside the rectangle r."""
    # Liang-Barsky clipping against the open rectangle.
    (x0, y0), (x1, y1) = a, b
    dx, dy = x1 - x0, y1 - y0
    t0, t1 = 0.0, 1.0
    for p, q in ((-dx, x0 - r[0]), (dx, r[2] - x0), (-dy, y0 - r[1]), (dy, r[3] - y0)):
        if p == 0:
            if q <= 0:
                return False
        else:
            t = q / p
            if p < 0:
                t0 = max(t0, t)
            else:
                t1 = min(t1, t)
    return t1 - t0 > 1e-9


def segment_rect_distance(a, b, r):
    if segment_enters(a, b, r) or point_rect_distance(a, r) == 0:
        return 0.0
    corners = [(r[0], r[1]), (r[2], r[1]), (r[2], r[3]), (r[0], r[3])]

    def point_segment(p):
        vx, vy = b[0] - a[0], b[1] - a[1]
        n = vx * vx + vy * vy
        t = 0 if n == 0 else max(0, min(1, ((p[0] - a[0]) * vx + (p[1] - a[1]) * vy) / n))
        return math.hypot(a[0] + t * vx - p[0], a[1] + t * vy - p[1])

    return min([point_rect_distance(a, r), point_rect_distance(b, r)] +
               [point_segment(c) for c in corners])


def shrink(r, d):
    return (r[0] + d, r[1] + d, r[2] - d, r[3] - d)


def inside_board(p):
    """True if the point is on the board: the rectangle with rounded corners."""
    x, y = p
    if not (-EPS <= x <= WIDTH + EPS and -EPS <= y <= HEIGHT + EPS):
        return False
    cx = min(max(x, CORNER), WIDTH - CORNER)
    cy = min(max(y, CORNER), HEIGHT - CORNER)
    return math.hypot(x - cx, y - cy) <= CORNER + EPS


class NetlistTest(unittest.TestCase):
    """The netlist of build.py, without lamina."""

    def test_nets_name_known_parts(self):
        for net, pads in NETS.items():
            for pad in pads:
                ref, _, num = pad.partition(".")
                self.assertIn(ref, PARTS, f"{pad} of {net}")
                self.assertTrue(num, f"{pad} of {net} has no pad number")

    def test_no_pad_in_two_nets(self):
        for pad, nets in pad_nets().items():
            self.assertEqual(len(nets), 1, f"{pad} is in {nets}")

    def test_each_net_joins_two_pads_or_more(self):
        for net, pads in NETS.items():
            self.assertGreaterEqual(len(set(pads)), 2, net)

    def test_each_part_is_on_a_net(self):
        on = {pad.split(".")[0] for pads in NETS.values() for pad in pads}
        self.assertEqual(set(PARTS) - on, set())

    def test_buttons_follow_the_datasheet(self):
        # C318884 (XKB TS-1187A) datasheet: A-B are joined, C-D are joined, and the
        # switch closes AB to CD. The footprint names pads 1-4 A, B, C, D.
        nets = {p: n[0] for p, n in pad_nets().items()}
        for sw, signal in (("SW1", "EN"), ("SW2", "BOOT")):
            self.assertEqual(nets[f"{sw}.1"], signal)
            self.assertEqual(nets[f"{sw}.2"], signal)
            self.assertEqual(nets[f"{sw}.3"], "GND")
            self.assertEqual(nets[f"{sw}.4"], "GND")

    def test_rgb_led_gets_5v_data(self):
        nets = {p: n[0] for p, n in pad_nets().items()}
        # D2 (WS2812B): 1 VDD, 3 VSS, 4 DIN.
        self.assertEqual(nets["D2.1"], "VBUS")
        self.assertEqual(nets["D2.3"], "GND")
        # U4 (SN74AHCT1G125): 1 OE#, 2 A, 3 GND, 4 Y, 5 VCC.
        self.assertEqual(nets["U4.5"], "VBUS", "the buffer runs from the LED supply")
        self.assertEqual(nets["U4.3"], "GND")
        self.assertEqual(nets["U4.1"], "GND", "OE# low keeps the buffer on")
        self.assertEqual(sorted(NETS[nets["U4.2"]]), ["U1.22", "U4.2"])
        self.assertEqual(sorted(NETS[nets["D2.4"]]), ["D2.4", "U4.4"],
                         "only the buffer drives DIN")
        self.assertEqual({nets["C7.1"], nets["C7.2"]}, {"VBUS", "GND"}, "C7 decouples U4")

    def test_module_ground_pads(self):
        for pad in build.U1_GND:
            self.assertIn(f"U1.{pad}", NETS["GND"])


class PlacementTest(unittest.TestCase):
    """The placement and the antenna keep-out of build.py, without lamina."""

    def test_each_part_placed_once(self):
        places = placements()
        self.assertEqual(set(places), set(PARTS))
        for ref, xyr in places.items():
            self.assertEqual(len(xyr), 1, f"{ref} is placed {len(xyr)} times")

    def test_part_origins_on_the_board(self):
        for ref, [(x, y, _)] in placements().items():
            self.assertTrue(inside_board((x, y)), f"{ref} at ({x}, {y})")

    def test_no_part_origin_in_the_keepout(self):
        for ref, [(x, y, _)] in placements().items():
            self.assertGreater(point_rect_distance((x, y), ANTENNA_KEEPOUT), 0, ref)

    def test_keepout_covers_the_antenna(self):
        # ESP32-C3-MINI-1 footprint: module body y -5.6 to 11.0, antenna area y 5.6 to
        # 11.0 across x -6.6 to 6.6, top pad row ends at y 5.3.
        [(ux, uy, rot)] = placements()["U1"]
        self.assertEqual(rot, 270, "the antenna points to +x only at rotation 270")
        x0, y0, x1, y1 = ANTENNA_KEEPOUT
        self.assertLessEqual(x0, ux + 5.6, "the keep-out starts at the antenna area")
        self.assertGreater(x0, ux + 5.3, "the keep-out stays off the module pads")
        self.assertGreaterEqual(x1, ux + 11.0)
        # 15 mm on each side of the antenna, as far as the board goes.
        self.assertLessEqual(y0, max(0, uy - 6.6 - 15))
        self.assertGreaterEqual(y1, min(HEIGHT, uy + 6.6 + 15))
        self.assertEqual(x1, WIDTH, "the keep-out reaches the board edge")

    def test_keepout_outline_on_the_board(self):
        vs = keepout_outline()
        self.assertGreaterEqual(len(vs), 4)
        for v in vs:
            self.assertTrue(inside_board((v["x"], v["y"])), v)
            if "arc" in v:
                c = v["arc"]["center"]
                self.assertAlmostEqual(math.hypot(v["x"] - c["x"], v["y"] - c["y"]), CORNER)


def find_lamina():
    exe = os.environ.get("LAMINA") or shutil.which("lamina")
    return exe if exe and os.access(exe, os.X_OK) else None


def network_problem():
    """None if LCSC's part server answers, else the reason."""
    try:
        urllib.request.urlopen(urllib.request.Request("https://easyeda.com/", method="HEAD"),
                               timeout=10)
    except urllib.error.HTTPError:
        pass  # It answered.
    except (urllib.error.URLError, OSError) as e:
        return str(e)
    return None


class BuiltBoardTest(unittest.TestCase):
    """Build the board with build.py, then check it through `lamina mcp`."""

    @classmethod
    def setUpClass(cls):
        exe = find_lamina()
        if not exe:
            raise unittest.SkipTest("no lamina program: set LAMINA or put lamina on PATH")
        cls.work = tempfile.mkdtemp(prefix="c3-sensor-test-")
        cls.board = os.path.join(cls.work, "board")
        log = io.StringIO()
        try:
            with contextlib.redirect_stdout(log):
                build.main(["--lamina", exe, "--out", cls.board])
        except SystemExit as e:
            shutil.rmtree(cls.work, ignore_errors=True)
            net = network_problem()
            if net:
                raise unittest.SkipTest(f"no network for the LCSC parts ({net})")
            raise AssertionError(f"build.py failed: {e}\n{log.getvalue()}")
        cls.t = build.Lamina(exe)
        cls.t("open", path=cls.board)
        cls.pins = {f"{p['refdes']}.{p['pad']}": p for p in cls.t("pins")}
        with open(os.path.join(cls.board, "routing.json")) as f:
            cls.routing = json.load(f)
        cls.footprints = {}

    @classmethod
    def tearDownClass(cls):
        cls.t.close()
        shutil.rmtree(cls.work, ignore_errors=True)

    def footprint(self, ref):
        with open(os.path.join(self.board, "parts.json")) as f:
            fid = json.load(f)["parts"][ref]["footprint"]
        if fid not in self.footprints:
            self.footprints[fid] = self.t("get_footprint", id=fid)
        return self.footprints[fid]

    # DRC and routing

    def test_drc_clean(self):
        self.assertEqual(self.t("check", warnings=False)["total"], 0, "errors")
        self.assertEqual(self.t("check")["violations"], [])
        s = self.t("status")
        self.assertEqual((s["errors"], s["warnings"]), (0, 0), s["by_rule"])

    def test_only_the_expected_exclusions(self):
        for e in self.t("exclusions")["exclusions"]:
            self.assertTrue(set(e["items"]) <= {"J1", "U1"}, e)
            self.assertTrue(e["reason"], e)

    def test_all_routed(self):
        self.assertEqual(self.t("ratsnest"), [])
        s = self.t("status")
        self.assertEqual(s["unplaced"], [])
        self.assertEqual(s["placed"], len(PARTS))

    def test_board_stats(self):
        s = self.t("board_stats")
        self.assertEqual((s["width"], s["height"]), (WIDTH, HEIGHT))
        self.assertEqual(s["parts_top"], len(PARTS))
        self.assertEqual(s["parts_unplaced"], 0)

    # Pins

    def test_netlist_pads_exist_with_their_net(self):
        for net, pads in NETS.items():
            for pad in pads:
                self.assertIn(pad, self.pins, f"{pad} of {net} is not on its footprint")
                self.assertEqual(self.pins[pad].get("net"), net, pad)

    def test_no_pad_on_a_net_it_should_not_have(self):
        want = {p: n[0] for p, n in pad_nets().items()}
        for pad, p in self.pins.items():
            if p.get("net"):
                self.assertEqual(want.get(pad), p["net"], pad)

    def test_button_pins_are_the_datasheet_letters(self):
        for sw, signal in (("SW1", "EN"), ("SW2", "BOOT")):
            got = {self.pins[f"{sw}.{n}"]["name"]: self.pins[f"{sw}.{n}"]["net"]
                   for n in "1234"}
            self.assertEqual(got, {"A": signal, "B": signal, "C": "GND", "D": "GND"}, sw)

    def test_rgb_led_and_buffer_pins(self):
        name_net = lambda pad: (self.pins[pad]["name"], self.pins[pad].get("net"))  # noqa: E731
        self.assertEqual(name_net("D2.1"), ("VDD", "VBUS"))
        self.assertEqual(name_net("D2.3"), ("VSS", "GND"))
        self.assertEqual(name_net("D2.4"), ("DIN", "LED_DIN"))
        self.assertEqual(name_net("U4.1"), ("OE#", "GND"))
        self.assertEqual(name_net("U4.2"), ("A", "LED_DATA"))
        self.assertEqual(name_net("U4.3"), ("GND", "GND"))
        self.assertEqual(name_net("U4.4"), ("Y", "LED_DIN"))
        self.assertEqual(name_net("U4.5"), ("VCC", "VBUS"))
        self.assertEqual(self.pins["U1.22"]["net"], "LED_DATA")

    # The antenna keep-out

    def test_keepout_stored(self):
        [k] = self.routing["keepouts"]
        self.assertEqual(set(k["layers"]), {"top", "bottom"})
        for what in ("tracks", "vias", "pours"):
            self.assertTrue(k["forbid"][what], what)

    def test_no_track_in_keepout(self):
        tracks = self.routing["tracks"]
        self.assertTrue(tracks)
        for tr in tracks:
            pts = [(p["x"], p["y"]) for p in tr["points"]]
            for a, b in zip(pts, pts[1:]):
                d = segment_rect_distance(a, b, ANTENNA_KEEPOUT)
                self.assertGreaterEqual(d, tr["width"] / 2 - EPS, f"track {tr['id']} {a}-{b}")

    def test_no_via_in_keepout(self):
        for v in self.routing["vias"]:
            d = point_rect_distance((v["at"]["x"], v["at"]["y"]), ANTENNA_KEEPOUT)
            self.assertGreaterEqual(d, v["diameter"] / 2 - EPS, f"via {v['id']}")

    def test_no_pour_in_keepout(self):
        # The project stores only the zone outline. The KiCad export has the poured
        # copper as filled polygons (KiCad's y axis points down).
        path = os.path.join(self.work, "fill.kicad_pcb")
        self.t("export_board", format="kicad", path=path)
        with open(path) as f:
            text = f.read()
        fills = re.findall(r'\(filled_polygon\s*\(layer "?([^")\s]+)"?\)\s*\(pts(.*?)\)\s*\)',
                           text, re.S)
        self.assertEqual({layer for layer, _ in fills}, {"F.Cu", "B.Cu"}, "a pour on each side")
        inner = shrink(ANTENNA_KEEPOUT, EPS)
        for layer, pts in fills:
            poly = [(float(x), -float(y)) for x, y in re.findall(r"\(xy ([-\d.]+) ([-\d.]+)\)", pts)]
            for a, b in zip(poly, poly[1:] + poly[:1]):
                self.assertFalse(segment_enters(a, b, inner), f"{layer} pour edge {a}-{b}")

    def test_no_pad_in_keepout(self):
        inner = shrink(ANTENNA_KEEPOUT, EPS)
        for ref, [place] in placements().items():
            for pad in self.footprint(ref)["pads"]:
                c, s = pad["at"], pad["shape"]
                if "diameter" in s:
                    centre = transform(place, c["x"], c["y"])
                    self.assertGreaterEqual(point_rect_distance(centre, ANTENNA_KEEPOUT),
                                            s["diameter"] / 2 - EPS, f"{ref}.{pad['number']}")
                    continue
                if s["type"] == "polygon":
                    local = [(p["x"], p["y"]) for p in s["points"]]
                else:  # rect, oval, roundrect: the bounding rectangle
                    w, h = s["width"] / 2, s["height"] / 2
                    local = [(-w, -h), (w, -h), (w, h), (-w, h)]
                a = math.radians(pad.get("rotation", 0) or 0)
                corners = []
                for dx, dy in local:
                    x = c["x"] + dx * math.cos(a) - dy * math.sin(a)
                    y = c["y"] + dx * math.sin(a) + dy * math.cos(a)
                    corners.append(transform(place, x, y))
                for p, q in zip(corners, corners[1:] + corners[:1]):
                    self.assertFalse(segment_enters(p, q, inner), f"{ref}.{pad['number']}")

    # Parts on the board

    def test_parts_inside_the_outline(self):
        # J1, the USB-C receptacle, overhangs the left edge by design (its exclusion
        # says so). Every other courtyard is on the board.
        for ref, [place] in placements().items():
            pts = []
            for g in self.footprint(ref)["graphics"]:
                if g["layer"] != "courtyard":
                    continue
                sh = g["shape"]
                if sh["type"] == "rect":
                    lo, hi = sh["min"], sh["max"]
                    pts += [(lo["x"], lo["y"]), (hi["x"], lo["y"]), (hi["x"], hi["y"]),
                            (lo["x"], hi["y"])]
                elif sh["type"] == "polygon":
                    pts += [(p["x"], p["y"]) for p in sh["points"]]
                elif sh["type"] == "line":
                    pts += [(sh["a"]["x"], sh["a"]["y"]), (sh["b"]["x"], sh["b"]["y"])]
            self.assertTrue(pts, f"{ref} has no courtyard")
            out = [transform(place, x, y) for x, y in pts
                   if not inside_board(transform(place, x, y))]
            if ref == "J1":
                self.assertTrue(out, "J1 should overhang the edge")
            else:
                self.assertEqual(out, [], f"{ref} courtyard leaves the board")


if __name__ == "__main__":
    unittest.main()
