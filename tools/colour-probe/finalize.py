"""Gives the marked ExtGState dictionaries of a probe an /RI entry.

lemon_pdf has no /RI in ExtGState (it can only set the intent with the `ri` operator), so
examples/colour_probe.rs marks the graphics states that are to have one with the fill alpha
0.91 (Perceptual), 0.92 (RelativeColorimetric), 0.93 (Saturation) and 0.94
(AbsoluteColorimetric); lemon_pdf writes two decimals. This replaces the alpha by the /RI. It is a tool of the test, and it says what
a viewer does with /RI; it is not something that lemon_pdf can do.

    python3 finalize.py probe.pdf probe-final.pdf
"""
import sys
import pikepdf

INTENTS = ["Perceptual", "RelativeColorimetric", "Saturation", "AbsoluteColorimetric"]


def finalize(source, target):
    pdf = pikepdf.open(source)
    changed = 0
    for page in pdf.pages:
        states = page.Resources.get("/ExtGState", {})
        for name in list(states.keys()):
            state = states[name]
            ca = float(state.get("/ca", 1.0))
            for index, intent in enumerate(INTENTS):
                if abs(ca - (0.91 + 0.01 * index)) < 0.004:
                    del state["/ca"]
                    state["/RI"] = pikepdf.Name("/" + intent)
                    changed += 1
    pdf.save(target, deterministic_id=True)
    return changed


if __name__ == "__main__":
    print(f"{finalize(sys.argv[1], sys.argv[2])} ExtGState dictionaries got an /RI")
