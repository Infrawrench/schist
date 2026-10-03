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

if len(sys.argv) > 5:
    with tempfile.TemporaryDirectory(prefix="schist-crossover-check-") as temporary:
        prefix = Path(temporary) / "crossovers"
        result = subprocess.run(["pdftoppm", "-png", "-r", "72", sys.argv[5], str(prefix)],
                                check=True, capture_output=True, text=True)
        assert not result.stderr.strip(), result.stderr
        files = sorted(Path(temporary).glob("crossovers-*.png"))
        assert len(files) == 3
        images = []
        for file in files:
            with Image.open(file) as image:
                images.append(image.convert("RGB"))
        left, right, imposed = images
        assert left.size == right.size == (132, 102)
        assert imposed.size == (264, 102)
        assert imposed.crop((0, 0, 132, 102)).tobytes() == left.tobytes()
        assert imposed.crop((132, 0, 264, 102)).tobytes() == right.tobytes()
        # A continuous image has blue pixels on the left and green on the right.
        # Both pages' inside bleeds must contain the adjacent part of that image.
        for image, point in [(left, (110, 50)), (right, (3, 50))]:
            r, g, b = image.getpixel(point)
            assert b > 100 and max(r, g) < 90, (point, (r, g, b))
        for image, point in [(right, (20, 50)), (left, (128, 50))]:
            r, g, b = image.getpixel(point)
            assert g > 100 and g > max(r, b), (point, (r, g, b))
        # The 50% black shape is above the red shape on both sides of the spine.
        assert left.getpixel((120, 26)) == right.getpixel((12, 26))
        assert left.getpixel((106, 26))[0] > left.getpixel((120, 26))[0] + 30
        assert right.getpixel((26, 26))[0] > right.getpixel((12, 26))[0] + 30
        for image, xs in [(left, range(96, 126)), (right, range(6, 60))]:
            assert sum(max(image.getpixel((x,y))) < 150 for x in xs for y in range(68, 93)) > 20, "crossing text lost one side"
    print("Design PDF: crossover shapes, continuous image/text, inside bleed and identical n-up placement verified.")

if len(sys.argv) > 6:
    with tempfile.TemporaryDirectory(prefix="schist-tint-check-") as temporary:
        prefix = Path(temporary) / "tints"
        result = subprocess.run(["pdftoppm", "-png", "-r", "72", "-singlefile", sys.argv[6], str(prefix)],
                                check=True, capture_output=True, text=True)
        assert not result.stderr.strip(), result.stderr
        with Image.open(prefix.with_suffix(".png")) as opened:
            image = opened.convert("RGB")
            assert image.size == (200, 170), image.size
            grays = [image.getpixel((25+i*36,22)) for i in range(5)]
            greens = [image.getpixel((25+i*36,57)) for i in range(5)]
            assert min(grays[0]) > 245 and max(grays[-1]) < 60, grays
            assert all(a[0] > b[0]+15 for a,b in zip(grays,grays[1:])), grays
            assert all(a[0] > b[0]+15 for a,b in zip(greens,greens[1:])), greens
            assert all(g > max(r,b)+15 for r,g,b in greens[1:]), greens
            zero, tint, opacity, overprint = [image.getpixel((25+i*45,92)) for i in range(4)]
            assert min(zero)>245, ("zero tint did not knock out",zero)
            assert max(tint)-min(tint)<10 and 140<min(tint)<235, tint
            assert opacity[0]>max(opacity[1:])+70, opacity
            assert overprint[0]>max(overprint[1:])+70, overprint
            # Compare solid glyph interiors with the matching 25%/100% patches.
            light = dark = 0
            for y in range(115,155):
                for x in range(10,190):
                    pixel = image.getpixel((x,y))
                    light += max(abs(a-b) for a,b in zip(pixel,grays[1]))<5
                    dark += max(abs(a-b) for a,b in zip(pixel,grays[-1]))<5
            assert light>100 and dark>100, (light,dark)
    print("Design PDF: process/spot tint ramps, zero-tint knockout, opacity, overprint and styled text verified.")

