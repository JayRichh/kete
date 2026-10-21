//! The window's WebView2, quieted: no browser right-click menu (Back,
//! Refresh, Save as, Print, Inspect) and, in release builds, no browser
//! keys (reload, print, find, zoom, devtools, back and forward). Kete
//! draws its own menus instead (desktop/src/lib/menus): Copy and Paste for
//! fields, the terminal and the diff viewer, and every row's verbs.
//!
//! WebView2 keeps the editing keys (Ctrl+C, V, X, A, Z) with the browser
//! keys off, and every key it no longer takes reaches the page: Alt+Left
//! and Alt+Right are Kete's back and forward, Ctrl+R, F and P go to the
//! shell in a terminal. Zoom keys were already off (Tauri's
//! `zoomHotkeysEnabled` defaults to false).
//!
//! Paste reads the clipboard with `navigator.clipboard.readText()`, which
//! WebView2 asks permission for. The grant here is the one Tauri's
//! `enable_clipboard_access` makes for a window it builds, held to this
//! app's own pages.
//!
//! Tauri doesn't pass these settings to wry, so they are set on the live
//! webview, and WebView2 applies a setting changed after a navigation has
//! started from the next one. The app's first navigation is under way by
//! the time the window exists (`settle`): it is restarted under the
//! settings when it already shows, and reloaded once it has loaded only
//! when it had started before them. A bare reload while it is still
//! pending would reload the blank page before it and drop the app.

/// The arguments wry gives a WebView2 window that names none.
#[cfg(debug_assertions)]
const WRY_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// A debug build driven by msedgedriver (the e2e suite): its browser
/// arguments, `--remote-debugging-port=0` among them, on every window.
/// WebView2 takes the app's own arguments, which wry always sets, over the
/// `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` msedgedriver sets, so without
/// this the driver never finds the window's DevTools port.
#[cfg(debug_assertions)]
pub fn take_driver_args(context: &mut tauri::Context<tauri::Wry>) {
    let Ok(more) = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") else {
        return;
    };
    for w in &mut context.config_mut().app.windows {
        let own = w.additional_browser_args.as_deref().unwrap_or(WRY_ARGS);
        w.additional_browser_args = Some(merge_browser_args(own, &more));
    }
}

/// `own` then `more`, each argument once, with one `--disable-features=`
/// holding both lists: Chromium reads only the last of that switch.
#[cfg(debug_assertions)]
fn merge_browser_args(own: &str, more: &str) -> String {
    const OFF: &str = "--disable-features=";
    let mut features: Vec<&str> = Vec::new();
    let mut rest: Vec<&str> = Vec::new();
    for arg in own.split_whitespace().chain(more.split_whitespace()) {
        let (list, items) = match arg.strip_prefix(OFF) {
            Some(names) => (&mut features, names.split(',').collect::<Vec<_>>()),
            None => (&mut rest, vec![arg]),
        };
        for item in items.into_iter().filter(|i| !i.is_empty()) {
            if !list.contains(&item) {
                list.push(item);
            }
        }
    }
    let off = (!features.is_empty()).then(|| format!("{OFF}{}", features.join(",")));
    off.into_iter()
        .chain(rest.into_iter().map(String::from))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether WebView2's browser keys stay on: in debug builds (F12 opens the
/// devtools there, F5 reloads), unless `KETE_BROWSER_KEYS=off` asks for the
/// release behaviour (the e2e keyboard flow).
pub fn browser_keys(debug: bool, env: Option<&str>) -> bool {
    debug && !matches!(env, Some(v) if v.eq_ignore_ascii_case("off"))
}

/// A page of this app: the bundled one (`http(s)://tauri.localhost`) or the
/// dev server's (`http://localhost:<port>`). Nothing else gets the clipboard.
pub fn own_page(uri: &str) -> bool {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return false;
    };
    if scheme != "http" && scheme != "https" && scheme != "tauri" {
        return false;
    }
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = match host_port.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host_port,
    };
    matches!(host, "tauri.localhost" | "localhost" | "127.0.0.1")
}

/// What the first navigation needs, from the webview's current source:
/// restarted at once when the app's page is already its document (or on
/// its way), left to finish when the source is still the blank page the
/// webview starts on (the handlers in `settle` decide then).
pub fn restart_now(source: &str) -> bool {
    !source.is_empty() && source != "about:blank"
}

/// Quiet `w`'s webview. Debug builds can hold this back
/// (`KETE_QUIET_DELAY_MS`), so the e2e suite sees the late path work.
pub fn quiet(w: &tauri::WebviewWindow) {
    let delay = cfg!(debug_assertions)
        .then(|| {
            std::env::var("KETE_QUIET_DELAY_MS")
                .ok()?
                .parse::<u64>()
                .ok()
        })
        .flatten();
    match delay {
        Some(ms) => {
            let w = w.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(ms));
                apply(&w);
            });
        }
        None => apply(w),
    }
}

