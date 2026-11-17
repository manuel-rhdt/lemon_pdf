# Colour probe

Tests which colour encodings of `lemon_pdf` render correctly on a device (Preview, Files, Safari,
any other viewer). Acceptance is the rendering on the device; the generator is checked first so that a
`lemon_pdf` bug is not mistaken for the behaviour of a viewer.

## Making the probe

```sh
python3 colour.py                                   # the self test of the colour mathematics
python3 spec.py OUT                                 # OUT/probe.spec, OUT/layout.json, the profiles of OUT
cargo run --example colour_probe -- OUT/probe.spec OUT/probe.pdf
python3 finalize.py OUT/probe.pdf OUT/colour-probe.pdf   # the /RI of ExtGState (see below)
```

`spec.py OUT --mutate NAME` writes a file with a deliberate mistake (`wrong_trc`, `wrong_white`,
`wrong_primaries`, `unadapted_carrier`) for negative controls: the expectation comes from the target
colour, so a mutated file must fail a check that the right one passes.

| file | what it does |
| --- | --- |
| `colour.py` | colour mathematics in float64, independent of `lemon_pdf` (Bradford, CIELAB, CIEDE2000, sRGB, Display P3, Rec. 2020), with a self test |
| `iccgen.py` | an ICC profile writer, independent of `lemon_pdf`: matrix/TRC profiles of versions 2 and 4 (table, gamma, parametric curves) and LUT-based profiles with a table for each rendering intent |
| `spec.py` | the probe: rows, patches, ladders; the spec for the generator; `layout.json` with the ground truth of every patch |
| `../../examples/colour_probe.rs` | writes the PDF from the spec through `lemon_pdf`; it converts nothing |
| `finalize.py` | gives four marked ExtGState dictionaries an `/RI` (see below) |
| `render.py` | renders with Ghostscript (lcms2, explicit output profile), MuPDF and poppler, and samples patches |

The numbers of the spec are the ones of the file: `lemon_pdf` writes colour operands with up to six
decimals (`Precise`), and the generator does no conversion.

## What `lemon_pdf` has (Step 0)

Public colour API, read from the source:

- device colours: `device_gray_*`, `device_rgb_*`, `device_cmyk_*_color` (fill and stroke), six decimals
- `add_color_space` with `LabColorSpace` (white point, range), `CalRgbColorSpace` (white point, gamma,
  matrix) and `IccBasedColorSpace` (any profile, from `DocumentContext::write_icc_profile` or the
  cache `icc_color_space`), then `set_fill_color_components` and `set_stroke_color_components`
- `icc`: `srgb_profile` (v2, table curve), `display_p3_profile` and `rec2020_profile` (v4,
  parametric curve), and `rgb_matrix_profile` with a `ParametricCurve` (any type 3 curve: sRGB, Rec. 2020,
  pure gamma)
- the rendering intent as the `ri` operator: `set_rendering_intent`
- `TransparencyGroup` with a device colour space; shadings and patterns with device colours only
  (`Gray`, `Rgb`, `Cmyk`); text is painted with the fill colour

## What it lacks

These are not tested (or only through a test tool), and none is invented:

- `ExtGState` has no `/RI`. The probe marks four graphics states and `finalize.py` adds the `/RI`
  afterwards, so the device test says what a viewer does with it; adding the field to `lemon_pdf` is a
  separate decision.
- no `CalGray`, no `DefaultRGB`, `DefaultGray` or `DefaultCMYK`, no `BlackPoint` for `CalRGB` and Lab
- the stream dictionary of an ICC profile has `/N` only: no `/Alternate`, no `/Range`
- no ICC (or Lab, CalRGB) colour space for images: `ColorSpace` is Device only, an indexed palette is
  RGB, and the profiles of PNG and JPEG files are ignored
- the page has no `/Group` (its colour space), and `TransparencyGroup` takes only a device space
- no builder of LUT-based profiles; `iccgen.py` writes the ones of the probe
- `/ca` and every other `f64` of a graphics state have two decimals (`Precise` is for colours)

## The device tests

`colour-probe.pdf` has six pages of 360 x 640 pt. Each page has a frame and a calibration strip of
DeviceRGB greys and sRGB primaries.

