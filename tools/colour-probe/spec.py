"""Builds the colour probe: a spec for examples/colour_probe.rs and a layout file with the ground
truth (where every patch is, what colour it stands for, how the encoding says it).

    python3 spec.py OUTDIR [--mutate NAME]

Every number that is written comes from float64 conversions of a target colour (XYZ, D65) in
colour.py; the Rust program converts nothing. A *mutation* is a deliberate mistake, for the negative
controls of verify.py: the file is wrong and the expectation (from the target) is not.
"""
import json
import os
import sys
import numpy as np
import colour as C
import iccgen

PAGE_W, PAGE_H = 360, 640
GREY_LEVELS = 17

# in-gamut hue targets, as linear sRGB (every channel from 0.02 to 0.98)
HUES = {
    "red": (0.80, 0.05, 0.04), "orange": (0.80, 0.30, 0.03), "yellow": (0.75, 0.70, 0.04),
    "green": (0.10, 0.70, 0.05), "teal": (0.05, 0.70, 0.50), "blue": (0.05, 0.30, 0.80),
    "violet": (0.40, 0.05, 0.70), "magenta": (0.75, 0.10, 0.50),
}
# the neutral ramp: 17 steps of encoded sRGB from 0.05 to 0.95 (neutrals cannot leave a gamut)
RAMP = [0.05 + 0.90 * i / (GREY_LEVELS - 1) for i in range(GREY_LEVELS)]


def fmt(values):
    return " ".join(f"{float(v):.9f}" for v in values)


def targets():
    """(kind, index, name, sRGB nominal (encoded), XYZ D65) of every patch of a row."""
    out = []
    for i, v in enumerate(RAMP):
        out.append(("ramp", i, f"grey{i}", np.array([v, v, v]), C.srgb_to_xyz65([v, v, v])))
    for k, (name, lin) in enumerate(HUES.items()):
        lin = np.array(lin)
        out.append(("hue", k, name, C.srgb_enc(lin), C.M_SRGB @ lin))
    return out


# ----------------------------------------------------------------------------- encoders
def lab_encoder(white, adapted):
    def enc(xyz65):
        xyz = C.adapt(xyz65, C.D65, white) if adapted else xyz65
        return C.xyz_to_lab(xyz, white)
    return enc


def rgb_encoder(matrix, enc):
    return lambda xyz65: C.to_rgb_space(xyz65, matrix, enc)


def carrier_encoder(white, scale=1.2):
    return lambda xyz65: (xyz65 if np.allclose(white, C.D65) else C.adapt(xyz65, C.D65, white)) / scale


def calrgb22_encoder():
    def enc(xyz65):
        lin = np.linalg.solve(C.M_SRGB, xyz65)
        return np.sign(lin) * np.abs(lin) ** (1 / 2.2)
    return enc


def colorants(xy):
    cols, _ = iccgen.colorants_d50(xy)
    return [float(v) for c in cols for v in c]


def intent_functions():
    """The device -> PCS functions of the profile that tells the intents apart: an sRGB LUT profile
    whose table for each intent is different (perceptual: darker and less colourful, saturation:
    more colourful), so that the device shows which table it used."""
    def exact(rgb):
        return C.adapt(C.M_SRGB @ C.srgb_dec(rgb), C.D65, C.D50)

    def scaled(lightness, chroma):
        def fn(rgb):
            L, a, b = C.xyz_to_lab(exact(rgb), C.D50)
            return C.lab_to_xyz([L * lightness, a * chroma, b * chroma], C.D50)
        return fn
    return scaled(0.85, 0.5), exact, scaled(1.0, 1.6)


INTENTS = ["Perceptual", "RelativeColorimetric", "Saturation", "AbsoluteColorimetric"]


