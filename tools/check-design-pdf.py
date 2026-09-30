#!/usr/bin/env python3
"""Check the deterministic Design proof with an independent PDF renderer."""
import subprocess
import sys
import tempfile
from pathlib import Path
from PIL import Image

pdf = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix="schist-pdf-check-") as temporary:
    prefix = Path(temporary) / "proof"
    result = subprocess.run(
        ["pdftoppm", "-png", "-r", "72", "-singlefile", str(pdf), str(prefix)],
        check=True, capture_output=True, text=True,
    )
    if result.stderr.strip():
        raise AssertionError(f"PDF renderer reported: {result.stderr}")
    with Image.open(prefix.with_suffix(".png")) as image:
        image = image.convert("RGB")
        assert image.size == (210, 150), image.size
        red, blue, black, spot = [image.getpixel((40 + i * 40, 45)) for i in range(4)]
        assert red[0] > 180 and max(red[1:]) < 80, red
        assert blue[2] > 100 and max(blue[:2]) < 90, blue
        assert max(black) < 100, black
        assert spot[1] > spot[0] and spot[1] > spot[2], spot
        assert min(image.getpixel((105, 75))) > 240, "paper is not white"
        # Four known frame-local centers after x shear, plus the PDF trim offset.
        red, blue, green, black = [image.getpixel(point) for point in
            [(135,91),(148,91),(143,107),(156,107)]]
        assert red[0] > 180 and max(red[1:]) < 80, red
        assert blue[2] > 100 and max(blue[:2]) < 90, blue
        assert green[1] > 100 and green[1] > max(green[0],green[2]), green
        assert max(black) < 100, black
        # The tilted text must contribute visible glyphs below the first row.
        assert sum(max(image.getpixel((x,y))) < 180 for y in range(65,110) for x in range(20,90)) > 20
        assert min(image.getpixel((127,110))) > 240, "shear left pixels in the old frame"
        # Image rotates inside a fixed square frame: source corners clip away,
        # and the diamond cannot spill beyond the frame's left boundary.
        red, blue, green, black = [image.getpixel(point) for point in
            [(94,96),(104,106),(84,106),(94,116)]]
        assert red[0] > 180 and max(red[1:]) < 80, red
        assert blue[2] > 100 and max(blue[:2]) < 90, blue
        assert green[1] > 100 and green[1] > max(green[0],green[2]), green
        assert max(black) < 100, black
        for point in [(81,93), (77,106)]:
            assert min(image.getpixel(point)) > 240, ("inner image clipping", point)
        # A curved frame clips all four corners and its compound hole. Each
        # surviving quadrant must retain its original process ink.
        red, blue, green, black = [image.getpixel(point) for point in
            [(176,91),(186,91),(176,111),(186,111)]]
        assert red[0] > 180 and max(red[1:]) < 80, red
        assert blue[2] > 100 and max(blue[:2]) < 90, blue
        assert green[1] > 100 and green[1] > max(green[0],green[2]), green
        assert max(black) < 100, black
        for point in [(171,81), (190,81), (171,120), (190,120), (181,101)]:
            assert min(image.getpixel(point)) > 240, ("curved frame clipping", point)
print("Design PDF: inks, frame affines, inner image rotation, curved/compound clipping and paper rendered correctly; no PDF syntax diagnostics.")