- Part A (pages 1 to 3): a pair of patches for every encoding: the colour in the encoding next to a
  DeviceRGB patch of the same target colour. Rows: ICC sRGB (v2, v4), Display P3 (v4, v2), Rec. 2020 with
  three curves (inverse OETF, gamma 2.4, sRGB curve) and a LUT-based profile; CalRGB (an XYZ carrier with
  an exact and a rounded white point, with a D50 white point, and gamma 2.2 with real primaries); Lab
  (D50 and D65 white, adapted and not, two ranges); and a profile whose tables differ by intent, with the
  default intent, `ri` and `/RI`. A 17 step neutral ramp and 8 hues in every row, all inside sRGB.
- Part B (pages 4 to 6, one per hue): ladders of 12 steps from the edge of sRGB to the Rec. 2020 colour
  on the clipped sRGB background. Clipping gives a plateau beyond the edge of Display P3, compression
  moves the thresholds, and no colour management gives never readable. The Display P3 row is the control.
- `capture-test.png` (made by the person who runs the test): a Display P3 image, to find out whether
  pixels outside sRGB survive a screenshot.

## Round 1: iPhone 16 Pro, Preview and Files (Part A, in-gamut)

Measured from P3-tagged screenshots (JPEG, registered by the frame of the page; the calibration strip is
within 0.6 levels of nominal, and the DeviceRGB patches are within 0.3 CIEDE2000 of their targets, so
DeviceRGB is sRGB on the device). The numbers are the mean CIEDE2000 between the patch in the encoding and
the DeviceRGB patch of the same colour (ramp / hues); the fits say which interpretation the device applies
(`hypotheses.py`).

| encoding | verdict | CIEDE2000 | what the device does |
| --- | --- | --- | --- |
| DeviceRGB, DeviceGray | works | 0.09 / 0.28 from the target | DeviceRGB is sRGB |
| ICC sRGB v2 (table), sRGB v4, Display P3 v4 (parametric), Display P3 v2 (table) | works | 0.00 | exact |
| ICC Rec. 2020, gamma 2.4 curve | works | 0.00 | exact |
| ICC Rec. 2020, sRGB curve | works | 0.00 / 0.04 | exact |
| ICC Rec. 2020, LUT-based | works | 0.12 / 0.41 | exact within the table |
| ICC Rec. 2020, inverse OETF (what `rec2020_profile` has) | fails | 5.5 / 5.2 | decodes with a pure gamma 2.4: fit 0.14, against 5.4 for the profile's curve |
| CalRGB, sRGB primaries, gamma 2.2 | works | 0.00 | exact |
| CalRGB XYZ carrier, matrix 1.2 (D65 exact, D65 rounded, D50) | fails | 7.5 / 5.3, 7.6 / 5.3, 7.2 / 5.5 | scales the matrix so that (1, 1, 1) is the white point: fit 0.7, 0.7, 0.5, against 6.8, 6.9, 6.7 for the specification |
| Lab D50, range ±256 (also D65, and not adapted) | fails | 0.00 / 10.4 | a* and b* at half: fit 0.50 (0.16), against 3.4 for 1.0 |
| Lab D50, range -128..127 | works, with a condition | 1.2 / 0.6 | right; the neutrals have about 1.2 chroma |
| `ri` and `/RI`, any of the four intents, and no intent | not honoured | 6.2 / 13.4 | always the perceptual table (A2B0): 0.22, against 8.5 (relative) and 10.5 (saturation) |

Not determined: wide-gamut retention (Part B), Safari and other viewers, whether a matrix/TRC profile
shows an intent effect (it cannot by construction).

Inferred and not measured: that Lab scales a* and b* by 128 / (half the range) for any range (two ranges
only so far), that the device scales the CalRGB matrix to the white point (a fit; the carrier with
matrix = diag(white) is the test), and that it is the profile of Rec. 2020 that makes the device use gamma
2.4 (Apple's own profile is the test).

What changed in `lemon_pdf` because of it: `LabColorSpace::d50()` has the range -128..127,
`CalRgbColorSpace::xyz_carrier` makes the carrier whose components (1, 1, 1) are the white point, and
`icc::rec2020_gamma24_profile` is the Rec. 2020 profile that the device shows as it says.

## Round 2

`python3 spec.py OUT --round 2` (with `APPLE_REC2020=` the path of Apple's Rec. 2020 profile, taken from a
PDF that Safari made, which is not in the repository): the carriers with matrix = diag(white) (gamma 1 and
2.2, D65 and D50), Lab with six ranges and with ±256 and doubled a* and b* (the scaling law), Rec. 2020 with
gamma 2.4 and Apple's profile, and Part B again with the rows that work.
