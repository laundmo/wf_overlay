# Warframe Relic Overlay

Just shows reward prices right now, *extremely* early state. Only works on linux, probably only on wayland.

Only tested on a 4k monitor, very possible it won't work on other resolutions (tho an attempt has been made).

## Setup

1. Run this in `assets/` to download OCR models: [download_models.sh](https://github.com/robertknight/ocrs/blob/4d76906598bfb4f539fd12d554c9c402dfa78be3/ocrs/examples/download-models.sh)
2. Make sure your user is in the `input` group.
    1. For most distros, run `sudo usermod -a -G input $USER` and then reboot
3. (Compile and) run wf_overlay
4. Select main screen in the Desktop Portal
5. Go ingame
6. Hit I during a relic screen

On KDE, i've had to set a window rule to force it to the Overlay layer, YMMV.

## What it does

This tool uses OCR to read relic reward names from the screen and try to give a plat price.

It slowly updates its list of plat prices in the background, to hopefully avoid spamming the WFM API too much. Of course if it doesn't have data about something yet, it will fetch it from the market immediately.