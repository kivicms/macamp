mod app;
mod audio;
mod config;
mod drivemusic;
mod metadata;
mod platform;
mod spectrum;
mod theme;
mod widgets;

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use theme::{DRIVE_H, DRIVE_W, EQ_H, EQ_W, MAIN_H, MAIN_W, PIXEL_FAMILY, SHADE_H};

use crate::app::{MacAmpApp, StartupOptions};
use crate::config::Config;

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "macamp_silkscreen".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Silkscreen-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "macamp_silkscreen_bold".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Silkscreen-Bold.ttf"
        ))),
    );

    let fallback = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut pixel = vec!["macamp_silkscreen".to_owned()];
    pixel.extend(fallback.iter().cloned());
    fonts
        .families
        .insert(egui::FontFamily::Name(PIXEL_FAMILY.into()), pixel);
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result {
    let _log_guard = config::init_logging();
    if let Err(error) = config::ensure_dirs() {
        tracing::warn!("could not create app directories: {error}");
    }
    tracing::info!("MacAmp starting; data at {}", config::config_dir_display());

    let config = Config::load();
    let initial_paths: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .filter_map(|arg| {
            let path = PathBuf::from(arg);
            if path.exists() {
                Some(path)
            } else {
                tracing::warn!("ignoring missing path argument: {}", path.display());
                None
            }
        })
        .collect();
    let screenshot = std::env::var_os("MACAMP_SCREENSHOT").map(PathBuf::from);
    let autoplay = std::env::var_os("MACAMP_AUTOPLAY").is_some() || !initial_paths.is_empty();
    let playlist_root = std::env::var_os("MACAMP_PLAYLIST_ROOT").is_some();
    let eq_root = std::env::var_os("MACAMP_EQ_ROOT").is_some();
    let drive_root = std::env::var_os("MACAMP_DRIVE_ROOT").is_some();
    let (window_w, window_h) = if playlist_root {
        (config.playlist_size[0], config.playlist_size[1])
    } else if drive_root {
        (DRIVE_W, DRIVE_H)
    } else if eq_root {
        (EQ_W, EQ_H)
    } else if config.shaded {
        (MAIN_W, SHADE_H)
    } else {
        (MAIN_W, MAIN_H)
    };
    let main_position = if screenshot.is_some() {
        Some(egui::pos2(80.0, 80.0))
    } else if config.double_size {
        // Positions are stored in egui points at the active zoom factor. The
        // zoom is applied on the first app frame, so restoring now would move
        // the window to the wrong physical spot. The app restores it instead.
        None
    } else {
        config.main_pos.map(|[x, y]| egui::pos2(x, y))
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("MacAmp")
        .with_decorations(false)
        .with_resizable(false)
        .with_inner_size([window_w, window_h])
        .with_min_inner_size([window_w, window_h])
        .with_max_inner_size([window_w, window_h]);
    if let Ok(image) = image::load_from_memory(include_bytes!("../assets/icon.png")) {
        let rgba = image.to_rgba8();
        let (width, height) = rgba.dimensions();
        viewport = viewport.with_icon(egui::IconData {
            rgba: rgba.into_raw(),
            width,
            height,
        });
    }
    if let Some(position) = main_position {
        viewport = viewport.with_position(position);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        centered: main_position.is_none(),
        ..Default::default()
    };

    eframe::run_native(
        "MacAmp",
        native_options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            let mut visuals = egui::Visuals::dark();
            visuals.window_fill = egui::Color32::from_rgb(0x0b, 0x0d, 0x0b);
            visuals.panel_fill = egui::Color32::from_rgb(0x0b, 0x0d, 0x0b);
            cc.egui_ctx.set_visuals(visuals);
            let app = MacAmpApp::new(
                cc,
                StartupOptions {
                    config,
                    initial_paths,
                    autoplay,
                    screenshot,
                    playlist_root,
                    eq_root,
                    drive_root,
                },
            );
            Ok(Box::new(app))
        }),
    )
}
