use std::sync::OnceLock;

use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Runtime,
};

use crate::models::LiveRecordingStatus;

const TRAY_ID: &str = "recording-status";

static RECORDING_ICON: OnceLock<(Vec<u8>, u32, u32)> = OnceLock::new();

pub fn initialize<R: Runtime>(app: &AppHandle<R>) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Show Transcribe Kit", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::new(app)?;
    menu.append(&show)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&quit)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(idle_icon())
        .tooltip("Transcribe Kit")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

pub fn set_recording<R: Runtime>(app: &AppHandle<R>, status: &LiveRecordingStatus) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let tooltip = match &status.input_device_label {
        Some(label) => format!("Transcribe Kit: recording from {label}"),
        None => "Transcribe Kit: recording".to_string(),
    };

    let _ = tray.set_icon(Some(recording_icon()));
    let _ = tray.set_tooltip(Some(&tooltip));

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let _ = tray.set_title(Some("● REC"));
    }
}

pub fn set_idle<R: Runtime>(app: &AppHandle<R>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let _ = tray.set_icon(Some(idle_icon()));
    let _ = tray.set_tooltip(Some("Transcribe Kit"));

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let _ = tray.set_title(Some(""));
    }
}

fn idle_icon() -> Image<'static> {
    tauri::include_image!("icons/icon.png")
}

fn recording_icon() -> Image<'static> {
    let (rgba, w, h) = RECORDING_ICON.get_or_init(|| {
        let base = idle_icon();
        let overlay = overlay_recording_badge(base.rgba(), base.width(), base.height());
        (overlay, base.width(), base.height())
    });
    Image::new(rgba.as_slice(), *w, *h)
}

/// Composites a red circle with a white ring and dark outer stroke onto the
/// bottom-right corner of the base icon, producing a "recording active" badge.
///
/// Three concentric zones: red fill → white ring → dark outline, with
/// anti-aliased outer edge. The dark outline ensures the badge is visible
/// even when the icon background is white.
fn overlay_recording_badge(base_rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rgba = base_rgba.to_vec();
    let size = width.min(height) as f64;

    let badge_r = (size * 0.20).round();
    let ring_w = (size * 0.035).round().max(2.0);
    let shadow_w = (size * 0.02).round().max(1.0);
    let margin = (size * 0.04).round();
    let aa = 1.5_f64;

    let total_r = badge_r + shadow_w;
    let cx = width as f64 - margin - total_r;
    let cy = height as f64 - margin - total_r;
    let inner_r = badge_r - ring_w;

    let x0 = (cx - total_r - aa).floor().max(0.0) as u32;
    let x1 = (cx + total_r + aa).ceil().min(width as f64 - 1.0) as u32;
    let y0 = (cy - total_r - aa).floor().max(0.0) as u32;
    let y1 = (cy + total_r + aa).ceil().min(height as f64 - 1.0) as u32;

    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist > total_r + aa {
                continue;
            }

            let idx = ((y * width + x) * 4) as usize;

            let (r, g, b) = if dist <= inner_r {
                (220u8, 38u8, 38u8)
            } else if dist <= badge_r {
                (255u8, 255u8, 255u8)
            } else {
                (60u8, 60u8, 60u8)
            };

            if dist <= total_r {
                rgba[idx] = r;
                rgba[idx + 1] = g;
                rgba[idx + 2] = b;
                rgba[idx + 3] = 255;
            } else {
                let fg_a = ((total_r + aa - dist) / aa).clamp(0.0, 1.0);
                alpha_composite(&mut rgba[idx..idx + 4], r, g, b, fg_a);
            }
        }
    }

    rgba
}

