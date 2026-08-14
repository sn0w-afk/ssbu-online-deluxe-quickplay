# SSBU Online Deluxe

A performance and online enhancement mod for **Super Smash Bros. Ultimate** that introduces latency controls, render optimizations, and real-time online information.

---

> ## 🍴 Quickplay Fork
>
> This fork adds **Quickplay / Elite Smash support** to SSBU Online Deluxe. The original mod only works in Online Arenas and Local Online; with this version, the mod's features also work in quickplay:
>
> - **Latency slider in quickplay**: set your own input delay (0f-25f or Auto) for quickplay/Elite Smash matches, just like arenas. Adjust it from the overlay UI (`ZL + ZR + D-Pad Down`) on the character select screen, or from the arena UI as before — the setting carries over.
> - **Render profiles in quickplay**: your selected NetProfile (LessLag, LLUltra, etc. — vsync off / reduced input delay) is applied when a quickplay match starts and stays active for the whole match, instead of being forced back to Vanilla. Auto mode applies your `online_match` config defaults in quickplay too.
> - Opponent ping / connection info in quickplay via the overlay UI.
> - **Stealth mode**: set `stealth_mode = true` in `config.toml` to never broadcast your latency/render-profile info — opponents on any version of the mod see and log nothing, as if you were a vanilla console — while you still see their extended info. See the config section below.
>
> **Known limitation**: on the quickplay/Elite Smash character select screen, the on-banner text UI (latency/profile readout) does not display — the quickplay VIP banner layout is different from the arena one. Use the overlay UI (`ZL + ZR + D-Pad Down`) instead; it works everywhere.
>
> ### Installation (fork)
>
> This fork is **fully standalone** — see the [📦 Installation](#-installation) section below. Our release zip bundles everything the original release did (skyline runtime, overclock sysmodule, `libnx_over.nro`, compatible `libssbusync.nro`), plus this fork's `libssbu_online_deluxe.nro` and a ready-to-use `config.toml` with stealth mode enabled.
>
> ### Changes from the original ([saad-script/ssbu-online-deluxe](https://github.com/saad-script/ssbu-online-deluxe))
>
> All changes are in `src/`; the mod is otherwise identical to upstream v1.2.0.
>
> **Quickplay mode tracking** (`src/net/mod.rs`)
> - Added a `MatchConnectionStatus::OnlineQuickplay` mode. The quickplay (`online_melee_any`) and background-matchmaking (`online_bg_matchmaking_seq`) scene hooks now set it instead of resetting the mod's mode state to Offline, so all mode-gated features (latency hook, CSS/overlay UI, render profiles) stay active in quickplay.
> - `is_valid_online_mode()` includes quickplay; `update_css` routes the quickplay CSS to a quickplay-specific UI path.
> - Fallback: if a pia-connected match starts while no online mode is tracked (background matchmaking via the main menu resets the tracked mode), the match is treated as quickplay.
>
> **Latency slider** (`src/net/latency_slider/mod.rs`)
> - The `set_online_latency` hook is no longer gated by the tracked online mode (same approach as [latency-slider-de](https://github.com/Naxdy/latency-slider-de)), so the latency override also applies to quickplay matches started via background matchmaking.
>
> **Quickplay CSS UI** (`src/ui/native/mod.rs`)
> - Added `update_quickplay_css_ui`: polls the same inputs as the arena CSS UI and writes the latency/profile text by searching the quickplay CSS pane tree for the `txt_vip_title_*` panes by name (the arena banner-pane traversal does not exist on the quickplay CSS, and running it there crashed the network session). See the known limitation above.
>
> **Render profiles in quickplay** (`src/render/profile.rs`, `src/net/mod.rs`, `src/ui/overlay/mod.rs`)
> - `match_init` no longer relies on the pia connection state to detect an online match — in quickplay the connection is not necessarily registered yet at stage load, which previously made the game fall back to the offline (Vanilla) render profile.
> - **ssbusync restriction bypass (the actual "reverts to Vanilla" fix).** The bundled `libssbusync.nro` deliberately restricts its runtime optimizations (vsync off, double buffering, reduced frame index) to offline, arena, and local-online play: when a match connects without a recognized online mode, it logs *"SsbuSync runtime optimizations may only be used on offline, online arena, or local online! Forcing vanilla runtime..."*, and its per-frame env-flag consumer silently **drops** any non-vanilla flag request while in that state (which is why re-applying the profile every frame could not work). ssbusync exports `ssbusync_restrict_mark_arena_mode` so companion plugins can mark the session type; this fork calls it when entering the quickplay/background-matchmaking scenes, when a match starts, and refreshes it once per second from the overlay draw loop (ssbusync clears its mode flags on every scene transition; the throttle avoids log spam). With the session marked, quickplay behaves exactly like arena mode.
> - `maybe_reapply_match_profile()`: per-frame safety net that re-applies the selected render profile if the live env flags drift from it during an online match.
>
> **Stealth mode** (`src/render/mod.rs`, `src/net/pia/mod.rs`, `src/net/pia/manager_stealth.rs`)
> - New optional `stealth_mode` config key (default `false`). When enabled, the mod never registers the custom-comms send hook, so no packet carrying your latency/render profile is ever broadcast — opponents running any version of the mod receive nothing, see nothing in their overlay, and get nothing in their log, exactly as if you were a vanilla console. Incoming packets are still parsed, so you keep seeing modded opponents' extended info. Receivers also silently ignore version-0 packets (the zeroed-buffer stealth approach used by v1.2.0-quickplay.2), so stealth players on that build leave no log trace here either.
> - **Manager beacon suppression:** `libssbu_pia_manager.nro` has its own lower-level handshake that rides normal PIA traffic: a `0x45` tag byte plus the console's self-reported `nn::nifm` interface type. The tag powers `is_modded` and the `[Wired]`/`[Wifi]` ping suffix on other consoles — so even with the send hook disabled, the manager itself would still out you as modded. In stealth mode we now patch the tag constant (`mov w8, #0x45` → `mov w8, #0x0`) inside the manager's `.text` at runtime (signature scan bounded by `svcQueryMemory`, patched via `sky_memcpy`), making the console indistinguishable from vanilla at every level we control. The receive path is untouched, so you still see everyone else. If the signature isn't found (unknown manager version), a warning is logged and nothing is patched.
>
> Build: see `build.sh` (requires the `skyline-v3` rustup toolchain; `cargo skyline update-std` equivalent setup).
>
> ---

A performance and online enhancement mod for **Super Smash Bros. Ultimate** that introduces latency controls, render optimizations, and real-time online information.

> ⚠️ This is a work in progress. Features and stability may change as development continues.  
> ⚠️ Use at your own risk. I have been testing this mod online personally without any major issues, but there is still a non-zero risk of a ban. The overclocks are intentionally minimal; however, any hardware damage or account penalties remain your responsibility.  

## ✅ Compatibility

- ✔️ Nintendo Switch (console)
- ✔️ Eden Emulator (requires workaround, see installation section below)
- ⚠️ Other emulators: not yet tested (not planned)
- ⚠️ HDR support not yet tested (planned)

## 📦 Installation

> ⚠️ Remove any previous latency slider mod, vsync mod, and less lag mod before proceeding with the installation steps!

### Manual Installation (Console or Emulator)

> ⚠️ Remove any previous latency slider mod, vsync mod, and less lag mod before proceeding!

1. **Download the latest Quickplay Edition release zip** from [this repo's releases page](https://github.com/sn0w-afk/ssbu-online-deluxe-quickplay/releases) and extract it. The zip bundles:
   - The compatible **skyline runtime** (⚠️ do *not* substitute the latest public skyline release — it causes crashes)
   - The **overclock sysmodule** (`atmosphere/contents/00FF0000A11CE0FF`) and `libnx_over.nro`
   - The compatible **`libssbusync.nro`** (⚠️ the public ssbusync release is outdated — use the bundled one)
   - **`libssbu_online_deluxe.nro`** (this fork's build, with quickplay support and stealth mode)
   - A ready-to-use **`config.toml`** with stealth mode enabled (goes in `ultimate/ssbu_online_deluxe/`)
2. Copy the `atmosphere/` and `ultimate/` folders from the extracted zip **to the root of your SD card** (or to your emulator's `sdmc/` folder).
3. Install the remaining **public prerequisites** (not bundled — grab the latest release of each):
   - [arcropolis](https://github.com/raytwo/arcropolis/releases)
   - [nro-hook](https://github.com/ultimate-research/nro-hook-plugin/releases)
   - [smashline](https://github.com/HDR-Development/smashline/releases)
   - [imgui-smash](https://github.com/Coolsonickirby/imgui-smash/releases)
   - [ssbu-pia-manager](https://github.com/project-ultelier/ssbu-pia-interface/releases)

   Each plugin's `.nro` goes in `sd:/atmosphere/contents/01006A800016E000/romfs/skyline/plugins/`.
4. **Eden emulator only**: right-click SSBU → `Configure Game` → `System` tab → check `RNG Seed` → set it to `00000000`.
5. Boot the game. Open the overlay UI (`ZL + ZR + D-Pad Down`) on any online character select screen to confirm the mod is running.

### Automatic Installation (alternative)

You can generate the base SD card folder with the upstream tools, then replace `libssbu_online_deluxe.nro` with the one from [this fork's releases](https://github.com/sn0w-afk/ssbu-online-deluxe-quickplay/releases):

- From the [original mod's releases](https://github.com/saad-script/ssbu-online-deluxe/releases), download `create-sdcard-folder.zip` and run `create-sdcard-folder.bat` (or `create-sdcard-folder.ps1` on Linux via PowerShell). Copy the generated `sdcard/` contents to your SD root (or `eden/sdmc` on emulator).
- Or use the GUI app: [ssbu-emu-optimizer](https://github.com/saad-script/ssbu-emu-optimizer/releases) — click `Generate SDCard Folder` (on emulator, point it at your Eden folder and check `SSBU Settings`, `SSBU Mods`, and optionally `Save Data`), then copy the generated folder to your SD root.

### Verify

Verify that your sdcard directory strucure looks like this on your switch or emulator:

```
`sdcard/` (or `sdmc/` on emulator)
│
├── atmosphere/
│   └── contents/
│       ├── 00FF0000A11CE0FF/
│       │   ├── exefs.nsp
│       │   └── flags/
│       │       └── boot2.flag
│       └── 01006A800016E000/
│           ├── exefs/
│           │   ├── main.npdm
│           │   └── subsdk9
│           └── romfs/
│               └── skyline/
│                   └── plugins/
│                       ├── libarcropolis.nro
│                       ├── libimgui_smash.nro
│                       ├── libnro_hook.nro
│                       ├── libnx_over.nro
│                       ├── libsmashline_plugin.nro
│                       ├── libssbu_online_deluxe.nro
│                       ├── libssbu_pia_manager.nro
│                       └── libssbusync.nro
│
└── ultimate/
    └── ssbu_online_deluxe/
        └── config.toml
```

## 🎮 Controls

### Native UI (Online Character Select Screen and Online Arena)

> Note: The character select screen UI is also available in Quickplay (including Elite Smash).
> Note: `All Shoulder Buttons` = `L + R + Z` on gamecube controller, `ZL + ZR + L + R` on procontroller

- On the character select screen or online arena:
  - `D-pad Left/Right`: Select network latency
  - `D-pad Up/Down`: Select render profile
  - `All Shoulder Buttons + X`: Toggle FPS Boost Mode (AKA FPS++ mode)
  - `All Shoulder Buttons + Y`: Toggle Streamer Mode (show/hide custom native ui)

- On the character select screen (more than one opponent):
  - `Left Trigger + Right Trigger + Dpad Left/Right`: Cycle between which opponent's network info to show

See 'Features' section below to see what these options do

### Overlay UI (Optional)

- `Left Trigger + Right Trigger + D-Pad Down` → Cycle between current window mode
  - Window Modes: `Hidden`, `Full Info`, `Performance Info`
  - In `Full Info Mode`:
    - `D-Pad Up / Down` → Select row
    - `D-Pad Left / Right` → Change value
    - While row `NetProfile` is selected:
      - `All Shoulder Buttons + X`: Toggle FPS Boost mode (AKA FPS++ mode)

See 'Features' section below to see what these options do

## ✨ Features

### 🌐 Online Enhancements

- Display **opponent ping** in all online modes (including Elite Smash):
  - Network RTT (ping) / connection quality
  - Green=Stable, Yellow=Inconsistent, Red=Unstable
- Show **extended opponent info** *(only if both players have the mod)*:
  - Opponent’s current network/render settings (latency slider, render profile)
- 🥷 **Stealth mode** *(fork only)*: never broadcast your extended info — opponents on any mod version see and log nothing, as if you were vanilla — while you can still see theirs. Enabled via `stealth_mode = true` in `config.toml` (on by default in the bundled config).

### 🎛️ Online Latency Controls
*(Available in Online Arena, Quickplay, and Local Online modes)*

- This allows you to control the online latency delay frames.
- Adjust:
  - Latency value:
    - Auto: Applies SSBU's default latency calculation method.
    - 0f-25f: Manually set the latency delay frames

> It is recommended to manually set the latency delay frames based on the ping and connection quality.

### 🎛️ Render Profile Controls
*(Available in Online Arena, Quickplay, and Local Online modes)*

- This allows you to set the games render/graphic settings for less native input delay.
- Adjust:
  - Render Profile:
    - Auto: Applies the recommended profile based on platform (console/emulator) and number of players.
    - Vanilla: This is the default vanilla profile that the game uses by default.
    - LessLag: This applies optimizations to cut 3 frames of native input delay.
    - LLUltra: This applies optimizations to cut 4 frames of native input delay.
      - This also works on console, but the game resolution will be scaled down to keep it stutter free.
      - On console, you may notice that certain UI elements look glitchy, such as the fighter cut-in screen, and match start countdown ui.
    - LLDoubles (Recommended for doubles): This applies optimizations to cut 2 frames of native input delay. This should work even in doubles when there are alot of players on screen without stuttering.
  - FPS Boost mode (AKA FPS++ Mode):
    - Only available on emulators.
    - If enabled, the current profile has '++' at the end of it. For example: LLUltra++
    - The amount of native latency it reduces varies based on the currently selected profile. For example, this will cutoff 3f of delay on Vanilla profile. But on LLUltra, it will only cutoff about half a frame of delay.
    - This may introduce some frametime variance causing the game to not feel as smooth.

**If you arent sure what profile to use, just leave it on Auto**

Best profile for console:
  - LessLag or LLUltra (depending on preference)

Best profile for emulator:
  - LLUltra

Best profile for doubles:
  - LLDoubles

> The mod will apply the **selected render profile automatically** when entering a valid online match.  
> Reverts to **vanilla settings** after exiting  
> You can play offline/training modes without having to worry about timing differences.

### 🎛️ Render Profile Config (Optional)

You can specify a config file in `sd/ultimate/ssbu_online_deluxe/config.toml`
- This will allow you to set the profile to use in the menu, and offline singles/doubles matches
- Add '++' at the end of the profile name to enable fps boost mode (emulator only)
- If you already have an overclock sysmodule, and dont want to conflict with or use ssbu-online-deluxe's built in overclocker:
  - Set `overclocker = false` in config file
  - Delete `libnx_over.nro` plugin file
  - Delete `atmosphere/contents/00FF0000A11CE0FF/` sysmodule folder
  - Restart switch
- All fields are optional. If you dont specify a field, it will use the default/recommended value.
- **Stealth mode**: `stealth_mode` controls the "extended opponent info" broadcast. When `true`, the mod never broadcasts your latency/render-profile info — opponents running any version of the mod see nothing in their overlay and nothing in their log, exactly as if you were a vanilla console — while you can still see *their* extended info. It also suppresses the lower-level mod-detection beacon inside `libssbu_pia_manager.nro` (the `0x45` tag that powers `is_modded` and the `[Wired]`/`[Wifi]` suffix on opponents' screens), so a stealth console shows no interface suffix at all, same as vanilla. The `config.toml` bundled with this fork's releases ships with `stealth_mode = true`. Set it to `false` if you want two-way extended info sharing. (Code default when the key is absent: `false`.)

Example `config.toml`:
```
overclocker = true                          # Set to 'false' if you are using your own overclock sysmodule
stealth_mode = true                         # 'true' = never broadcast extended info; 'false' = share with modded opponents

[render_profile_config]
menu = "Vanilla"                            # Recommended to keep this on Vanilla always
offline_match.singles = "Vanilla"           # Applies to offline single matches (1 or 2 players)
offline_match.doubles = "Vanilla"           # Applies to offline doubles matches (more than 2 players)
online_match.singles = "LessLagUltra++"     # 'Auto' mode will choose this profile for online single matches
online_match.doubles = "LessLag"            # 'Auto' mode will choose this profile for online double matches
```

## 📝 Notes and Contribution

- The dynamic resolution logic currently only applies to zoom in moves (final hit/critical hit) and Sephiroth's gigaflare.  
- Contributions are open especially for applying dynamic resolution to moves that cause stutter. I don't know if I'll have time to optimize every single move, so if you notice a specific move causes stutters, you can use smashline's api to contribute and optimize the move. You can start by viewing how `src/perf_scaler` currently applies dynamic resolution optimization.

## 🙌 Credits

Huge thanks to the following people who made this possible. Without these people, this project wouldn't have been possible:

- **Bludev**
  For SSBU render system research and the initial less-lag and latency slider mod.

- **BlankMauser**
  Creator of the SsbuSync and smash-ultelier mod, which this mod uses to modify ssbu's render system.
  BlanksMauser's work and guidance on SSBU’s rendering internals were critical to making this mod possible.

- **Kinnay** & contributors of the NintendoClients repo/wiki
  For guidance on network service implementation. The network service wouldn't have been possible if not for the incredible efforts of these people.

- **Coolsonickirby**
  For the imgui-smash plugin, making UI development significantly easier

- **The HDR team**
  For smashline, allowing for easy figher/effect/moveset hooks and adjustments 

- **The developers of Skyline**
  For the modding environment, allowing for code hooking/edits
