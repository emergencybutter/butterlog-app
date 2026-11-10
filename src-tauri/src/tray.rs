//! The tray icon. A finished flight puts a dot on it: a click then opens that flight's
//! debrief on the web and clears the dot; any other click opens the window. Its menu
//! always offers both, the window and the service in the browser.

use parking_lot::Mutex;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "main";
const TOOLTIP: &str = "ButterLog";

/// Managed state: the two icons and the menu entries that change.
pub struct Tray {
    plain: Image<'static>,
    dotted: Image<'static>,
    debrief: MenuItem<Wry>,
    dot: Mutex<bool>,
}

/// The icon with a notification dot in its top right corner.
fn with_dot(icon: &Image<'_>) -> Image<'static> {
    let (w, h) = (icon.width(), icon.height());
    let mut rgba = icon.rgba().to_vec();
    let r = w.min(h) as f64 * 0.24;
    let (cx, cy) = (w as f64 - r - 0.5, r + 0.5);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
            let px = ((y * w + x) * 4) as usize;
            let color: Option<[u8; 3]> = if d <= r - 1.5 {
                Some([0xff, 0x9f, 0x1a]) // amber
            } else if d <= r {
                Some([0x10, 0x12, 0x14]) // a dark ring, so it reads on any taskbar
            } else {
                None
            };
            if let Some([cr, cg, cb]) = color {
                rgba[px..px + 4].copy_from_slice(&[cr, cg, cb, 0xff]);
            }
        }
    }
    Image::new_owned(rgba, w, h)
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Open ButterLog", true, None::<&str>)?;
    let debrief = MenuItem::with_id(app, "debrief", "Last flight's debrief", false, None::<&str>)?;
    let web = MenuItem::with_id(app, "web", "My flights on the web", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &web, &debrief, &separator, &quit])?;

    let plain = Image::from_bytes(include_bytes!("../../public/icon.png"))?.to_owned();
    let dotted = with_dot(&plain);

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(plain.clone())
        .tooltip(TOOLTIP)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "show" => crate::show_main_window(app),
            "web" => open(app, "flights"),
            "debrief" => open(app, "debrief"),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                clicked(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(Tray { plain, dotted, debrief, dot: Mutex::new(false) });
    Ok(())
}

fn open(app: &AppHandle, page: &str) {
    if let Some(url) = crate::service::page_url(app, page) {
        if let Err(e) = crate::service::open_url(app, &url) {
            crate::append_log(app, format!("[Tray] Couldn't open {url}: {e}"));
        }
    }
}

/// A left click: the flight just done when there is one, the window otherwise.
fn clicked(app: &AppHandle) {
    let dot = app.try_state::<Tray>().is_some_and(|t| *t.dot.lock());
    if dot {
        open(app, "debrief");
    } else {
        crate::show_main_window(app);
    }
}

/// Shows the dot for a flight just done, or clears it.
pub fn flight_done(app: &AppHandle, flight: Option<&crate::service::FlightRef>) {
    let (Some(tray), Some(icon)) = (app.try_state::<Tray>(), app.tray_by_id(TRAY_ID)) else {
        return;
    };
    let mut dot = tray.dot.lock();
    if *dot == flight.is_some() {
        return;
    }
    *dot = flight.is_some();
    let _ = icon.set_icon(Some(if *dot { tray.dotted.clone() } else { tray.plain.clone() }));
    let tip = match flight {
        Some(f) => format!("{TOOLTIP}: flight done, {}. Click for the debrief.", f.route()),
        None => TOOLTIP.to_string(),
    };
    let _ = icon.set_tooltip(Some(tip));
}

/// Names the last flight in the menu, which opens its debrief.
pub fn last_flight(app: &AppHandle, flight: Option<&crate::service::FlightRef>) {
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    let text = match flight {
        Some(f) => format!("Debrief of {}", f.route()),
        None => "Last flight's debrief".to_string(),
    };
    let _ = tray.debrief.set_text(text);
    let _ = tray.debrief.set_enabled(flight.is_some());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dot_sits_top_right_and_leaves_the_rest() {
        let icon = Image::new_owned(vec![0; 32 * 32 * 4], 32, 32);
        let dotted = with_dot(&icon);
        let at = |x: u32, y: u32| {
            let i = ((y * 32 + x) * 4) as usize;
            dotted.rgba()[i..i + 4].to_vec()
        };
        assert_eq!(at(25, 6), [0xff, 0x9f, 0x1a, 0xff]);
        assert_eq!(at(4, 28), [0, 0, 0, 0]);
    }
}
