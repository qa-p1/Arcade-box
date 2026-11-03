if _G.arcadeBoxSurfaceRule then _G.arcadeBoxSurfaceRule:set_enabled(false) end
    if _G.arcadeBoxIslandRule then
        _G.arcadeBoxIslandRule:set_enabled(true)
    else
        _G.arcadeBoxIslandRule = hl.window_rule({
            name = "arcade-box-island-window",
            match = { class = [[^dev\.arcade\.Box$]], title = "^Arcade Box$" },
            float = true, pin = true,
            move = {"(monitor_w-window_w)/2", 0},
            no_blur = true, no_shadow = true, no_dim = true, no_anim = true,
            decorate = false, border_size = 0, rounding = 0,
            opacity = "1 override 1 override 1 override"
        })
    end
