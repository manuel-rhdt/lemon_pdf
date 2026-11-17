"""Part A: measures the patch pairs of a probe page from a screenshot of a device.

    python3 measure.py OUT/layout.json SHOT.jpg PAGE_INDEX [--profile P3.icc]

The screenshot is registered by the frame of the page (a black rectangle at 5 pt and 355 pt from the
left, 5 pt and 635 pt from the top): the scale and the position come from the frame lines. Every
patch is the median of the central 50 % of its rectangle. The pixels are read with the profile of the
screenshot (Display P3 for an iPhone), converted to CIELAB (D50), and compared with the DeviceRGB patch
of the same pair: CIEDE2000 and signed dL*, dC*, dh. The calibration strip (DeviceRGB greys) says what
the capture does to a colour that is inside sRGB.
"""
import io
import json
import sys
import numpy as np
from PIL import Image, ImageCms
import colour as C

PAGE_PT = (360, 640)


def runs(mask):
    d = np.diff(np.concatenate([[0], mask.astype(int), [0]]))
    s, e = np.where(d == 1)[0], np.where(d == -1)[0]
    return list(zip(s, e - s))


class Shot:
    def __init__(self, path):
        im = Image.open(path)
        self.icc = im.info.get("icc_profile")
        self.rgb = np.asarray(im.convert("RGB")).astype(np.float64) / 255.0
        # the profile of the screenshot: Display P3 has the primaries and the curve of CSS display-p3
        self.space = "unknown"
        if self.icc:
            desc = ImageCms.getProfileDescription(ImageCms.ImageCmsProfile(io.BytesIO(self.icc))).strip()
            self.space = desc
        self.matrix = C.M_P3 if "P3" in self.space else C.M_SRGB

    def register(self):
        """Scale and offsets from the frame lines: x of the left line, and the pages found."""
        dark = self.rgb.max(axis=2) < 70 / 255
        H, W = dark.shape
        xs = [x for x in range(W) if any(l > 600 for s, l in runs(dark[:, x]))]
        groups, cur = [], [xs[0]]
        for x in xs[1:]:
            if x - cur[-1] <= 2:
                cur.append(x)
            else:
                groups.append(cur)
                cur = [x]
        groups.append(cur)
        # the two lines of the frame are the inner pair of bars
        centres = [(g[0] + g[-1]) / 2 for g in groups]
        left, right = centres[1], centres[-2]
        scale = (right - left) / 350.0
        frac = dark[:, int(left) + 12:int(right) - 12].mean(axis=1)
        lines = [(s + l / 2.0, l) for s, l in runs(frac > 0.97) if 6 <= l <= 14]
        pages = []
        for i, (top, _) in enumerate(lines):
            for bottom, _ in lines[i + 1:]:
                if abs((bottom - top) - 630 * scale) < 4:
                    pages.append((top, bottom))
        return scale, left, pages

    def sample(self, rect, scale, x0, y0, shrink=0.25):
        """Median colour of the central part of `rect` = [x, y, w, h] in points of the page."""
        x, y, w, h = rect
        px = lambda v: x0 + (v - 5) * scale
        py = lambda v: y0 + (v - 5) * scale
        xa, xb = int(round(px(x + w * shrink))), int(round(px(x + w * (1 - shrink))))
        ya, yb = int(round(py(y + h * shrink))), int(round(py(y + h * (1 - shrink))))
        region = self.rgb[ya:max(yb, ya + 1), xa:max(xb, xa + 1)].reshape(-1, 3)
        return np.median(region, axis=0)

    def lab(self, rgb):
        return C.xyz65_to_lab50(self.matrix @ C.srgb_dec(rgb))


def signed(lab_a, lab_b):
    """dL*, dC* and dh (degrees, signed) of `a` against `b`."""
    La, Ca, ha = C.lab_to_lch(lab_a)
    Lb, Cb, hb = C.lab_to_lch(lab_b)
    dh = (ha - hb + 180) % 360 - 180
    return La - Lb, Ca - Cb, dh


def measure_page(layout, shot, page, which=0):
    scale, left, pages = shot.register()
    top = pages[which][0]
    rows = {}
    for p in layout["patches"]:
        if p["page"] != page:
            continue
        enc, ref = shot.sample(p["enc"], scale, left, top), shot.sample(p["ref"], scale, left, top)
        l_enc, l_ref = shot.lab(enc), shot.lab(ref)
        l_target = C.xyz65_to_lab50(np.array(p["target_xyz65"]))
        row = rows.setdefault(p["row"], dict(ramp=[], hue=[]))
        row[p["kind"]].append(dict(p=p, enc=enc, ref=ref, lab_enc=l_enc, lab_ref=l_ref, lab_target=l_target,
                                   de=C.delta_e2000(l_enc, l_ref), de_ref=C.delta_e2000(l_ref, l_target),
                                   d=signed(l_enc, l_ref)))
    calibration = []
    for c in layout["calibration"]:
        if c["page"] == page and c["kind"] == "grey":
            m = shot.sample(c["rect"], scale, left, top)
            calibration.append((np.array(c["rgb"]), m))
    return rows, calibration, scale


def summarise(rows):
    out = []
    for row, kinds in rows.items():
        cells = []
        for kind in ("ramp", "hue"):
            items = kinds[kind]
            if not items:
                continue
            de = np.array([i["de"] for i in items])
            d = np.array([i["d"] for i in items])
            cells.append((kind, de.mean(), de.max(), d[:, 0].mean(), d[:, 1].mean(), d[:, 2].mean(),
                          np.mean([i["de_ref"] for i in items])))
        out.append((row, cells))
    return out


if __name__ == "__main__":
    layout = json.load(open(sys.argv[1]))
    shot = Shot(sys.argv[2])
    page = int(sys.argv[3])
    rows, calibration, scale = measure_page(layout, shot, page)
    print(f"screenshot space: {shot.space}, scale {scale:.4f} px/pt")
    cal = np.array([np.abs(m - nominal).max() for nominal, m in calibration])
    print(f"calibration strip: max |measured - nominal| = {cal.max()*255:.1f} levels (mean {cal.mean()*255:.1f})")
    print("row   kind  mean dE2000 (max)   dL*    dC*    dh   | DeviceRGB patch vs its target")
    for row, cells in summarise(rows):
        for kind, mean, mx, dl, dc, dh, deref in cells:
            print(f"{row}  {kind:4}  {mean:6.2f} ({mx:5.2f})  {dl:+6.2f} {dc:+6.2f} {dh:+6.1f}  | {deref:4.2f}")
