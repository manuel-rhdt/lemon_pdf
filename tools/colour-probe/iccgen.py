"""An ICC profile writer, independent of lemon_pdf, for the profile variants of the colour probe:
matrix/TRC profiles (versions 2 and 4, curves as tables, gamma or parametric) and LUT-based
profiles with a different table for each rendering intent (A2B0 perceptual, A2B1 relative
colorimetric, A2B2 saturation), which show on a device which table it uses."""
import struct
import numpy as np
import colour as C

def s15(v):
    return struct.pack(">i", int(round(v * 65536)))

def pad4(b):
    return b + b"\0" * ((4 - len(b) % 4) % 4)

def xyz_tag(xyz):
    return b"XYZ \0\0\0\0" + b"".join(s15(v) for v in xyz)

def text_tag(text):
    return b"text\0\0\0\0" + text.encode("ascii") + b"\0"

def desc_v2(text):
    t = text.encode("ascii") + b"\0"
    return b"desc\0\0\0\0" + struct.pack(">I", len(t)) + t + b"\0" * (4 + 4 + 2 + 1 + 67)

def mluc(text):
    u = text.encode("utf-16-be")
    return b"mluc\0\0\0\0" + struct.pack(">II", 1, 12) + b"enUS" + struct.pack(">II", len(u), 28) + u

def curv_table(fn, n=1024):
    xs = np.linspace(0, 1, n)
    return b"curv\0\0\0\0" + struct.pack(">I", n) + b"".join(struct.pack(">H", int(round(float(np.clip(fn(x), 0, 1)) * 65535))) for x in xs)

def curv_gamma(g):
    return b"curv\0\0\0\0" + struct.pack(">I", 1) + struct.pack(">H", int(round(g * 256)))

def para3(g, a, b, c, d):
    return b"para\0\0\0\0" + struct.pack(">HH", 3, 0) + b"".join(s15(v) for v in (g, a, b, c, d))

def sf32(m):
    return b"sf32\0\0\0\0" + b"".join(s15(v) for v in np.asarray(m).flatten())

def assemble(version, cls, tags, aliases=None):
    """`tags`: list of (signature, data); `aliases`: {signature: [other signatures with its data]}."""
    aliases = aliases or {}
    entries = []
    body = b""
    first = 128 + 4 + 12 * (len(tags) + sum(len(v) for v in aliases.values()))
    for sig, data in tags:
        offset = first + len(body)
        for s in [sig] + aliases.get(sig, []):
            entries.append((s, offset, len(data)))
        body += pad4(data)
    table = b"".join(s.encode("ascii") + struct.pack(">II", o, n) for s, o, n in entries)
    header = struct.pack(">I", first + len(body)) + b"\0" * 4 + struct.pack(">I", version)
    header += cls + b"RGB " + b"XYZ " + struct.pack(">6H", 2024, 1, 1, 0, 0, 0) + b"acsp"
    header += b"\0" * 28 + struct.pack(">I", 0) + b"".join(s15(v) for v in C.D50) + b"\0" * 4
    header = header.ljust(128, b"\0")
    return header + struct.pack(">I", len(entries)) + table + body

def colorants_d50(xy_primaries):
    m = C.primaries_matrix(*xy_primaries, C.D65)
    return [C.adapt(m[:, i], C.D65, C.D50) for i in range(3)], m

def matrix_profile(desc, xy_primaries, trc, version=4):
    """`trc`: ("table", f(x)->linear), ("gamma", g) or ("para", (g, a, b, c, d))."""
    cols, _ = colorants_d50(xy_primaries)
    kind, arg = trc
    if kind == "table":
        curve = curv_table(arg)
    elif kind == "gamma":
        curve = curv_gamma(arg)
    elif kind == "para":
        assert version >= 4
        curve = para3(*arg)
    else:
        raise ValueError(kind)
    d = desc_v2(desc) if version < 4 else mluc(desc)
    cprt = text_tag("Public Domain") if version < 4 else mluc("Public Domain")
    tags = [("desc", d), ("cprt", cprt), ("wtpt", xyz_tag(C.D50)),
            ("rXYZ", xyz_tag(cols[0])), ("gXYZ", xyz_tag(cols[1])), ("bXYZ", xyz_tag(cols[2])),
            ("rTRC", curve)]
    if version >= 4:
        tags.append(("chad", sf32(C.adaptation(C.D65, C.D50))))
    return assemble(0x02100000 if version < 4 else 0x04000000, b"mntr", tags, {"rTRC": ["gTRC", "bTRC"]})

def mft2(fn, grid=17):
    """A `lut16Type` (mft2) from device RGB (0..1 each) to PCS XYZ (D50), by `fn(rgb) -> xyz`."""
    out = b"mft2\0\0\0\0" + bytes([3, 3, grid, 0])
    out += b"".join(s15(v) for v in np.eye(3).flatten())
    out += struct.pack(">HH", 2, 2)
    out += b"".join(struct.pack(">HH", 0, 65535) for _ in range(3))
    clut = b""
    for r in range(grid):
        for g in range(grid):
            for b in range(grid):
                xyz = fn(np.array([r, g, b]) / (grid - 1))
                # 16-bit XYZ of ICC version 2: u1Fixed15, 1.0 is 0x8000
                clut += b"".join(struct.pack(">H", int(round(np.clip(v, 0, 1.99997) * 32768))) for v in xyz)
    out += clut
    out += b"".join(struct.pack(">HH", 0, 65535) for _ in range(3))
    return out

def lut_profile(desc, fn_perceptual, fn_relative, fn_saturation):
    """An input profile (class scnr, version 2) whose tables for the three intents are different."""
    tags = [("desc", desc_v2(desc)), ("cprt", text_tag("Public Domain")), ("wtpt", xyz_tag(C.D50)),
            ("A2B0", mft2(fn_perceptual)), ("A2B1", mft2(fn_relative)), ("A2B2", mft2(fn_saturation))]
    return assemble(0x02100000, b"scnr", tags)

# parameters of the curves of the probe
SRGB_PARA = (2.4, 1 / 1.055, 0.055 / 1.055, 1 / 12.92, 0.04045)
REC2020_PARA = (1 / 0.45, 1 / C.REC2020_ALPHA, (C.REC2020_ALPHA - 1) / C.REC2020_ALPHA, 1 / 4.5, 4.5 * C.REC2020_BETA)
GAMMA24_PARA = (2.4, 1.0, 0.0, 0.0, 0.0)

if __name__ == "__main__":
    from PIL import ImageCms
    import io
    # the profiles are read by lcms2 and give the colorants that they were made with
    for name, data in [("p3 v4", matrix_profile("P3 v4", C.P3_XY, ("para", SRGB_PARA))),
                       ("p3 v2", matrix_profile("P3 v2", C.P3_XY, ("table", lambda x: float(C.srgb_dec(x))), version=2)),
                       ("rec2020 v4", matrix_profile("R2020", C.REC2020_XY, ("para", REC2020_PARA))),
                       ("rec2020 g2.4", matrix_profile("R2020 g", C.REC2020_XY, ("para", GAMMA24_PARA)))]:
        p = ImageCms.ImageCmsProfile(io.BytesIO(data))
        print(name, len(data), "bytes", ImageCms.getProfileDescription(p).strip())
    lut = lut_profile("lut", lambda c: c, lambda c: c, lambda c: c)
    p = ImageCms.ImageCmsProfile(io.BytesIO(lut)); print("lut", len(lut), "bytes", ImageCms.getProfileDescription(p).strip())
