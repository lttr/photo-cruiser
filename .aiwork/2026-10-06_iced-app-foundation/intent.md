---
status: accepted
references:
  - "Prototypes: ~/sandbox/photo-cruiser (10 stacks, comparison.html)"
  - https://html.lukastrumm.com/photo-cruiser-stack-comparison/
---

# Intent

Compile all architecture findings and directions from the prototypes and create an initial spec
for building the iced-based app, which splits GUI and lib, and can be operated from the CLI and by agents.

Background, in the user's words from the prototyping session:

- A photo app for myself, potentially for friends who run different OSes.
- It definitely needs to be fast.
- Future features: format conversion, photo editing like improving low contrast (a Darktable tool), sharpening, showing images from Samba shares.
- Native look and feel is nice (liked the COSMIC app); Slint was ruled out because of its licensing.
