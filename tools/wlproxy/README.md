# wlproxy -- the plat-wayland acceptance tools (agent plat-wayland, R4)

wlinject.py is a Wayland protocol proxy inside WSL that plays pointer, keyboard and the other
clipboard clients for a real app window, so the user's input and system clipboard stay out.
accept.py reruns the whole comparison of linuxtest (asm build/rwref vs Rust build) on Wayland.
The scripts expect to live in build/plat-wayland/ (they write their outputs there):

    mkdir -p build/plat-wayland && cp tools/wlproxy/* build/plat-wayland/
    python build/plat-wayland/accept.py