if len(sys.argv) > 7:
    with tempfile.TemporaryDirectory(prefix="schist-decoration-check-") as temporary:
        prefix = Path(temporary) / "decorations"
        result = subprocess.run(["pdftoppm", "-png", "-r", "144", sys.argv[7], str(prefix)],
                                check=True, capture_output=True, text=True)
        assert not result.stderr.strip(), result.stderr
        files = sorted(Path(temporary).glob("decorations-*.png"))
        assert len(files) == 4
        images = []
        for file in files:
            with Image.open(file) as opened:
                images.append(opened.convert("RGB"))
        for before, after in [(images[0],images[1]),(images[2],images[3])]:
            assert before.size == after.size == (400,400)
            plain = [before.getpixel((x,y)) for y in range(400) for x in range(400)]
            decorated = [after.getpixel((x,y)) for y in range(400) for x in range(400)]
            # Solid interior glyph colours must stay unchanged: the decoration
            # crossing a translucent glyph must not lay a second coat of ink.
            from collections import Counter
            interiors = [colour for colour,count in Counter(plain).most_common(3) if min(colour)<240]
            assert len(interiors) == 2, interiors
            for colour in interiors:
                assert sum(pixel==colour for pixel in plain)>300
                for original, actual in zip(plain,decorated):
                    if original==colour:
                        assert max(abs(a-b) for a,b in zip(original,actual))<5, (original,actual)
            added = [(i%400,i//400) for i,(a,b) in enumerate(zip(plain,decorated)) if min(a)>250 and min(b)<230]
            assert len(added)>400, "missing decoration ink through spaces"
            # A decoration axis contains over 150 new pixels through previously
            # blank space. Glyph-only rasterization cannot satisfy this.
            from collections import defaultdict
            axes = defaultdict(set)
            vertical = before is images[2]
            for x,y in added:
                axes[x if vertical else y].add(y if vertical else x)
            assert max(len(points) for points in axes.values())>150
    print("Design PDF: horizontal/vertical underline and strikethrough, spaces and single-pass translucent glyph coverage verified.")


if len(sys.argv) > 8:
    with tempfile.TemporaryDirectory(prefix="schist-baseline-check-") as temporary:
        prefix = Path(temporary) / "baseline"
        result = subprocess.run(["pdftoppm", "-png", "-r", "144", sys.argv[8], str(prefix)],
                                check=True, capture_output=True, text=True)
        assert not result.stderr.strip(), result.stderr
        rendered = sorted(Path(temporary).glob("baseline-*.png"))
        assert len(rendered) == 8
        # pdftoppm's page rasterization changes antialiased edge colours by up
        # to six levels after a shift, despite identical translated samples.
        # Compare exact image samples through Poppler's independent extractor,
        # and separately verify their rendered placement on the PDF pages.
        result = subprocess.run(["pdfimages", "-png", sys.argv[8], str(Path(temporary)/"samples")],
                                check=True, capture_output=True, text=True)
        assert not result.stderr.strip(), result.stderr
        files = sorted(Path(temporary).glob("samples-*.png"))
        assert len(files) == 8
        def ink_bounds(path):
            with Image.open(path) as image:
                image = image.convert("RGB")
                assert image.size == (400,400)
                points = [(x,y) for y in range(400) for x in range(400) if min(image.getpixel((x,y)))<240]
            assert len(points)>1000
            return min(x for x,y in points), min(y for x,y in points), max(x for x,y in points), max(y for x,y in points)
        for pair, (dx, dy) in enumerate([(0,-16), (16,0), (-16,0), (16,0)]):
            with Image.open(files[pair*2]) as opened:
                before = opened.convert("RGB")
            with Image.open(files[pair*2+1]) as opened:
                after = opened.convert("RGB")
            assert before.size == after.size == (400,400)
            ink = 0
            for y in range(400):
                for x in range(400):
                    sx, sy = x-dx, y-dy
                    expected = before.getpixel((sx,sy)) if 0<=sx<400 and 0<=sy<400 else (255,255,255)
                    actual = after.getpixel((x,y))
                    assert expected == actual, (pair,x,y,expected,actual)
                    ink += min(actual)<240
            assert ink>1000
            before_box, after_box = ink_bounds(rendered[pair*2]), ink_bounds(rendered[pair*2+1])
            assert all(abs(b-a-d)<=1 for a,b,d in zip(before_box,after_box,(dx,dy,dx,dy))), (pair,before_box,after_box)
    print("Design PDF: point baseline shifts translate horizontal, mixed upright/rotated vertical and rotated-frame ink without changing glyph colours or decorations.")


if len(sys.argv) > 9:
    with tempfile.TemporaryDirectory(prefix="schist-script-check-") as temporary:
        prefix = Path(temporary) / "scripts"
        result = subprocess.run(["pdfimages", "-png", sys.argv[9], str(prefix)],
                                check=True, capture_output=True, text=True)
        assert not result.stderr.strip(), result.stderr
        files = sorted(Path(temporary).glob("scripts-*.png"))
        assert len(files) == 9
        images = []
        for file in files:
            with Image.open(file) as opened:
                images.append(opened.convert("RGB"))
        def colour_box(image, cyan):
            points = [(x,y) for y in range(400) for x in range(400)
                      if image.getpixel((x,y))[0 if cyan else 1] < 200
                      and image.getpixel((x,y))[1 if cyan else 0] > image.getpixel((x,y))[0 if cyan else 1] + 30
                      and image.getpixel((x,y))[2] > image.getpixel((x,y))[0 if cyan else 1] + 30]
            assert len(points)>100
            return min(x for x,y in points),min(y for x,y in points),max(x for x,y in points),max(y for x,y in points)
        for mode in range(3):
            plain, superior, inferior = images[mode*3:mode*3+3]
            assert plain.size == superior.size == inferior.size == (400,400)
            dx,dy = (0,80) if mode==0 else (-80,0)
            # Equal-size super/subscript glyphs differ only in their displacement:
            # 50% of 40pt leading in opposite directions, at 144 dpi.
            for y in range(400):
                for x in range(400):
                    sx,sy=x-dx,y-dy
                    expected=superior.getpixel((sx,sy)) if 0<=sx<400 and 0<=sy<400 else (255,255,255)
                    assert inferior.getpixel((x,y))==expected, (mode,x,y)
            for cyan in [True,False]:
                a,b=colour_box(plain,cyan),colour_box(superior,cyan)
                for start,end in [(0,2),(1,3)]:
                    assert abs((b[end]-b[start]+1)-(a[end]-a[start]+1)*0.5)<=3, (mode,a,b)
            # The two differently coloured paragraphs retain a 40pt advance.
            for image in [plain,superior,inferior]:
                first,second=colour_box(image,True),colour_box(image,False)
                axis=1 if mode==0 else 0
                assert abs(abs(second[axis]-first[axis])-80)<=1, (mode,first,second)
    print("Design PDF: script glyph scaling, regular leading, explicit offsets and opposite super/subscript displacement verified in horizontal and both vertical modes.")

if len(sys.argv) > 10:
    with tempfile.TemporaryDirectory(prefix="schist-opentype-check-") as temporary:
        # Check both decoded image samples and independently rendered pages.
        for command, prefix in [("pdfimages", "samples"), ("pdftoppm", "pages")]:
            args = [command, "-png"]
            if command == "pdftoppm":
                args += ["-r", "144"]
            result = subprocess.run(args + [sys.argv[10], str(Path(temporary)/prefix)],
                                    check=True, capture_output=True, text=True)
            assert not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix+"-*.png"))
            assert len(files) == 9
            for mode in range(3):
                images=[]
                for file in files[mode*3:mode*3+3]:
                    with Image.open(file) as opened:
                        images.append(opened.convert("RGB"))
                off,on,mixed=images
                assert off.size == on.size == mixed.size == (400,400)
                assert off.tobytes() != on.tobytes(), (prefix,mode,"feature switches did not affect glyphs")
                cross=1 if mode==0 else 0
                occupied=[]
                for coordinate in range(400):
                    if any(min(im.getpixel((along,coordinate) if cross else (coordinate,along))) < 240
                           for im in (off,on) for along in range(400)):
                        occupied.append(coordinate)
                gaps=[(a,b) for a,b in zip(occupied,occupied[1:]) if b-a>8]
                assert len(gaps)==1, (prefix,mode,gaps)
                split=sum(gaps[0])//2
                first=(0,0,400,split) if cross else ((split,0,400,400) if mode==1 else (0,0,split,400))
                expected=off.copy()
                expected.paste(on.crop(first),first[:2])
                assert expected.tobytes()==mixed.tobytes(), (prefix,mode,"feature settings leaked across paragraph/range boundaries")
    print("Design PDF: ranged OpenType ligatures and kerning match whole-style controls in horizontal and both vertical modes, in extracted samples and rendered pages.")

