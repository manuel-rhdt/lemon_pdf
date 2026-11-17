"""Renders the pages of a probe with Ghostscript (lcms2, with an explicit output profile), MuPDF and
poppler, and samples the patches of the layout.

A patch is sampled as the median of the central 50 % of its rectangle, per channel.
"""
import json
import os
import subprocess
import tempfile
import numpy as np
import tifffile
from PIL import Image, ImageCms
import io

SCALE = 4   # pixels per point: 288 dpi


def load_layout(directory):
    with open(os.path.join(directory, "layout.json")) as f:
        return json.load(f)


def render_gs(pdf, output_profile, dpi=288, intent=None):
    """16-bit RGB in the space of `output_profile`, as arrays (height, width, 3) in 0..1."""
    with tempfile.TemporaryDirectory() as tmp:
        cmd = ["gs", "-q", "-dNOPAUSE", "-dBATCH", "-dNOOUTERSAVE", "-sDEVICE=tiff48nc", f"-r{dpi}",
               f"-sOutputICCProfile={output_profile}", f"-sOutputFile={tmp}/p%d.tif", pdf]
        if intent is not None:
            cmd.insert(1, f"-dRenderIntent={intent}")
        result = subprocess.run(cmd, capture_output=True, text=True)
        pages = []
        n = 1
        while os.path.exists(f"{tmp}/p{n}.tif"):
            pages.append(tifffile.imread(f"{tmp}/p{n}.tif").astype(np.float64) / 65535.0)
            n += 1
        return pages, result.stderr


def render_mupdf(pdf, scale=SCALE):
    import pymupdf
    doc = pymupdf.open(pdf)
    pages = []
    for page in doc:
        pix = page.get_pixmap(matrix=pymupdf.Matrix(scale, scale), colorspace=pymupdf.csRGB, alpha=False)
        pages.append(np.frombuffer(pix.samples, dtype=np.uint8).reshape(pix.height, pix.width, 3).astype(np.float64) / 255.0)
    return pages


def render_poppler(pdf, dpi=288):
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(["pdftoppm", "-r", str(dpi), "-png", pdf, f"{tmp}/p"], check=True, capture_output=True)
        files = sorted(f for f in os.listdir(tmp) if f.endswith(".png"))
        return [np.asarray(Image.open(os.path.join(tmp, f)).convert("RGB")).astype(np.float64) / 255.0 for f in files]


def sample(image, rect, scale=SCALE, shrink=0.25):
    """The median colour of the central part of `rect` = [x, y, w, h] in points (top left origin)."""
    x, y, w, h = rect
    x0, x1 = int(round((x + w * shrink) * scale)), int(round((x + w * (1 - shrink)) * scale))
    y0, y1 = int(round((y + h * shrink) * scale)), int(round((y + h * (1 - shrink)) * scale))
    region = image[y0:max(y1, y0 + 1), x0:max(x1, x0 + 1)].reshape(-1, 3)
    return np.median(region, axis=0)


def icc_profile(data_or_path):
    if isinstance(data_or_path, (bytes, bytearray)):
        return ImageCms.ImageCmsProfile(io.BytesIO(data_or_path))
    return ImageCms.getOpenProfile(data_or_path)
