//! Startup window sizing.
//!
//! The app has one shape — the rail + stage workbench — sized from
//! [`DEFAULT_SIZE`], but never larger than the monitor can show: on a small or
//! scaled display the work area wins, because a window whose edges sit off-screen
//! is worse than a slightly cramped one.
//!
//! Sizing happens while the window is still hidden, and the window is only shown
//! once the frontend has painted. That is what keeps startup from flashing an
//! empty dark frame and then visibly reshaping it.

use tauri::{LogicalSize, Manager, PhysicalPosition, WebviewWindow};

use crate::error::{Error, Result};

pub const DEFAULT_SIZE: (f64, f64) = (980.0, 660.0);
pub const MIN_SIZE: (f64, f64) = (860.0, 580.0);

/// Axis-aligned rectangle in physical pixels, mirroring the monitor work area
/// and the window's outer frame closely enough for the placement math below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Requested size normalized against the minimum and the work area (all logical
/// px). An oversized request shrinks to the work area; an undersized one grows to
/// the minimum. When the work area itself is smaller than the minimum, the work
/// area wins — a fully reachable window beats a "big enough" one whose edges sit
/// off-screen.
pub fn normalize_size(
    width: Option<f64>,
    height: Option<f64>,
    work_logical: (f64, f64),
) -> (f64, f64) {
    let clamp_axis = |requested: Option<f64>, default: f64, min: f64, work: f64| -> f64 {
        let value = requested
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(default);
        let floor = min.min(work.max(1.0));
        value.clamp(floor, work.max(floor))
    };
    (
        clamp_axis(width, DEFAULT_SIZE.0, MIN_SIZE.0, work_logical.0),
        clamp_axis(height, DEFAULT_SIZE.1, MIN_SIZE.1, work_logical.1),
    )
}

/// Top-left position (physical px) that keeps `prev`'s center for a window of
/// `target_w`×`target_h`, pulled back inside `work`. Right/bottom clamp first,
/// then left/top, so an oversized window pins to the work area's origin and its
/// drag region stays reachable.
pub fn placement(prev: Rect, target_w: f64, target_h: f64, work: Rect) -> (f64, f64) {
    let center_x = prev.x + prev.w / 2.0;
    let center_y = prev.y + prev.h / 2.0;
    let x = (center_x - target_w / 2.0)
        .min(work.x + work.w - target_w)
        .max(work.x);
    let y = (center_y - target_h / 2.0)
        .min(work.y + work.h - target_h)
        .max(work.y);
    (x, y)
}

/// Fit the main window to the monitor it opens on. Leaves it hidden; the
/// frontend shows it once it has something to show.
pub fn apply_startup_size(window: &WebviewWindow) -> Result<()> {
    let win_err = |op: &str, e: tauri::Error| Error::msg(format!("window {op}: {e}"));
    let scale = window.scale_factor().unwrap_or(1.0).max(0.1);

    // The configured frame, already centered by the window manager: its center
    // is what the resize below preserves.
    let prev_pos = window
        .outer_position()
        .map_err(|e| win_err("position", e))?;
    let prev_size = window.outer_size().map_err(|e| win_err("size", e))?;
    let prev = Rect {
        x: prev_pos.x as f64,
        y: prev_pos.y as f64,
        w: prev_size.width as f64,
        h: prev_size.height as f64,
    };

    // Whichever monitor hosts the window, falling back to the primary one. With
    // neither there is nothing to measure against, so the configured size stands.
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return Ok(());
    };
    let area = monitor.work_area();
    let work = Rect {
        x: area.position.x as f64,
        y: area.position.y as f64,
        w: area.size.width as f64,
        h: area.size.height as f64,
    };

    let (logical_w, logical_h) = normalize_size(None, None, (work.w / scale, work.h / scale));

    window
        .set_size(LogicalSize::new(logical_w, logical_h))
        .map_err(|e| win_err("resize", e))?;
    // On a work area smaller than the nominal minimum the applied size already
    // shrank below it — the floor has to follow, or the user could never drag the
    // window back inside the screen.
    window
        .set_min_size(Some(LogicalSize::new(
            MIN_SIZE.0.min(logical_w),
            MIN_SIZE.1.min(logical_h),
        )))
        .map_err(|e| win_err("min size", e))?;

    let (x, y) = placement(prev, logical_w * scale, logical_h * scale, work);
    window
        .set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
        .map_err(|e| win_err("reposition", e))?;

    Ok(())
}

/// Reveal the main window, already sized. The frontend calls this once it has
/// painted, which is what keeps startup from showing an empty frame first.
#[tauri::command]
pub fn show_main_window(app: tauri::AppHandle) -> Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| Error::msg("main window unavailable"))?;
    let win_err = |op: &str, e: tauri::Error| Error::msg(format!("window {op}: {e}"));
    window.show().map_err(|e| win_err("show", e))?;
    window.set_focus().map_err(|e| win_err("focus", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect {
        x: 0.0,
        y: 25.0,
        w: 1728.0,
        h: 1052.0,
    };

    #[test]
    fn placement_keeps_center_when_it_fits() {
        // 400×640 window centered at (864, 551) growing to 1100×720.
        let prev = Rect {
            x: 664.0,
            y: 231.0,
            w: 400.0,
            h: 640.0,
        };
        assert_eq!(placement(prev, 1100.0, 720.0, WORK), (314.0, 191.0));
    }

    #[test]
    fn placement_clamps_into_work_area() {
        // Window hugging the bottom-right corner must not expand off-screen.
        let prev = Rect {
            x: 1320.0,
            y: 430.0,
            w: 400.0,
            h: 640.0,
        };
        assert_eq!(placement(prev, 1100.0, 720.0, WORK), (628.0, 357.0));
    }

    #[test]
    fn placement_pins_origin_when_oversized() {
        // Target wider than the work area: the left/top edge wins so the drag
        // region stays reachable.
        let prev = Rect {
            x: 0.0,
            y: 25.0,
            w: 1728.0,
            h: 1052.0,
        };
        assert_eq!(placement(prev, 2000.0, 1200.0, WORK), (WORK.x, WORK.y));
    }

    #[test]
    fn size_defaults_and_clamps() {
        // Defaults when unspecified.
        assert_eq!(
            normalize_size(None, None, (1728.0, 1052.0)),
            DEFAULT_SIZE
        );
        // Oversized request shrinks to the work area.
        assert_eq!(
            normalize_size(Some(3000.0), Some(2000.0), (1728.0, 1052.0)),
            (1728.0, 1052.0)
        );
        // Undersized / nonsense requests grow to the minimum.
        assert_eq!(
            normalize_size(Some(100.0), Some(f64::NAN), (1728.0, 1052.0)),
            (MIN_SIZE.0, DEFAULT_SIZE.1)
        );
        // A work area smaller than the minimum wins over the minimum: the
        // window must stay fully reachable (1366×768 laptop at 125% scale).
        assert_eq!(
            normalize_size(None, None, (1092.8, 582.4)),
            (DEFAULT_SIZE.0, 582.4)
        );
    }
}