if len(sys.argv) > 11:
    with tempfile.TemporaryDirectory(prefix="schist-leading-check-") as temporary:
        directory = Path(temporary)
        result = subprocess.run(["pdftoppm", "-png", "-r", "144", sys.argv[11], str(directory / "page")], check=True, capture_output=True, text=True)
        assert not result.stderr.strip(), result.stderr
        result = subprocess.run(["pdfimages", "-png", sys.argv[11], str(directory / "ink")], check=True, capture_output=True, text=True)
        assert not result.stderr.strip(), result.stderr
        for prefix in ("page", "ink"):
            files = sorted(directory.glob(prefix + "-*.png"))
            assert len(files) == 12, (prefix, len(files))
            for index in range(0, 12, 2):
                with Image.open(files[index]) as opened:
                    actual = opened.convert("RGB")
                with Image.open(files[index + 1]) as opened:
                    reference = opened.convert("RGB")
                assert actual.size == reference.size
                assert actual.tobytes() == reference.tobytes(), (prefix, index, "leading differs from independently placed baselines")
                assert min(channel[0] for channel in actual.getextrema()) < 100, (prefix, index, "empty proof")
    print("Design PDF: fixed/automatic mixed-size leading and blank-line spacing match independently placed horizontal/vertical baselines.")

