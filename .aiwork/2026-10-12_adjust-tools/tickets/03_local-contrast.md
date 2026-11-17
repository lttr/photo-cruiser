---
status: done
verified: [behaviour]
blocked_by: [01]
references:
  - "Research: ../research.md"
---

# 03 — Local contrast

**What to build:** darktable's local contrast. Opens at detail 125%, slider 100–200%; highlights 50%, shadows 50% and midtone range 0.5 fixed. Builds the edge-aware smoothing filter that Shadows & highlights and Dehaze reuse.

## Acceptance criteria

- [x] No halos along strong edges at 200%
- [x] The smoothing filter lives in core, reusable by other tools (`core::smooth::guided`, built for Shadows & highlights)
- [x] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [x] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [x] Unit test in core for the pixel math; a render golden image
- [x] `just verify` passes; headless shot shows the effect

## Verification (behaviour)

- `just verify` passed: `verify ok`, including `verify-local-contrast.png`. The tool opens at 1.25 and is back at 1.25 after a change plus Auto; the footer shows "Local contrast 125%". It stacks on the shadows & highlights edit that the earlier verify step saved when it left the photo.
- Headless real window, fresh copy of `fixtures/adjust`, palette query "local": one row, "Local contrast" with key `D`. Activating it opens the tool at 1.25 (`edits` = local_contrast 1.25), dirty, no errors.
- Measured in the window at 200% against the photo without edits (screen pixels, green channel): the wall's grain goes from 27–31 to 25–33. The wall right next to the window goes from 27–31 to 26–32, within the far grain, so it doesn't darken. The sky around the tree crown rises by at most 2 levels.
- **The first pass found a halo** that the tests had missed. The math then worked in log luminance with an edge threshold of 0.5 EV. The sky next to the crown (a soft edge, about 1 EV) rose from 216 to 228. Lowering the threshold didn't help: in log luminance, dark texture (±6 levels at sRGB 34 is about 0.4 EV) is as big as that edge, so no threshold kept one and boosted the other. The fix splits perceptual lightness (sRGB-encoded luminance, like darktable's Lab L) instead, with a threshold of 0.05.
- Tests: `local_contrast::boosts_texture_without_halos` now covers a strong (about 3 EV), a medium (about 1 EV) and a soft (0.5 EV) edge at 200 %. The medium edge reproduced the halo before the fix. The golden test `local_contrast_on_a_backlit_room` now also checks the sky around the crown, and it fails with a loose threshold (0.3). Golden image: `window-local-contrast.png`.
- Undo, Esc, Auto and No change: `update_local_contrast_opens_at_125`. `D` key: `tool_keys`. pcr: `local_contrast_render_and_save` (render, edits JSON, save).
- A faint line in the lower sky at 200 % is a ±1-level JPEG block step in the fixture, boosted. That's the tool working as intended.
- Real photos: 8 random JPEGs from the user's photo library (copied to `/tmp/photo-cruiser/real`, not committed), rendered by `pcr` at 100, 125 and 200 %. Texture in snow, fleece, hair and paving gets stronger. At 1:1 there are no halos on the strongest edges: snow against dark water, a red wing against a night sky, a hat against a bright background.
- Noise: in dark areas the tool boosts sensor noise too, since to the filter it's fine detail (darktable does the same). In a night sky (sRGB ~19), the noise's standard deviation goes from 9.4 to 10.9 at 125 % and 14.6 at 200 %; the average brightness barely moves (18.6 → 19.4). Kept as it is: the user found darktable's defaults worked well, noise and all.

## Notes

- Value: the detail factor itself, 1–2, shown as 100–200 %. Neutral is 1 (the first tool whose neutral value isn't 0). Keyboard steps are 10 % (fine: 1 %).
- Math: lightness L = sRGB-encoded luminance, split into base + detail. The base comes from `smooth::guided_fit` (radius 1 % of the longer side, variations over about 0.05 in lightness count as edges). The change in lightness is `(amount − 1)·(1 − a)·detail`, capped at ±0.25. It's applied as a gain on linear RGB, so the hue stays. The fit's slope `a` is near 1 at edges, so the extra `1 − a` stops the edge itself from being boosted. darktable's highlights, shadows and midtone range stay at their defaults, so they aren't modelled.
- `smooth::guided_fit` is new: it returns the filter's `(a, b)` at full size, and `guided` is built on it.
- Order: after shadows & highlights, before colour.
- Key `D` ("detail", darktable's name for the slider). JSON name `local_contrast`; `pcr` also accepts `clarity`.
- Speed (release, `pcr render` to JPEG): +0.5 to +1.4 s on 12–20 MP photos, 10–30 % more than Shadows & highlights on the same photos (a 20 MP photo: 4.3 s plain, 5.2 s with S&H, 5.7 s with Local contrast).
