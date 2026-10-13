# platshared -- the plat-shared acceptance tools (agent plat-shared, R4)

Printing, the xdg-desktop-portal client, the plugin loader / crash guard and the monitor
profile of `lib/plat` against the assembly reference.  Nothing here opens a real dialog,
touches a printer (only "Microsoft Print to PDF" into files) or the user's clipboard.

The tools need a copy of the reference build inside the checkout they run in (WSL paths are
repo-relative):

    mkdir -p build/rwref/win build/rwref/linux build/rwref/plugins/win build/rwref/plugins/linux
    copy <ref>/win/{PaintCraft,gfxtest,codectest,template}.exe  -> build/rwref/win/
    copy <ref>/linux/{PaintCraft,gfxtest,codectest,template}    -> build/rwref/linux/
    copy <asm build>/{win,linux}/plugins/*           -> build/rwref/plugins/{win,linux}/
         (python build.py plugin --target both --out <asm build>)
    python build_rs.py app plattest --target both
    python build_rs.py app template --target both
    python tools/platshared/accept.py

| Tool | What |
|---|---|
| `accept.py` | everything below + the monitor profile (plattest --test icc against codectest op 325) |
| `print_rs_check.py` | layout replay; jfgui_PRINT_TO PDFs byte for byte with PaintCraft (Linux); Microsoft Print to PDF files byte for byte apart from their dates (Windows) |
| `clip_rs_check.py` | clip_check.py's DIB / BMP variants through template's `clip dib` / `clip file` + `clip save bmp`: the BMPs byte for byte with the assembly, the expected pixels, invalid DIBs refused |
| `lib_check.py` | plattest --test guard / guardcrash / lib (the assembly's test plugins); loader fuzz counts against PaintCraft --plugin-fuzz |
| `icc_x11.py` | (inside WSL) a raw X11 client sets / restores the root window's _ICC_PROFILE around plattest --test iccx11 (valid, invalid, above 1 MB, none) |
| `portal_bus.py` | (inside WSL) fake session bus: OpenDirectory / OpenURI (gfxtest --open) and Print (PaintCraft) byte for byte; FileChooser and OpenURI of plattest --test portal checked |

Work files go to `build/plat-shared/`.
