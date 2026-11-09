#!/usr/bin/env python3
"""Tests of the social preview step of scripts/readme-media.py, and of the image
in docs/images. They need Pillow, not the Lamina programs: the step is tested
with a drawn board.

    python3 -m unittest discover -s scripts -p 'test_*.py' -v
"""
import importlib.util
import os
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("readme_media", os.path.join(HERE, "readme-media.py"))
media = importlib.util.module_from_spec(spec)
spec.loader.exec_module(media)

try:
    from PIL import Image, ImageDraw, ImageStat
except ImportError:
    Image = None

PREVIEW = os.path.join(media.IMAGES, "social-preview.png")


def gradient(x, y):
    """The plain background of the preview at a pixel."""
    t = y / (media.SOCIAL[1] - 1)
    return tuple(round(a + (b - a) * t) for a, b in zip(media.SOCIAL_TOP, media.SOCIAL_BOTTOM))


def share_off_background(img, box, limit=12):
    """The share of the pixels in `box` that are not the plain background."""
    px = img.load()
    x0, y0, x1, y1 = box
    n = off = 0
    for y in range(y0, y1, 2):
        for x in range(x0, x1, 2):
            n += 1
            if max(abs(a - b) for a, b in zip(px[x, y], gradient(x, y))) > limit:
                off += 1
    return off / n


def spread(img):
    """The mean standard deviation of the colour channels."""
    return sum(ImageStat.Stat(img).stddev) / 3


@unittest.skipIf(Image is None, "Pillow is not installed")
class DrawnBoard(unittest.TestCase):
    """The step with a drawn render: a board on the vignetted background of the
    3D view, and a dark part on the board."""

    def setUp(self):
        self.dir = tempfile.mkdtemp()
        w, h = 800, 600
        bg = Image.new("RGB", (w, h))
        px = bg.load()
        for y in range(h):
            for x in range(w):
                d2 = ((x / w - 0.5) ** 2 + (y / h - 0.42) ** 2)
                v = (1 - 0.45 * d2) * (22 - 18 * y / h)
                px[x, y] = (round(v), round(v * 1.1), round(v * 1.35))
        board = bg.copy()
        d = ImageDraw.Draw(board)
        d.polygon([(200, 150), (650, 220), (600, 470), (150, 400)], fill=(30, 120, 60))
        # A black part that is the colour of the background: it must stay board.
        d.rectangle([330, 260, 420, 330], fill=bg.getpixel((375, 295)))
        self.bg = os.path.join(self.dir, "bg.png")
        self.board = os.path.join(self.dir, "board.png")
        bg.save(self.bg)
        board.save(self.board)

    def tearDown(self):
        for f in os.listdir(self.dir):
            os.remove(os.path.join(self.dir, f))
        os.rmdir(self.dir)

    def test_cutout_keeps_the_board_and_drops_the_background(self):
        cut = media.board_cutout(self.board, self.bg)
        # Cropped to the board (less the one pixel of edge).
        self.assertLessEqual(abs(cut.width - 500), 4)
        self.assertLessEqual(abs(cut.height - 320), 4)
        a = cut.getchannel("A")
        self.assertEqual(a.getpixel((0, 0)), 0)  # A corner of the box is background.
        self.assertEqual(a.getpixel((cut.width // 2, cut.height // 2)), 255)
        # The dark part inside the board stays.
        self.assertEqual(a.getpixel((375 - 150, 295 - 150)), 255)

    def test_cutout_needs_the_same_size(self):
        small = os.path.join(self.dir, "small.png")
        Image.new("RGB", (10, 10)).save(small)
        with self.assertRaises(ValueError):
            media.board_cutout(self.board, small)

    def test_cutout_of_an_empty_render_fails(self):
        with self.assertRaises(ValueError):
            media.board_cutout(self.bg, self.bg)

    def test_preview(self):
        out = os.path.join(self.dir, "preview.png")
        media.social_preview(self.board, self.bg, out)
        img = Image.open(out)
        self.assertEqual(img.size, (1280, 640))
        self.assertEqual(img.mode, "RGB")
        self.assertGreater(spread(img), 8)
        self.assertGreater(share_off_background(img, media.SOCIAL_BOARD), 0.25)
        # The text side has the white title and the orange logo.
        self.assertGreater(share_off_background(img, (290, 170, 620, 260)), 0.1)
        self.assertGreater(share_off_background(img, (80, 100, 250, 310)), 0.1)

    def test_logo(self):
        img = media.logo(4)
        self.assertEqual(img.size, (176, 224))
        a = img.getchannel("A")
        # The via at (22, 14) and the pad at (46, 50) are rings: open in the middle.
        self.assertEqual(a.getpixel(((22 - 12) * 4, (14 - 4) * 4)), 0)
        self.assertEqual(a.getpixel(((46 - 12) * 4, (50 - 4) * 4)), 0)
        self.assertEqual(a.getpixel(((22 - 12) * 4 + 18, (14 - 4) * 4)), 255)
        self.assertEqual(a.getpixel(((22 - 12) * 4, (30 - 4) * 4)), 255)  # The track.
        self.assertEqual(a.getpixel((2, 220)), 0)  # Empty at the bottom left.
        self.assertEqual(img.getpixel(((22 - 12) * 4, (30 - 4) * 4))[:3], media.LOGO)


@unittest.skipIf(Image is None, "Pillow is not installed")
class CommittedPreview(unittest.TestCase):
    def setUp(self):
        self.img = Image.open(PREVIEW).convert("RGB")

    def test_size(self):
        self.assertEqual(self.img.size, (1280, 640))

    def test_not_blank(self):
        self.assertGreater(spread(self.img), 8)

    def test_board_is_there(self):
        self.assertGreater(share_off_background(self.img, media.SOCIAL_BOARD), 0.25)
        # A green PCB: many pixels where green is clearly the strongest channel.
        x0, y0, x1, y1 = media.SOCIAL_BOARD
        px = self.img.load()
        green = sum(1 for y in range(y0, y1, 2) for x in range(x0, x1, 2)
                    if px[x, y][1] > px[x, y][0] + 25 and px[x, y][1] > px[x, y][2] + 10)
        self.assertGreater(green / ((x1 - x0) * (y1 - y0) / 4), 0.1)

    def test_text_side(self):
        px = self.img.load()
        white = sum(1 for y in range(170, 260) for x in range(290, 620) if min(px[x, y]) > 230)
        self.assertGreater(white, 2000)  # The title.
        orange = sum(1 for y in range(100, 310) for x in range(80, 250)
                     if px[x, y][0] > 180 and px[x, y][2] < 100)
        self.assertGreater(orange, 4000)  # The logo.


if __name__ == "__main__":
    unittest.main()
