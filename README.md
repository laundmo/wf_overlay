# Warframe Relic Overlay

Just shows reward prices right now, *extremely* early state. Only works on linux, probably only on wayland.

## Setup

1. Check wf_overlay.toml for the settings like keybinds
2. Run this in `assets/` (might need to create) to download OCR models: [download_models.sh](https://github.com/robertknight/ocrs/blob/4d76906598bfb4f539fd12d554c9c402dfa78be3/ocrs/examples/download-models.sh)
3. Make sure your user is in the `input` group.
    1. For most distros, run `sudo usermod -a -G input $USER` and then reboot
4. (Compile and) run wf_overlay
5. Configure you Desktop Environment of choice so that wf_overlay is always on top (on KDE, set "layer" to Overlay using Window Rules)
6. Select main screen in the Desktop Portal
7. Go ingame
8. Hit the configured keybind during a relic screen


## What it does

This tool uses OCR to read relic reward names from the screen and try to give a plat price.

It slowly updates its list of plat prices in the background, to hopefully avoid spamming the WFM API too much. Of course if it doesn't have data about something yet, it will fetch it from the market immediately.