def variants():
    """The rows of Part A."""
    v = []
    srgb_cols, p3_cols, r2020_cols = colorants(C.SRGB_XY), colorants(C.P3_XY), colorants(C.REC2020_XY)

    def matrix_line(pid, desc, cols, para):
        return f"profile {pid} matrix {desc.replace(' ', '_')} {fmt(cols)} {fmt(para)}"

    dev = lambda x: C.srgb_enc(np.linalg.solve(C.M_SRGB, x))
    v.append(dict(id="C00", label="control: DeviceRGB against DeviceRGB", space="device", enc=dev, tests="the noise of the capture"))
    v.append(dict(id="C01", label="control: DeviceGray ramp", space="gray", enc=lambda x: np.array([float(dev(x)[1])]), tests="DeviceGray", hue=False))
    v.append(dict(id="I01", label="ICC sRGB v2, table curve (lemon)", profiles=["profile p_srgb2 lemon srgb_v2"], space="icc p_srgb2", enc=rgb_encoder(C.M_SRGB, C.srgb_enc), tests="control"))
    v.append(dict(id="I02", label="ICC sRGB v4, parametric (lemon)", profiles=[matrix_line("p_srgb4", "sRGB v4", srgb_cols, iccgen.SRGB_PARA)], space="icc p_srgb4", enc=rgb_encoder(C.M_SRGB, C.srgb_enc), tests="control"))
    v.append(dict(id="I03", label="ICC Display P3 v4, parametric (lemon)", profiles=["profile p_p3v4 lemon p3_v4"], space="icc p_p3v4", enc=rgb_encoder(C.M_P3, C.srgb_enc), tests="control that worked"))
    v.append(dict(id="I04", label="ICC Display P3 v2, table curve (python)", profiles=["profile p_p3v2 file {OUT}/p3_v2.icc"], space="icc p_p3v2", enc=rgb_encoder(C.M_P3, C.srgb_enc), tests="v2 table against v4 parametric"))
    v.append(dict(id="I05", label="ICC Rec.2020 v4, inverse OETF (lemon)", profiles=["profile p_r20oetf lemon rec2020_v4"], space="icc p_r20oetf", enc=rgb_encoder(C.M_REC2020, C.rec2020_enc), tests="TRC: the OETF", trc="oetf"))
    v.append(dict(id="I06", label="ICC Rec.2020 v4, gamma 2.4", profiles=[matrix_line("p_r20g24", "Rec2020 g2.4", r2020_cols, iccgen.GAMMA24_PARA)], space="icc p_r20g24", enc=rgb_encoder(C.M_REC2020, C.gamma_enc(2.4)), tests="TRC: pure gamma 2.4", trc="g24"))
    v.append(dict(id="I07", label="ICC Rec.2020 v4, sRGB curve", profiles=[matrix_line("p_r20srgb", "Rec2020 srgb", r2020_cols, iccgen.SRGB_PARA)], space="icc p_r20srgb", enc=rgb_encoder(C.M_REC2020, C.srgb_enc), tests="TRC: the sRGB curve", trc="srgb"))
    v.append(dict(id="I08", label="ICC Rec.2020 LUT-based (python)", profiles=["profile p_r20lut file {OUT}/r2020_lut.icc"], space="icc p_r20lut", enc=rgb_encoder(C.M_REC2020, C.rec2020_enc), tests="LUT against matrix/TRC"))
    v.append(dict(id="K01", label="CalRGB: XYZ carrier, D65 white exact", space=f"calrgb {fmt(C.D65)} 1 1 1 1.2 0 0 0 1.2 0 0 0 1.2", enc=carrier_encoder(C.D65), tests="carrier, exact white"))
    v.append(dict(id="K02", label="CalRGB: XYZ carrier, white rounded", space="calrgb 0.95 1 1.09 1 1 1 1.2 0 0 0 1.2 0 0 0 1.2", enc=carrier_encoder(C.D65), tests="the rounded white point"))
    v.append(dict(id="K03", label="CalRGB: XYZ carrier, D50 white", space=f"calrgb {fmt(C.D50)} 1 1 1 1.2 0 0 0 1.2 0 0 0 1.2", enc=carrier_encoder(C.D50), tests="D50-adapted carrier"))
    mflat = C.M_SRGB.T.flatten()   # XA YA ZA XB YB ZB XC YC ZC: the columns of the RGB -> XYZ matrix
    v.append(dict(id="K04", label="CalRGB: sRGB primaries, gamma 2.2", space=f"calrgb {fmt(C.D65)} 2.2 2.2 2.2 {fmt(mflat)}", enc=calrgb22_encoder(), tests="real primaries, gamma 2.2"))
    v.append(dict(id="L01", label="Lab: white D50, adapted, range +-256", space=f"lab {fmt(C.D50)} -256 256 -256 256", enc=lab_encoder(C.D50, True), tests="Lab as helium wrote it"))
    v.append(dict(id="L02", label="Lab: white D50, adapted, range -128..127", space=f"lab {fmt(C.D50)} -128 127 -128 127", enc=lab_encoder(C.D50, True), tests="the range"))
    v.append(dict(id="L03", label="Lab: white D50, NOT adapted", space=f"lab {fmt(C.D50)} -256 256 -256 256", enc=lab_encoder(C.D50, False), tests="colours without adaptation"))
    v.append(dict(id="L04", label="Lab: white D65", space=f"lab {fmt(C.D65)} -256 256 -256 256", enc=lab_encoder(C.D65, True), tests="D65 white point"))
    srgb_dev = rgb_encoder(C.M_SRGB, C.srgb_enc)   # the coordinates of the intent profile are the sRGB ones
    v.append(dict(id="R00", label="intent LUT: no ri (the default)", profiles=["profile p_lut3 file {OUT}/intent_lut.icc"], space="icc p_lut3", enc=srgb_dev, tests="the default intent", intent="default"))
    for mode, key, base in (("ri", "ri", "R1"), ("RI", "gs", "R2")):
        for n, name in enumerate(INTENTS):
            v.append(dict(id=f"{base}{n}", label=f"intent LUT: {mode} {name}" + (" (RI injected)" if mode == "RI" else ""),
                          profiles=["profile p_lut3 file {OUT}/intent_lut.icc"], space="icc p_lut3", enc=srgb_dev, tests=f"{mode} {name}", intent=name, intent_key=key))
    return v