if len(sys.argv) > 2:
    directory = Path(sys.argv[2])
    for up in (2, 4):
        with tempfile.TemporaryDirectory(prefix="schist-nup-check-") as temporary:
            prefix = Path(temporary) / "sheet"
            result = subprocess.run(
                ["pdftoppm", "-png", "-r", "72", str(directory / f"schist-nup-{up}.pdf"), str(prefix)],
                check=True, capture_output=True, text=True,
            )
            assert not result.stderr.strip(), result.stderr
            images = sorted(Path(temporary).glob("sheet-*.png"))
            assert len(images) == (5 + up - 1) // up
            for sheet, file in enumerate(images):
                with Image.open(file) as opened:
                    image = opened.convert("RGB")
                    assert image.size == (120, 40 if up == 2 else 80), image.size
                    for slot in range(up):
                        pixel = image.getpixel((30 + slot % 2 * 60, 20 + slot // 2 * 40))
                        page = sheet * up + slot
                        if page >= 5:
                            assert min(pixel) > 240, (up, page, pixel)
                        elif page in (0, 4):
                            assert pixel[0] > 180 and max(pixel[1:]) < 80, (up, page, pixel)
                        elif page == 1:
                            assert pixel[2] > 100 and max(pixel[:2]) < 90, (up, page, pixel)
                        elif page == 2:
                            assert max(pixel) < 100, (up, page, pixel)
                        else:
                            assert pixel[1] > pixel[0] and pixel[1] > pixel[2], (up, page, pixel)
    print("Design PDF: two-up and four-up reading order, sheet counts, and empty slots verified.")

if len(sys.argv) > 3:
    with tempfile.TemporaryDirectory(prefix="schist-text-check-") as temporary:
        prefix = Path(temporary) / "text"
        result = subprocess.run(
            ["pdftoppm", "-png", "-r", "144", "-singlefile", sys.argv[3], str(prefix)],
            check=True, capture_output=True, text=True,
        )
        assert not result.stderr.strip(), result.stderr
        with Image.open(prefix.with_suffix(".png")) as opened:
            image = opened.convert("RGB")
            # Inspect coloured glyphs, independent of the composer's line boxes.
            # A normal-size initial would cover far fewer than 55 pixel rows.
            for name, count, colour in [
                ("red", 2, lambda r, g, b: r > 140 and max(g, b) < 90),
                ("blue", 1, lambda r, g, b: b > 100 and max(r, g) < 90),
            ]:
                rows = [y for y in range(image.height)
                        if any(colour(*image.getpixel((x, y))) for x in range(image.width))]
                groups = []
                for y in rows:
                    # Accents can be disconnected from their base glyph.
                    if not groups or y > groups[-1][-1] + 14:
                        groups.append([])
                    groups[-1].append(y)
                assert len(groups) == count, (name, [len(g) for g in groups])
                assert all(g[-1] - g[0] >= 55 for g in groups), (name, groups)
            # Japanese text flows down the 75pt frame height, then across its
            # width. Inspect actual ink, beyond the horizontal paragraph above.
            ink = [(x, y) for y in range(280, 470) for x in range(390, 710)
                   if max(image.getpixel((x, y))) < 120]
            assert len(ink) > 2000, "vertical CJK glyphs did not render"
            left, right = min(x for x, _ in ink), max(x for x, _ in ink)
            top, bottom = min(y for _, y in ink), max(y for _, y in ink)
            assert 398 <= left < right <= 698 and 288 <= top < bottom <= 438, (left, top, right, bottom)
            assert right - left > 130 and bottom - top > 130, "vertical text used the wrong flow axes"
    print("Design PDF: enlarged initials and vertical Japanese/Latin text rendered within their frames.")

if len(sys.argv) > 4:
    # Independent parser validates boxes, renderer validates the plate origin.
    boxes = subprocess.run(["pdfinfo", "-f", "1", "-l", "1", "-box", sys.argv[4]],
                           check=True, capture_output=True, text=True)
    assert not boxes.stderr.strip(), boxes.stderr
    for label, expected in [("MediaBox", [0, 0, 86, 52]),
                            ("TrimBox", [14, 8, 74, 48]),
                            ("BleedBox", [4, 0, 80, 52])]:
        line = next(line for line in boxes.stdout.splitlines() if label + ":" in line)
        assert [float(v) for v in line.split(":", 1)[1].split()] == expected, line
    with tempfile.TemporaryDirectory(prefix="schist-offsets-check-") as temporary:
        prefix = Path(temporary) / "offsets"
        result = subprocess.run(["pdftoppm", "-png", "-r", "72", sys.argv[4], str(prefix)],
                                check=True, capture_output=True, text=True)
        assert not result.stderr.strip(), result.stderr
        images = sorted(Path(temporary).glob("offsets-*.png"))
        assert len(images) == 2
        for i, file in enumerate(images):
            with Image.open(file) as opened:
                image = opened.convert("RGB")
                assert image.size == ((86 if i == 0 else 172), 52), image.size
                # First page has 4/6/8/10pt bleed. The second has 8/10/4/6pt.
                for x, y in [(8, 20), (40, 2), (77, 20), (40, 48)] + (
                        [(95, 20), (125, 3), (162, 20), (125, 50)] if i else []):
                    r, g, b = image.getpixel((x, y))
                    assert r > 180 and max(g, b) < 80, (i, x, y, (r, g, b))
                for point in [(40, 20)] + ([(125, 20)] if i else []):
                    assert max(image.getpixel(point)) < 90, (i, point)
                for point in [(1, 20), (83, 20)] + ([(88, 20), (170, 20)] if i else []):
                    assert min(image.getpixel(point)) > 240, (i, point)
    print("Design PDF: asymmetric bleed, absolute slug, boxes, plate origins and n-up placement verified.")
