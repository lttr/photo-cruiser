---
status: done
verified: [checks, behaviour]
blocked_by: [01, 03]
references:
  - "Research: ../research.md"
---

# 09 — Dehaze

**What to build:** Like darktable's haze removal. Opens at strength 0.2, slider 0–1; distance fixed at 0.2. Uses the smoothing filter from Local contrast.

## Acceptance criteria

- [x] A photo without haze changes only slightly at the opening value
- [x] Opening the tool applies the opening value as one undoable step; Esc reverts it; double-clicking the slider resets it to the opening value
- [x] Available from the command palette; works the same through `pcr` (edits JSON, render, save)
- [x] Unit test in core for the pixel math; a render golden image
- [x] `just verify` passes; headless shot shows the effect

## Verification (checks, behaviour)

- `just check` and `just verify` passed (`PC_ARTIFACTS=/tmp/photo-cruiser/09/art`), including the new `verify-dehaze` step: on `adjust/woods-haze.jpg` the tool opens at 0.2 and is back at 0.2 after a change plus Auto. The footer reads "Dehaze 0.20". `exposure_slider_drag_and_double_click` failed once in a `just check` run while four sibling worktrees were building (13 s test run); it passed 3/3 alone and in the following `just verify`.
- Headless real window, fresh copy of `fixtures/adjust`: palette query "haze" gives one row, "Dehaze" with key `M`; activating it opens the tool at 0.2 (`edits` = dehaze 0.2), no errors. Slider to 1.0, then Esc: `edits` empty, tool closed, the shot is identical to the original's.
- Effect in the window (mean green of screen regions; original → 0.2 → 1.0): sky 183 → 183 → 183, far ridge 177 → 173 → 173, middle ridge 153 → 147 → 145, near ground 108 → 103 → 84. The haze lifts, the sky stays. Above 0.2 only nearer parts change more, because of the fixed distance.
- Photos without haze: the 12 real photos in `/tmp/photo-cruiser/contrast-samples/`, rendered by `pcr` at a 1200 px long side, at 0.2 against none. The mean absolute change is 1.0–2.9 % (about 2.5–7 levels). They look slightly deeper, with no halos along tree tops or ridges against the sky. A hazy mountain valley (DSC_0417) clears visibly. One photo (DSC_0487) doesn't change: over 5 % of it is clipped white cloud (see Notes). The golden test also checks `adjust/window.jpg` (mean change under 3 levels).
- Tests: `dehaze::{airlight_is_the_brightest_haze, clear_inverts_haze, lifts_the_veil_more_where_hazier_up_to_the_distance, no_haze_at_the_sky_edge, zero_flat_and_black_are_identity}`; golden `dehaze_on_hazy_hills_and_a_clear_room` and image `woods-haze-dehaze.png`. App: `update_dehaze_opens_at_0_2` (one undo step, Auto, No change, Esc) and `M` in `tool_keys`. pcr: `dehaze_render_and_save` (render, `haze_removal` alias, edits JSON, save).

## Notes

- darktable's math (`src/iop/hazeremoval.c`): the airlight is the mean of the brightest 5 % (R+G+B) of the 5 % haziest pixels (dark channel); `t = 1 − strength · min_c(pixel / airlight)`, smoothed; `t_min = level^(1.125 · distance)` (level: the haziest 5 %'s dark channel); `(pixel − airlight) / max(t, t_min) + airlight`, in linear RGB.
- Deviations: the dark channel is per pixel, not darktable's 6 px minimum window, and it is smoothed by the self-guided filter (`smooth::guided`, radius 0.25 % of the longer side, edges above 0.1 kept) instead of darktable's colour-guided one. darktable's window would spread a dark ridge's low haze into the sky beside it, which only a colour-guided filter hides.
- Because of the fixed distance 0.2, far haze is only partly removed (at most 1 / level^0.225, e.g. 14 % for a level of 0.55) whatever the strength; higher strength mostly clears nearer parts. When over 5 % of the photo is clipped white, level is 1 and the tool does nothing. Both follow darktable's formula.
- Order: after Warmth, before Exposure. Key `M`. JSON name `dehaze`; `pcr` also accepts `haze_removal`.
- New fixture `adjust/woods-haze.jpg`, named to sort after `window.jpg` so the other tests' photo indices stay.