PAGES_A = [["C00", "C01", "I01", "I02", "I03", "I04", "I05", "I06", "I07", "I08", "K01", "K02"],
           ["K03", "K04", "L01", "L02", "L03", "L04", "R00", "R10", "R11", "R12", "R13"],
           ["R20", "R21", "R22", "R23"]]

# ----------------------------------------------------------------------------- part B ladders
HUES_B = {"red": ((1, 0, 0), (1, 0, 0)), "green": ((0, 1, 0), (0, 1, 0)), "cyan": ((0, 1, 1), (0, 1, 1))}
STEPS_B = 12


def ladder_colors(hue):
    """12 steps from the sRGB edge (a primary or secondary of sRGB) to the Rec. 2020 one, mixed in
    linear XYZ. The background of every step is the colour clipped to sRGB."""
    s_rgb, e_rgb = HUES_B[hue]
    start = C.M_SRGB @ np.array(s_rgb, dtype=float)
    end = C.M_REC2020 @ np.array(e_rgb, dtype=float)
    out = []
    for i in range(STEPS_B):
        t = i / (STEPS_B - 1)
        xyz = start * (1 - t) + end * t
        lin = np.linalg.solve(C.M_SRGB, xyz)
        out.append((i + 1, t, xyz, C.srgb_enc(np.clip(lin, 0, 1))))
    return out


def ladder_variants():
    return [
        dict(id="P3v4", label="ICC Display P3 v4 (the control)", profiles=["profile p_p3v4 lemon p3_v4"], space="icc p_p3v4", enc=rgb_encoder(C.M_P3, C.srgb_enc), clip=True),
        dict(id="P3v2", label="ICC Display P3 v2 table", profiles=["profile p_p3v2 file {OUT}/p3_v2.icc"], space="icc p_p3v2", enc=rgb_encoder(C.M_P3, C.srgb_enc), clip=True),
        dict(id="R20", label="ICC Rec.2020 v4 (OETF)", profiles=["profile p_r20oetf lemon rec2020_v4"], space="icc p_r20oetf", enc=rgb_encoder(C.M_REC2020, C.rec2020_enc), clip=False),
        dict(id="K01", label="CalRGB XYZ carrier, D65", space=f"calrgb {fmt(C.D65)} 1 1 1 1.2 0 0 0 1.2 0 0 0 1.2", enc=carrier_encoder(C.D65), clip=False),
        dict(id="L01", label="Lab D50", space=f"lab {fmt(C.D50)} -256 256 -256 256", enc=lab_encoder(C.D50, True), clip=False),
    ]