fn apply(w: &tauri::WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Settings3, COREWEBVIEW2_PERMISSION_KIND,
        COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
    };
    use webview2_com::PermissionRequestedEventHandler;
    use windows::core::{Interface, PWSTR};

    let keys = browser_keys(
        cfg!(debug_assertions),
        std::env::var("KETE_BROWSER_KEYS").ok().as_deref(),
    );
    let _ = w.with_webview(move |pw| unsafe {
        let Ok(core) = pw.controller().CoreWebView2() else {
            return;
        };
        let Ok(settings) = core.Settings() else {
            return;
        };
        let _ = settings.SetAreDefaultContextMenusEnabled(false);
        if !keys {
            if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
                let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
            }
        }
        let grant = PermissionRequestedEventHandler::create(Box::new(|_, args| {
            let Some(args) = args else {
                return Ok(());
            };
            let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
            args.PermissionKind(&mut kind)?;
            if kind != COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ {
                return Ok(());
            }
            let mut uri = PWSTR::null();
            args.Uri(&mut uri)?;
            if own_page(&webview2_com::take_pwstr(uri)) {
                args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
            }
            Ok(())
        }));
        let mut token = 0i64;
        let _ = core.add_PermissionRequested(&grant, &mut token);
        let _ = settle(&core);
    });
}

/// Make the settings hold for the app's page (see the module docs).
unsafe fn settle(
    core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
) -> windows::core::Result<()> {
    use std::cell::Cell;
    use std::rc::Rc;
    use webview2_com::{NavigationCompletedEventHandler, NavigationStartingEventHandler};
    use windows::core::{HSTRING, PWSTR};

    let mut source = PWSTR::null();
    core.Source(&mut source)?;
    let source = webview2_com::take_pwstr(source);
    if restart_now(&source) {
        eprintln!("kete-desktop: webview settings set, restarting {source}");
        return core.Navigate(&HSTRING::from(source.as_str()));
    }
    // Still on the blank page: if the app's navigation starts from here,
    // it starts under the settings; if it had already started, reload it
    // once it has loaded.
    let started = Rc::new(Cell::new(false));
    let seen = started.clone();
    let mut token = 0i64;
    core.add_NavigationStarting(
        &NavigationStartingEventHandler::create(Box::new(move |_, _| {
            seen.set(true);
            Ok(())
        })),
        &mut token,
    )?;
    let done = Cell::new(false);
    core.add_NavigationCompleted(
        &NavigationCompletedEventHandler::create(Box::new(move |sender, _| {
            if done.replace(true) {
                return Ok(());
            }
            if started.get() {
                eprintln!("kete-desktop: webview settings set before the first navigation");
                return Ok(());
            }
            eprintln!("kete-desktop: webview settings set late, reloading once");
            match sender {
                Some(core) => core.Reload(),
                None => Ok(()),
            }
        })),
        &mut token,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_keys_only_in_debug_and_off_on_request() {
        assert!(browser_keys(true, None));
        assert!(browser_keys(true, Some("on")));
        assert!(!browser_keys(true, Some("off")));
        assert!(!browser_keys(true, Some("OFF")));
        assert!(!browser_keys(false, None));
        assert!(!browser_keys(false, Some("on")));
    }

    #[test]
    fn the_drivers_arguments_join_wrys_with_one_disable_features() {
        let driver = "--disable-features=IgnoreDuplicateNavs,Prewarm --enable-automation --remote-debugging-port=0";
        assert_eq!(
            merge_browser_args(WRY_ARGS, driver),
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,IgnoreDuplicateNavs,Prewarm \
             --enable-automation --remote-debugging-port=0"
        );
        assert_eq!(merge_browser_args("--a --a", ""), "--a");
        assert_eq!(merge_browser_args("", "--disable-features="), "");
    }

    #[test]
    fn restarts_only_once_the_app_page_is_the_source() {
        assert!(restart_now("http://tauri.localhost/"));
        assert!(restart_now("http://localhost:1420/"));
        assert!(!restart_now("about:blank"));
        assert!(!restart_now(""));
    }

    #[test]
    fn own_page_is_the_bundle_or_the_dev_server() {
        assert!(own_page("http://tauri.localhost/"));
        assert!(own_page("https://tauri.localhost/index.html"));
        assert!(own_page("http://localhost:1420/"));
        assert!(own_page("http://127.0.0.1:1420"));
        assert!(own_page("tauri://localhost"));
        assert!(!own_page("https://example.com/"));
        assert!(!own_page("https://tauri.localhost.example.com/"));
        assert!(!own_page("http://localhost.evil:80/"));
        assert!(!own_page("file:///C:/x.html"));
        assert!(!own_page("about:blank"));
        assert!(!own_page(""));
    }
}
