---
references:
  - https://github.com/darktable-org/darktable/tree/master/src/iop
  - https://docs.darktable.org/usermanual/development/en/overview/workflow/process/
  - https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/exposure/
  - https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/sigmoid/
  - https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/rgb-levels/
  - https://discuss.pixls.us/t/can-you-edit-jpeg-with-a-scene-referred-workflow/36245
  - https://discuss.pixls.us/t/quick-and-simple-settings-for-darktable-to-get-great-results-quickly/36987
  - https://discuss.pixls.us/t/how-to-get-darktable-sharpness-to-match-lightroom/35125
---

# Adjustment tools inspired by darktable

Goal: one slider per tool. Nothing is applied to a photo until the user opens a tool.
Opening a tool applies a sensible starting value right away (as darktable does when you
enable a module), as one undoable step. Esc reverts it. Double-clicking the slider resets
it to the opening value.

## Why the values are only applied when a tool opens

darktable auto-applies exposure +0.7 EV, sigmoid/filmic and color calibration to **raws
only**. That's gated by `dt_image_is_rawprepare_supported` / `FOR_RAW | FOR_MATRIX` in
`src/iop/*.c`. JPEG, HEIC and WebP are already tone-mapped in the camera, and pixls users
advise against adding a scene-referred tone curve on top. So the app's baseline stays at
"no change", and darktable's defaults and presets become each tool's opening value.

## Tools

| Tool | Value when opened | Slider range | Source |
|---|---|---|---|
| Exposure | Auto: brings average brightness to middle grey | ±2 EV | exposure spot picker targets 50% lightness |
| Colour | 1.0× "basic colorfulness: standard" (saturation 0.2, shadows +0.25, mid-tones 0, highlights −0.25; darktable UCS) | 0–2× | color balance rgb preset |
| Local contrast | detail 125% (highlights 50%, shadows 50%, midtone range 0.5 fixed) | 100–200% | local contrast default (local laplacian); "clarity" preset 133%, HDR 200% |
| Straighten | Auto angle from detected lines; empty corners cropped | ±45° | rotate & perspective "fit" |
| Shadows & highlights | compress 0.45 ("medium") | 0–1 | tone equalizer "compress shadows/highlights": soft 0.25, medium 0.45, strong 0.65 |
| Warmth | Grey-world auto white balance | ± temperature | color calibration "surfaces" detection |
| Vignette | brightness −0.5 (start 80%, radius 50%, shape 1, ratio 1 fixed) | −1…+1 | vignette defaults |
| Sharpen | amount 0.5 (radius 2, threshold 0.5 fixed) | 0–1 | sharpen defaults |
| Dehaze | strength 0.2 (distance 0.2 fixed) | 0–1 | haze removal defaults |
| Contrast | ~1.2 (unverified guess) | 1.0–2.0 | sigmoid default 1.5 is meant for raws; "neutral gray" preset uses 1.22 |
| Auto levels | strength 1: auto black/white point per channel, 0.5% clipped at each end | strength 0–2: 0–1 blends from original, 1–2 raises clip to 5% | rgb levels "auto", independent channels; for photos of documents |

Color balance rgb vibrance is `v·(1 − Y^|v|)`, which favours low-chroma pixels.

## Left out

- Denoise (profiled): needs camera noise profiles.
- Highlight reconstruction: raw only.
- Lens correction: needs lensfun profiles; phone JPEGs are usually corrected already.
- Filmic rgb: too many controls; Contrast (sigmoid) replaces it.

## Unverified

- No community consensus values were found for haze strength or sharpen amount.
- The raws-only auto-apply comes from reading darktable's source, not from testing.
- The Contrast opening value (~1.2) needs checking on real photos.
- The darktable manual doesn't say which percentiles rgb levels "auto" clips at; 0.5% is a guess.