if len(sys.argv) > 12:
    with tempfile.TemporaryDirectory(prefix="schist-font-style-check-") as temporary:
        # Check both decoded image samples and independently rendered pages.
        for command, prefix in [("pdfimages", "samples"), ("pdftoppm", "pages")]:
            args = [command, "-png"]
            if command == "pdftoppm":
                args += ["-r", "144"]
            result = subprocess.run(args + [sys.argv[12], str(Path(temporary)/prefix)],
                                    check=True, capture_output=True, text=True)
            assert not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix+"-*.png"))
            assert len(files) == 9
            for mode in range(3):
                images=[]
                for file in files[mode*3:mode*3+3]:
                    with Image.open(file) as opened:
                        images.append(opened.convert("RGB"))
                regular,light,mixed=images
                assert regular.size == light.size == mixed.size == (400,400)
                assert sum(255-v for v in light.convert("L").tobytes()) < sum(255-v for v in regular.convert("L").tobytes()) * 0.8, (prefix,mode,"Light must use its actual lighter glyphs")
                cross=1 if mode==0 else 0
                occupied=[]
                for coordinate in range(400):
                    if any(min(im.getpixel((along,coordinate) if cross else (coordinate,along))) < 240
                           for im in (regular,light) for along in range(400)):
                        occupied.append(coordinate)
                gaps=[(a,b) for a,b in zip(occupied,occupied[1:]) if b-a>8]
                assert len(gaps)==1, (prefix,mode,gaps)
                split=sum(gaps[0])//2
                first=(0,0,400,split) if cross else ((split,0,400,400) if mode==1 else (0,0,split,400))
                expected=regular.copy()
                expected.paste(light.crop(first),first[:2])
                assert expected.tobytes()==mixed.tobytes(), (prefix,mode,"font variants leaked across paragraph/range boundaries")
    print("Design PDF: named font faces match whole-style controls in horizontal and both vertical modes, in extracted samples and rendered pages.")

