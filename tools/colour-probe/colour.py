"""Colour mathematics in float64, written independently of lemon_pdf and helium.

Conventions: XYZ is relative to a white with Y = 1. "D65" and "D50" are the whites of CSS Color 4
(chromaticities 0.3127, 0.3290 and 0.3457, 0.3585). A *target colour* is always given as XYZ of the
D65 viewing white (that is what a CSS colour is); `adapt` turns it into the XYZ of another white
with the Bradford transform.
"""
import numpy as np

def white(x, y):
    return np.array([x / y, 1.0, (1 - x - y) / y])

D65 = white(0.3127, 0.3290)
D50 = white(0.3457, 0.3585)

BRADFORD = np.array([[0.8951, 0.2664, -0.1614],
                     [-0.7502, 1.7135, 0.0367],
                     [0.0389, -0.0685, 1.0296]])

def adaptation(src_white, dst_white):
    """Bradford chromatic adaptation matrix from `src_white` to `dst_white`."""
    s = BRADFORD @ src_white
    d = BRADFORD @ dst_white
    return np.linalg.inv(BRADFORD) @ np.diag(d / s) @ BRADFORD

def adapt(xyz, src_white, dst_white):
    return adaptation(src_white, dst_white) @ np.asarray(xyz, dtype=float)

def primaries_matrix(xy_r, xy_g, xy_b, wp):
    """RGB (linear) -> XYZ for the primaries with these chromaticities and the white `wp`."""
    def col(xy):
        x, y = xy
        return np.array([x / y, 1.0, (1 - x - y) / y])
    m = np.column_stack([col(xy_r), col(xy_g), col(xy_b)])
    scale = np.linalg.solve(m, wp)
    return m * scale

# chromaticities of the RGB spaces of CSS Color 4
SRGB_XY = ((0.640, 0.330), (0.300, 0.600), (0.150, 0.060))
P3_XY = ((0.680, 0.320), (0.265, 0.690), (0.150, 0.060))
REC2020_XY = ((0.708, 0.292), (0.170, 0.797), (0.131, 0.046))
M_SRGB = primaries_matrix(*SRGB_XY, D65)       # linear sRGB -> XYZ (D65)
M_P3 = primaries_matrix(*P3_XY, D65)
M_REC2020 = primaries_matrix(*REC2020_XY, D65)

# transfer functions: encoded -> linear, and back
def srgb_dec(v):
    v = np.asarray(v, dtype=float)
    return np.where(np.abs(v) <= 0.04045, v / 12.92, np.sign(v) * ((np.abs(v) + 0.055) / 1.055) ** 2.4)

def srgb_enc(l):
    l = np.asarray(l, dtype=float)
    return np.where(np.abs(l) <= 0.0031308, 12.92 * l, np.sign(l) * (1.055 * np.abs(l) ** (1 / 2.4) - 0.055))

REC2020_ALPHA, REC2020_BETA = 1.09929682680944, 0.018053968510807

def rec2020_enc(l):
    l = np.asarray(l, dtype=float)
    return np.where(l < REC2020_BETA, 4.5 * l, REC2020_ALPHA * np.abs(l) ** 0.45 - (REC2020_ALPHA - 1))

def rec2020_dec(v):
    v = np.asarray(v, dtype=float)
    return np.where(v < 4.5 * REC2020_BETA, v / 4.5, ((v + REC2020_ALPHA - 1) / REC2020_ALPHA) ** (1 / 0.45))

def gamma_enc(g):
    return lambda l: np.sign(l) * np.abs(l) ** (1 / g)

def gamma_dec(g):
    return lambda v: np.sign(v) * np.abs(v) ** g

# CIELAB
KAPPA, EPSILON = 24389 / 27, 216 / 24389

def xyz_to_lab(xyz, wp):
    t = np.asarray(xyz, dtype=float) / wp
    f = np.where(t > EPSILON, np.cbrt(t), (KAPPA * t + 16) / 116)
    return np.array([116 * f[1] - 16, 500 * (f[0] - f[1]), 200 * (f[1] - f[2])])

def lab_to_xyz(lab, wp):
    L, a, b = lab
    f1 = (L + 16) / 116
    f0, f2 = a / 500 + f1, f1 - b / 200
    cube = lambda f: f ** 3 if f ** 3 > EPSILON else (116 * f - 16) / KAPPA
    y = f1 ** 3 if L > KAPPA * EPSILON else L / KAPPA
    return np.array([cube(f0), y, cube(f2)]) * wp

def lab_to_lch(lab):
    L, a, b = lab
    return np.array([L, np.hypot(a, b), np.degrees(np.arctan2(b, a)) % 360])