# ----------------------------------------------------------------------------- the writer
class Probe:
    def __init__(self, out, mutate=None):
        self.out, self.mutate = out, mutate
        self.lines = []
        self.layout = {"page": [PAGE_W, PAGE_H], "pages": [], "calibration": [], "patches": [], "ladders": [], "variants": {}}
        self.profile_lines, self.space_lines, self.space_ids = {}, {}, {}

    def declare(self, v):
        for line in v.get("profiles", []):
            self.profile_lines.setdefault(line.split()[1], line.replace("{OUT}", self.out))
        if v["space"] not in self.space_ids:
            self.space_ids[v["space"]] = f"s{len(self.space_ids)}"
            self.space_lines[self.space_ids[v["space"]]] = f"space s{len(self.space_ids) - 1} {v['space']}"
        return self.space_ids[v["space"]]

    def add_page(self, title):
        self.lines.append(f"page {PAGE_W} {PAGE_H}")
        self.layout["pages"].append(title)
        page = len(self.layout["pages"]) - 1
        self.lines.append(f"stroke 5 5 {PAGE_W - 10} {PAGE_H - 10} 0 3")
        self.lines.append(f"text 14 22 11 1 0.1 {title}")
        for i in range(GREY_LEVELS):
            v = i / (GREY_LEVELS - 1)
            x, y, w, h = 24 + i * 18, 30, 17, 14
            self.lines.append(f"rect {x} {y} {w} {h} device {fmt([v, v, v])}")
            self.layout["calibration"].append(dict(page=page, kind="grey", rgb=[v, v, v], rect=[x, y, w, h]))
        corners = [(1, 0, 0), (0, 1, 0), (0, 0, 1), (0, 1, 1), (1, 0, 1), (1, 1, 0), (1, 1, 1), (0, 0, 0)]
        for i, rgb in enumerate(corners):
            x, y, w, h = 24 + i * 38, 48, 34, 12
            self.lines.append(f"rect {x} {y} {w} {h} device {fmt(rgb)}")
            self.layout["calibration"].append(dict(page=page, kind="primary", rgb=list(rgb), rect=[x, y, w, h]))
        return page

    def coordinates(self, v, xyz65):
        enc, mutation = v["enc"], self.mutate
        if mutation == "wrong_trc" and v["id"] == "I05":
            enc = rgb_encoder(C.M_REC2020, C.srgb_enc)          # the profile says OETF
        if mutation == "wrong_white" and v["id"] == "L01":
            enc = lab_encoder(C.D65, True)                       # the dictionary says D50
        if mutation == "wrong_primaries" and v["id"] == "I03":
            enc = rgb_encoder(C.M_SRGB, C.srgb_enc)              # the profile is Display P3
        if mutation == "unadapted_carrier" and v["id"] == "K03":
            enc = carrier_encoder(C.D65)                         # the white point is D50
        if mutation == "rounded_white" and v["id"] == "K01":
            pass                                                 # K02 is the rounded one; kept for the report
        return np.asarray(enc(xyz65), dtype=float)

    def row(self, page, v, y0):
        sid = self.declare(v)
        self.layout["variants"][v["id"]] = dict(label=v["label"], tests=v.get("tests"), trc=v.get("trc"), intent=v.get("intent"),
                                               intent_key=v.get("intent_key"), space=v["space"])
        self.lines.append(f"text 14 {y0 + 8} 7.5 0 0.15 {v['id']}  {v['label']}")
        for kind, index, name, srgb, xyz65 in targets():
            if kind == "hue" and v.get("hue") is False:
                continue
            coords = self.coordinates(v, xyz65)
            if kind == "ramp":
                x, y, w, h, gap = 24 + index * 18, y0 + 12, 8, 12, 9
            else:
                x, y, w, h, gap = 24 + index * 40, y0 + 27, 16, 12, 17
            extra = ""
            if v.get("intent_key") == "ri":
                extra = f" ri={v['intent']}"
            if v.get("intent_key") == "gs":
                extra = f" gs={v['intent']}"
            self.lines.append(f"rect {x} {y} {w} {h} {sid} {fmt(coords)}{extra}")
            self.lines.append(f"rect {x + gap} {y} {w} {h} device {fmt(srgb)}")
            self.layout["patches"].append(dict(page=page, row=v["id"], kind=kind, index=index, name=name,
                enc=[x, y, w, h], ref=[x + gap, y, w, h], target_srgb=[float(t) for t in srgb],
                target_xyz65=[float(t) for t in xyz65], coords=[float(c) for c in coords]))

    def part_a(self):
        by_id = {v["id"]: v for v in variants()}
        for n, ids in enumerate(PAGES_A):
            page = self.add_page(f"Part A, page {n + 1}: do the pairs match?")
            for r, vid in enumerate(ids):
                self.row(page, by_id[vid], 72 + r * 44)

    def part_b(self):
        for hue in HUES_B:
            page = self.add_page(f"Part B, {hue}: first and last readable step")
            for r, v in enumerate(ladder_variants()):
                sid = self.declare(v)
                y0 = 72 + r * 108
                self.lines.append(f"text 14 {y0 + 8} 8 1 0.1 {v['id']}  {v['label']}")
                steps = []
                for index, t, xyz, bg in ladder_colors(hue):
                    coords = self.coordinates(v, xyz)
                    if v["clip"]:
                        coords = np.clip(coords, 0, 1)
                    x, y, w, h = 20 + (index - 1) * 27, y0 + 14, 26, 22
                    self.lines.append(f"rect {x} {y} {w} {h} device {fmt(bg)}")
                    self.lines.append(f"rect {x + 5} {y + 4} {w - 10} {h - 8} {sid} {fmt(coords)}")
                    self.lines.append(f"text {x + w / 2 - 3} {y + h + 11} 9 0 0.1 {index}")
                    steps.append(dict(index=index, t=t, rect=[x, y, w, h], inner=[x + 5, y + 4, w - 10, h - 8], bg=[float(b) for b in bg],
                                      target_xyz65=[float(a) for a in xyz], coords=[float(c) for c in coords]))
                self.layout["ladders"].append(dict(page=page, hue=hue, row=v["id"], label=v["label"], steps=steps))

    def write(self, directory):
        head = [f"# colour probe, mutation: {self.mutate}"]
        spec = "\n".join(head + list(self.profile_lines.values()) + list(self.space_lines.values()) + self.lines) + "\n"
        with open(f"{directory}/probe.spec", "w") as f:
            f.write(spec)
        with open(f"{directory}/layout.json", "w") as f:
            json.dump(self.layout, f, indent=1)


