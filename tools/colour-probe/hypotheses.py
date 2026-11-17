"""Which interpretation does the device apply? For the rows of Part A that do not match, predicts what
each candidate interpretation would show and says which one is closest to the measurement.

    python3 hypotheses.py OUT/layout.json SHOT.jpg PAGE_INDEX ROW [ROW ...]
"""
import json
import sys
import numpy as np
import colour as C
import iccgen
from measure import Shot, measure_page


def lab_of_xyz(xyz):
    return C.xyz65_to_lab50(np.asarray(xyz, dtype=float))


def best(items, measured_labs, predict):
    """CIEDE2000 (mean) between the measured colours and what `predict(item)` gives."""
    return float(np.mean([C.delta_e2000(lab_of_xyz(predict(it)), m) for it, m in zip(items, measured_labs)]))


def trc_hypotheses(patches, labs):
    """Rec. 2020 matrix profile whose coordinates were made with the inverse OETF: which curve decodes them?"""
    cands = {
        "the curve of the profile (OETF)": C.rec2020_dec,
        "pure gamma 2.4": C.gamma_dec(2.4),
        "pure gamma 2.2": C.gamma_dec(2.2),
        "the sRGB curve": C.srgb_dec,
    }
    out = {name: best(patches, labs, lambda p, f=f: C.M_REC2020 @ f(np.array(p["coords"]))) for name, f in cands.items()}
    # the best pure exponent, by a search
    gs = np.linspace(1.6, 3.2, 161)
    fit = [best(patches, labs, lambda p, g=g: C.M_REC2020 @ C.gamma_dec(g)(np.array(p["coords"]))) for g in gs]
    out[f"pure gamma, best fit g = {gs[int(np.argmin(fit))]:.2f}"] = float(min(fit))
    return out


def carrier_hypotheses(patches, labs, white, scale=1.2):
    """A CalRGB carrier (gamma 1, matrix scale*I): the spec says XYZ = scale * components. A device that
    renormalises the matrix so that the components (1, 1, 1) are the white point shows white * components."""
    def shown(p, factor):
        xyz = factor * np.array(p["coords"])
        # a white point that is not D65 is adapted to the white of the screen
        return xyz if np.allclose(white, C.D65, atol=1e-3) else C.adapt(xyz, white, C.D65)
    return {
        "the PDF specification (XYZ = 1.2 * components)": best(patches, labs, lambda p: shown(p, scale)),
        "components (1,1,1) renormalised to the white point": best(patches, labs, lambda p: shown(p, white)),
    }


def lab_hypotheses(patches, labs, range_half, white=C.D50):
    """Lab: the colour is shown with a*, b* scaled by k (a device that scales the range to +-128)."""
    out = {}
    for k in (1.0, 128.0 / range_half, 0.5):
        def predict(p, k=k):
            L, a, b = p["coords"]
            return C.adapt(C.lab_to_xyz([L, a * k, b * k], white), white, C.D65) if not np.allclose(white, C.D65) else C.lab_to_xyz([L, a * k, b * k], white)
        out[f"a*, b* scaled by {k:.3f}"] = best(patches, labs, predict)
    # the best k by a search
    ks = np.linspace(0.2, 1.2, 101)
    fit = [best(patches, labs, lambda p, k=k: (C.adapt(C.lab_to_xyz([p["coords"][0], p["coords"][1] * k, p["coords"][2] * k], white), white, C.D65)
                                                if not np.allclose(white, C.D65) else C.lab_to_xyz([p["coords"][0], p["coords"][1] * k, p["coords"][2] * k], white))) for k in ks]
    out[f"best fit k = {ks[int(np.argmin(fit))]:.2f}"] = float(min(fit))
    return out


def intent_hypotheses(patches, labs):
    perceptual, relative, saturation = iccgen_intents()
    cands = {"A2B0 (perceptual)": perceptual, "A2B1 (relative colorimetric)": relative, "A2B2 (saturation)": saturation}
    out = {}
    for name, fn in cands.items():
        out[name] = best(patches, labs, lambda p, fn=fn: C.adapt(fn(C.srgb_enc(np.linalg.solve(C.M_SRGB, np.array(p["target_xyz65"])))), C.D50, C.D65))
    return out


def iccgen_intents():
    import spec
    return spec.intent_functions()


if __name__ == "__main__":
    layout = json.load(open(sys.argv[1]))
    shot = Shot(sys.argv[2])
    page = int(sys.argv[3])
    rows, _, _ = measure_page(layout, shot, page, which=0)
    for row in sys.argv[4:]:
        items = rows[row]["ramp"] + rows[row]["hue"]
        patches = [i["p"] for i in items]
        labs = [i["lab_enc"] for i in items]
        variant = layout["variants"][row]
        print(f"\n{row}: {variant['label']}")
        if variant.get("trc"):
            results = trc_hypotheses(patches, labs)
        elif variant["space"].startswith("calrgb"):
            white = np.array([float(v) for v in variant["space"].split()[1:4]])
            results = carrier_hypotheses(patches, labs, white if not np.allclose(white, C.D50) else white)
        elif variant["space"].startswith("lab"):
            half = float(variant["space"].split()[5])
            results = lab_hypotheses(patches, labs, half, C.D65 if np.allclose([float(v) for v in variant["space"].split()[1:4]], C.D65, atol=1e-3) else C.D50)
        elif variant.get("intent"):
            results = intent_hypotheses(patches, labs)
        else:
            continue
        for name, de in sorted(results.items(), key=lambda kv: kv[1]):
            print(f"   {de:6.2f}  {name}")