fn alpha_composite(dst: &mut [u8], r: u8, g: u8, b: u8, fg_alpha: f64) {
    let bg_a = dst[3] as f64 / 255.0;
    let out_a = fg_alpha + bg_a * (1.0 - fg_alpha);
    if out_a <= 0.0 {
        return;
    }
    dst[0] = ((r as f64 * fg_alpha + dst[0] as f64 * bg_a * (1.0 - fg_alpha)) / out_a) as u8;
    dst[1] = ((g as f64 * fg_alpha + dst[1] as f64 * bg_a * (1.0 - fg_alpha)) / out_a) as u8;
    dst[2] = ((b as f64 * fg_alpha + dst[2] as f64 * bg_a * (1.0 - fg_alpha)) / out_a) as u8;
    dst[3] = (out_a * 255.0) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn badge_center(w: u32, h: u32) -> (u32, u32) {
        let size = w.min(h) as f64;
        let badge_r = (size * 0.20).round();
        let shadow_w = (size * 0.02).round().max(1.0);
        let total_r = badge_r + shadow_w;
        let margin = (size * 0.04).round();
        let cx = (w as f64 - margin - total_r).round() as u32;
        let cy = (h as f64 - margin - total_r).round() as u32;
        (cx, cy)
    }

    #[test]
    fn overlay_preserves_dimensions() {
        let w = 64u32;
        let h = 64u32;
        let base = vec![128u8; (w * h * 4) as usize];
        let result = overlay_recording_badge(&base, w, h);
        assert_eq!(result.len(), base.len());
    }

    #[test]
    fn overlay_top_left_is_unchanged() {
        let w = 64u32;
        let h = 64u32;
        let base = vec![42u8; (w * h * 4) as usize];
        let result = overlay_recording_badge(&base, w, h);
        let idx = ((2 * w + 2) * 4) as usize;
        assert_eq!(&result[idx..idx + 4], &[42, 42, 42, 42]);
    }

    #[test]
    fn overlay_badge_center_is_red() {
        let w = 128u32;
        let h = 128u32;
        let base = vec![200u8; (w * h * 4) as usize];
        let result = overlay_recording_badge(&base, w, h);

        let (cx, cy) = badge_center(w, h);
        let idx = ((cy * w + cx) * 4) as usize;
        assert_eq!(result[idx], 220);
        assert_eq!(result[idx + 1], 38);
        assert_eq!(result[idx + 2], 38);
        assert_eq!(result[idx + 3], 255);
    }

    #[test]
    fn overlay_has_dark_outer_stroke() {
        let w = 256u32;
        let h = 256u32;
        let base = vec![255u8; (w * h * 4) as usize];
        let result = overlay_recording_badge(&base, w, h);

        let size = w.min(h) as f64;
        let badge_r = (size * 0.20).round();
        let shadow_w = (size * 0.02).round().max(1.0);
        let total_r = badge_r + shadow_w;
        let margin = (size * 0.04).round();
        let cx = w as f64 - margin - total_r;
        let cy = h as f64 - margin - total_r;

        let sample_dist = badge_r + shadow_w / 2.0;
        let sx = (cx + sample_dist).round() as u32;
        let sy = cy.round() as u32;
        let idx = ((sy * w + sx) * 4) as usize;
        assert_eq!(result[idx], 60, "dark stroke R");
        assert_eq!(result[idx + 1], 60, "dark stroke G");
        assert_eq!(result[idx + 2], 60, "dark stroke B");
        assert_eq!(result[idx + 3], 255, "dark stroke A");
    }

    #[test]
    fn alpha_composite_opaque_foreground_replaces() {
        let mut dst = [100, 150, 200, 255];
        alpha_composite(&mut dst, 220, 38, 38, 1.0);
        assert_eq!(dst, [220, 38, 38, 255]);
    }

    #[test]
    fn alpha_composite_transparent_foreground_preserves() {
        let mut dst = [100, 150, 200, 255];
        alpha_composite(&mut dst, 220, 38, 38, 0.0);
        assert_eq!(dst, [100, 150, 200, 255]);
    }

    #[test]
    fn alpha_composite_half_alpha_blends() {
        let mut dst = [0, 0, 0, 255];
        alpha_composite(&mut dst, 255, 255, 255, 0.5);
        assert!(
            dst[0] > 120 && dst[0] < 135,
            "R should be ~127, got {}",
            dst[0]
        );
        assert_eq!(dst[3], 255, "output should be fully opaque");
    }
}