if len(sys.argv) > 13:
    with tempfile.TemporaryDirectory(prefix="schist-object-style-check-") as temporary:
        for command, prefix in [("pdfimages", "samples"), ("pdftoppm", "pages")]:
            args = [command, "-png"]
            if command == "pdftoppm":
                args += ["-r", "144"]
            result = subprocess.run(args + [sys.argv[13], str(Path(temporary)/prefix)],
                                    check=True, capture_output=True, text=True)
            assert not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix+"-*.png"))
            assert len(files) == 8
            for index in range(0, 8, 2):
                with Image.open(files[index]) as opened:
                    actual = opened.convert("RGB")
                with Image.open(files[index+1]) as opened:
                    expected = opened.convert("RGB")
                assert actual.size == expected.size == (400,400)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"frame paint order differs from independent artwork")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty frame proof")
    print("Design PDF: inherited rectangular and curved text/image frame paints match independent fill/content/stroke objects under affine placement.")

if len(sys.argv) > 14:
    with tempfile.TemporaryDirectory(prefix="schist-text-stroke-check-") as temporary:
        for command, prefix in [("pdfimages", "samples"), ("pdftoppm", "pages")]:
            args = [command, "-png"]
            if command == "pdftoppm":
                args += ["-r", "144"]
            result = subprocess.run(args + [sys.argv[14], str(Path(temporary)/prefix)],
                                    check=True, capture_output=True, text=True)
            assert not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix+"-*.png"))
            assert len(files) == 12
            for index in range(0, 12, 2):
                with Image.open(files[index]) as opened:
                    actual = opened.convert("RGB")
                with Image.open(files[index+1]) as opened:
                    expected = opened.convert("RGB")
                assert actual.size == expected.size == (400,400)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"text paints differ from independent fill/stroke objects")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty text stroke proof")
    print("Design PDF: centered/outside glyph strokes with miter/round/bevel joins match independent fill/stroke text objects, including spot tints, opacity, overprint, affine placement and both vertical directions.")

if len(sys.argv) > 15:
    with tempfile.TemporaryDirectory(prefix="schist-custom-decoration-check-") as temporary:
        for command, prefix in [("pdfimages", "samples"), ("pdftoppm", "pages")]:
            args = [command, "-png"]
            if command == "pdftoppm":
                args += ["-r", "144"]
            result = subprocess.run(args + [sys.argv[15], str(Path(temporary)/prefix)],
                                    check=True, capture_output=True, text=True)
            assert not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix+"-*.png"))
            assert len(files) == 12
            for index in range(0, 12, 2):
                with Image.open(files[index]) as opened:
                    actual = opened.convert("RGB")
                with Image.open(files[index+1]) as opened:
                    expected = opened.convert("RGB")
                assert actual.size == expected.size == (400,400)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"decorated text differs from independent underline/fill/strike objects")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty custom decoration proof")
    print("Design PDF: custom underline/strike paints match independent text objects, including spot tints, opacity, overprint, affine placement and both vertical directions.")

if len(sys.argv) > 16:
    with tempfile.TemporaryDirectory(prefix="schist-cjk-feature-check-") as temporary:
        for command, prefix in [("pdfimages", "samples"), ("pdftoppm", "pages")]:
            args = [command, "-png"]
            if command == "pdftoppm":
                args += ["-r", "144"]
            result = subprocess.run(args + [sys.argv[16], str(Path(temporary)/prefix)],
                                    check=True, capture_output=True, text=True)
            assert not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix+"-*.png"))
            assert len(files) == 9
            for index in range(0, 9, 3):
                images = []
                for file in files[index:index+3]:
                    with Image.open(file) as opened:
                        images.append(opened.convert("RGB"))
                off, actual, expected = images
                assert actual.size == expected.size == off.size == (400,400)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"directional defaults differ from explicit font features")
                assert off.tobytes() != actual.tobytes(), (prefix,index,"proportional metrics did not change real glyph placement")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty CJK feature proof")
    print("Design PDF: mode-dependent CJK defaults match explicit OpenType tags, and real horizontal/vertical proportional metrics differ from full-em controls.")