def delta_e2000(lab1, lab2):
    """CIEDE2000 (Sharma, Wu and Dalal, 2005)."""
    L1, a1, b1 = lab1
    L2, a2, b2 = lab2
    C1, C2 = np.hypot(a1, b1), np.hypot(a2, b2)
    Cm = (C1 + C2) / 2
    G = 0.5 * (1 - np.sqrt(Cm ** 7 / (Cm ** 7 + 25 ** 7)))
    a1p, a2p = (1 + G) * a1, (1 + G) * a2
    C1p, C2p = np.hypot(a1p, b1), np.hypot(a2p, b2)
    h1p = np.degrees(np.arctan2(b1, a1p)) % 360
    h2p = np.degrees(np.arctan2(b2, a2p)) % 360
    dLp, dCp = L2 - L1, C2p - C1p
    if C1p * C2p == 0:
        dhp = 0.0
    else:
        dhp = h2p - h1p
        dhp = dhp - 360 if dhp > 180 else dhp + 360 if dhp < -180 else dhp
    dHp = 2 * np.sqrt(C1p * C2p) * np.sin(np.radians(dhp / 2))
    Lpm, Cpm = (L1 + L2) / 2, (C1p + C2p) / 2
    if C1p * C2p == 0:
        hpm = h1p + h2p
    elif abs(h1p - h2p) <= 180:
        hpm = (h1p + h2p) / 2
    else:
        hpm = (h1p + h2p + 360) / 2 if h1p + h2p < 360 else (h1p + h2p - 360) / 2
    T = (1 - 0.17 * np.cos(np.radians(hpm - 30)) + 0.24 * np.cos(np.radians(2 * hpm))
         + 0.32 * np.cos(np.radians(3 * hpm + 6)) - 0.20 * np.cos(np.radians(4 * hpm - 63)))
    dtheta = 30 * np.exp(-(((hpm - 275) / 25) ** 2))
    Rc = 2 * np.sqrt(Cpm ** 7 / (Cpm ** 7 + 25 ** 7))
    Sl = 1 + 0.015 * (Lpm - 50) ** 2 / np.sqrt(20 + (Lpm - 50) ** 2)
    Sc, Sh = 1 + 0.045 * Cpm, 1 + 0.015 * Cpm * T
    Rt = -np.sin(np.radians(2 * dtheta)) * Rc
    return float(np.sqrt((dLp / Sl) ** 2 + (dCp / Sc) ** 2 + (dHp / Sh) ** 2 + Rt * (dCp / Sc) * (dHp / Sh)))

def delta_e76(lab1, lab2):
    return float(np.linalg.norm(np.asarray(lab1) - np.asarray(lab2)))

# conversions between a target XYZ (D65) and the encodings of the probe
def to_rgb_space(xyz65, matrix, enc):
    """Encoded coordinates of the space with this linear matrix and transfer function."""
    return enc(np.linalg.solve(matrix, xyz65))

def from_rgb_space(rgb, matrix, dec):
    return matrix @ dec(np.asarray(rgb, dtype=float))

def srgb_to_xyz65(rgb):
    return M_SRGB @ srgb_dec(rgb)

def xyz65_to_lab50(xyz65):
    """The CIELAB (D50) of a colour given as XYZ (D65): the colour is adapted first, as ICC does."""
    return xyz_to_lab(adapt(xyz65, D65, D50), D50)

def in_gamut(rgb, margin=0.0):
    rgb = np.asarray(rgb)
    return bool(np.all(rgb >= -margin) and np.all(rgb <= 1 + margin))

if __name__ == "__main__":
    # self test: published values
    assert abs(M_SRGB[1, 0] - 0.2126390058715104) < 1e-9, M_SRGB  # the value of CSS Color 4
    assert np.allclose(M_SRGB @ [1, 1, 1], D65, atol=1e-9)
    # sRGB red in CIELAB D50 is lab(54.29 80.80 69.89) (CSS Color 4, lab() example)
    lab = xyz65_to_lab50(srgb_to_xyz65([1, 0, 0]))
    assert np.allclose(lab, [54.29, 80.80, 69.89], atol=0.05), lab
    # Display P3 red in the same space: lab(59.89 ... ) is not asserted; the primaries matrix is
    assert np.allclose(M_P3[:, 0], [0.48657, 0.22897, 0.0], atol=2e-5), M_P3[:, 0]
    # Sharma, Wu, Dalal test pairs 1, 2 and 5
    for pair, expected in [(((50.0, 2.6772, -79.7751), (50.0, 0.0, -82.7485)), 2.0425),
                           (((50.0, 3.1571, -77.2803), (50.0, 0.0, -82.7485)), 2.8615),
                           (((50.0, 2.5, 0.0), (50.0, 0.0, -2.5)), 4.3065),
                           (((60.2574, -34.0099, 36.2677), (60.4626, -34.1751, 39.4387)), 1.2644)]:
        got = delta_e2000(*pair)
        assert abs(got - expected) < 1e-3, (pair, got, expected)
    # round trips
    xyz = srgb_to_xyz65([0.3, 0.6, 0.2])
    assert np.allclose(lab_to_xyz(xyz_to_lab(xyz, D50), D50), xyz)
    assert np.allclose(rec2020_dec(rec2020_enc([0.001, 0.2, 0.9])), [0.001, 0.2, 0.9])
    print("colour.py: self test passed")