def write_profiles(directory):
    open(f"{directory}/p3_v2.icc", "wb").write(
        iccgen.matrix_profile("Display P3 v2 table", C.P3_XY, ("table", lambda x: float(C.srgb_dec(x))), version=2))

    def r2020_exact(rgb):
        return C.adapt(C.M_REC2020 @ C.rec2020_dec(rgb), C.D65, C.D50)
    open(f"{directory}/r2020_lut.icc", "wb").write(iccgen.lut_profile("Rec2020 LUT", r2020_exact, r2020_exact, r2020_exact))
    perceptual, relative, saturation = intent_functions()
    open(f"{directory}/intent_lut.icc", "wb").write(iccgen.lut_profile("Intent LUT", perceptual, relative, saturation))


def build(directory, parts=("A", "B"), mutate=None):
    os.makedirs(directory, exist_ok=True)
    directory = os.path.abspath(directory)
    write_profiles(directory)
    probe = Probe(directory, mutate)
    if "A" in parts:
        probe.part_a()
    if "B" in parts:
        probe.part_b()
    probe.write(directory)
    return probe


if __name__ == "__main__":
    out = sys.argv[1]
    mutate = sys.argv[sys.argv.index("--mutate") + 1] if "--mutate" in sys.argv else None
    probe = build(out, mutate=mutate)
    print(f"{len(probe.layout['patches'])} patch pairs, {len(probe.layout['ladders'])} ladders, pages: {probe.layout['pages']}")