if len(sys.argv) > 17:
    with tempfile.TemporaryDirectory(prefix="schist-caps-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[17], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 24, (prefix, len(files))
            for index in range(0, 24, 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400,400), (prefix,index,actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"capitalization differs from independent control")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty capitalization proof")
    print("Design PDF: uppercase expansion, native small/all-small caps and synthetic small caps match independently authored glyph/feature controls in all three writing modes.")

if len(sys.argv) > 18:
    with tempfile.TemporaryDirectory(prefix="schist-striped-decoration-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[18], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 12, (prefix, len(files))
            for index in range(0, 12, 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400,400), (prefix,index,actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"stripes/gaps differ from independent solid bands")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty striped decoration proof")
    print("Design PDF: striped underline/strike and independent gap inks match solid-band text objects, including fractional weights, spot tints, opacity, overprint, affine placement and all writing modes.")

if len(sys.argv) > 19:
    with tempfile.TemporaryDirectory(prefix="schist-dashed-decoration-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[19], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 12, (prefix, len(files))
            for index in range(0, 12, 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400,400), (prefix,index,actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"dashes/gaps differ from independent solid rectangles")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty dashed decoration proof")
    print("Design PDF: unadjusted butt-ended underline/strike dashes and gap inks match independent solid rectangles in all writing modes, including fractional lengths, spot tints, opacity and affine placement.")

if len(sys.argv) > 20:
    with tempfile.TemporaryDirectory(prefix="schist-capped-decoration-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[20], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 24, (prefix, len(files))
            for index in range(0, 24, 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400,400), (prefix,index,actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"capped dashes differ from independent analytic capsules")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty capped decoration proof")
    print("Design PDF: round/projecting dash caps and gap inks match independent analytic capsules in all writing modes, with fractional weights, spot tints, opacity and affine placement.")

if len(sys.argv) > 21:
    with tempfile.TemporaryDirectory(prefix="schist-dotted-decoration-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[21], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 12, (prefix, len(files))
            for index in range(0, 12, 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400,400), (prefix,index,actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"dotted ink differs from independently positioned circles")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty dotted decoration proof")
    print("Design PDF: unadjusted dotted underline/strike and gap inks match independently positioned analytic circles in all writing modes, including fractional weights, spot tints, opacity and affine placement.")

if len(sys.argv) > 22:
    with tempfile.TemporaryDirectory(prefix="schist-fitted-decoration-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[22], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 24, (prefix, len(files))
            for index in range(0, 24, 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400,400), (prefix,index,actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"fitted dashes differ from independent analytic capsules")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty fitted decoration proof")
    print("Design PDF: fitted straight dashes/dots match independently enumerated placements and analytic coverage in all writing modes, preserving selected lengths, inks and affine placement.")

if len(sys.argv) > 23:
    with tempfile.TemporaryDirectory(prefix="schist-language-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[23], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 24, (prefix, len(files))
            for index in range(0, 24, 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400,400), (prefix,index,actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"language shaping differs from independent glyphs")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty language proof")
    print("Design PDF: Turkic/Lithuanian capitals and Romanian glyphs match independent Unicode text, including inherited languages and explicit resets in all writing modes.")

if len(sys.argv) > 24:
    with tempfile.TemporaryDirectory(prefix="schist-text-path-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[24], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 24, (prefix, len(files))
            for index in range(0,24,2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400,400), (prefix,index,actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"Design path differs from independent engine specification")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty path proof")
    print("Design PDF: bounded path alignment, glyph strokes, solid/striped/dashed/dotted decorations, gap inks and affine placement match independent engine baseline specifications; native application agreement remains unverified.")

if len(sys.argv) > 25:
    with tempfile.TemporaryDirectory(prefix="schist-lists-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[25], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 72, (prefix, len(files))
            for index in range(0,len(files),2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400,400), (prefix,index,actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"automatic markers differ from ordinary text frame controls")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty list proof")
    print("Design PDF: generated bullets, decimal/Roman/alphabetic/padded numbers and hidden counters match independently positioned text frames across marker sizes, alignments, strokes, spot tints, opacity and overprint.")

if len(sys.argv) > 26:
    with tempfile.TemporaryDirectory(prefix="schist-tabs-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[26], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 404, (prefix, len(files))
            for index in range(0,404,2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400,400), (prefix,index,actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix,index,"tabbed fields differ from independently positioned frames")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix,index,"empty tab proof")
    print("Design PDF: leading/right/center/character tab fields match independently positioned frames across writing modes, font sizes, initial indents, touching fields, spot/process paint, strokes and opacity; full native application agreement remains unverified.")

if len(sys.argv) > 27:
    with tempfile.TemporaryDirectory(prefix="schist-footnotes-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[27], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 24, (prefix, len(files))
            for index in range(0, len(files), 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400, 400), (prefix, index, actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix, index, "footnotes differ from independent text/shape frames")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix, index, "empty footnote proof")
    print("Design PDF: inline references, single-column, spanning and continued note bodies, and first/continued solid separator rules match independent text/shape frames with spot inks, tint, opacity and affine placement.")

if len(sys.argv) > 28:
    with tempfile.TemporaryDirectory(prefix="schist-paragraph-starts-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[28], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 48, (prefix, len(files))
            for index in range(0, len(files), 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400, 400), (prefix, index, actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix, index, "paragraph starts differ from independent frame destinations")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix, index, "empty paragraph start proof")
    print("Design PDF: paragraph starts and explicit breaks match independently positioned frames across column/frame/page destinations, numbered odd/even pages and LTR/RTL columns.")

if len(sys.argv) > 29:
    with tempfile.TemporaryDirectory(prefix="schist-no-break-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[29], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 24, (prefix, len(files))
            for index in range(0, len(files), 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (400, 400), (prefix, index, actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix, index, "No Break differs from independent frame destinations")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix, index, "empty No Break proof")
    print("Design PDF: No Break destinations match independent frames across paragraph/run settings, writing axes, reading directions and process/spot inks.")

if len(sys.argv) > 30:
    with tempfile.TemporaryDirectory(prefix="schist-soft-hyphen-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[30], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 24, (prefix, len(files))
            for index in range(0, len(files), 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (480, 480), (prefix, index, actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix, index, "discretionary glyphs differ from independent visible source")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix, index, "empty discretionary hyphen proof")
    print("Design PDF: selected and unused discretionary hyphens match independent visible source across threaded frames, writing axes, reading directions, tracking and process/spot inks.")

if len(sys.argv) > 31:
    with tempfile.TemporaryDirectory(prefix="schist-automatic-hyphenation-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[31], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 24, (prefix, len(files))
            for index in range(0, len(files), 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (480, 480), (prefix, index, actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix, index, "dictionary-selected glyphs differ from independent visible source")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix, index, "empty dictionary hyphen proof")
    print("Design PDF: dictionary-selected and unused automatic hyphens match independent visible source across threaded frames, writing axes, reading directions, tracking and process/spot inks.")

if len(sys.argv) > 32:
    with tempfile.TemporaryDirectory(prefix="schist-vertical-initials-check-") as temporary:
        for prefix, args in [("samples", ["pdfimages", "-png"]), ("pages", ["pdftoppm", "-r", "144", "-png"])]:
            result = subprocess.run(args + [sys.argv[32], str(Path(temporary)/prefix)], capture_output=True, text=True)
            assert result.returncode == 0 and not result.stderr.strip(), result.stderr
            files = sorted(Path(temporary).glob(prefix + "-*.png"))
            assert len(files) == 16, (prefix, len(files))
            for index in range(0, len(files), 2):
                actual = Image.open(files[index]).convert("RGB")
                expected = Image.open(files[index+1]).convert("RGB")
                assert actual.size == expected.size == (720, 520), (prefix, index, actual.size)
                assert actual.tobytes() == expected.tobytes(), (prefix, index, "vertical initials differ from independent ordinary frames")
                assert min(v[0] for v in actual.getextrema()) < 150, (prefix, index, "empty vertical initial proof")
    print("Design PDF: vertical initials match independent ordinary frames across upright/sideways glyphs, both column directions, affine placement and process/spot inks.")
