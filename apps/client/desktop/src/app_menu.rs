//! The native app menu.

use tauri::menu::{Menu, MenuBuilder, SubmenuBuilder};
use tauri::{Manager, Runtime};

#[cfg(not(target_os = "macos"))]
use crate::CLOSE_WINDOW_MENU_ID;

pub(crate) fn build_app_menu<M: Manager<R>, R: Runtime>(manager: &M) -> tauri::Result<Menu<R>> {
    // Devtools are not a native menu item; they are toggled from the frontend
    // (Cmd/Ctrl+Alt+I) gated by the "Developer tools" setting via the
    // `toggle_devtools` command, so they can be flipped on/off in one build.

    // macOS: build the standard App / Edit / Window submenus out of predefined
    // items. Predefined items map to native AppKit selectors (`performClose:`,
    // `copy:`, …) dispatched through the responder chain, so their key
    // equivalents fire even while the WKWebView holds focus. A custom MenuItem
    // accelerator (the route used on other platforms below) is swallowed by the
    // focused webview, which is why the standard shortcuts need the predefined
    // items. `close_window` -> `performClose:` closes the focused window for
    // all windows uniformly.
    #[cfg(target_os = "macos")]
    {
        let app_menu = SubmenuBuilder::new(manager, manager.package_info().name.clone())
            .about(None)
            .separator()
            .services()
            .separator()
            .hide()
            .hide_others()
            .show_all()
            .separator()
            .quit()
            .build()?;
        let edit_menu = SubmenuBuilder::new(manager, "Edit")
            .undo()
            .redo()
            .separator()
            .cut()
            .copy()
            .paste()
            .select_all()
            .build()?;
        let window_menu = SubmenuBuilder::new(manager, "Window")
            .minimize()
            .maximize()
            .separator()
            .close_window()
            .build()?;

        let builder = MenuBuilder::new(manager).item(&app_menu).item(&edit_menu);
        let builder = builder.item(&window_menu);
        return builder.build();
    }

    // Other platforms keep the custom Close Window item: their webviews do not
    // intercept the accelerator the way the macOS WKWebView does, and the
    // predefined close item is macOS-only.
    #[cfg(not(target_os = "macos"))]
    {
        let close_window = tauri::menu::MenuItem::with_id(
            manager,
            CLOSE_WINDOW_MENU_ID,
            "Close Window",
            true,
            Some("CmdOrCtrl+W"),
        )?;
        let file_menu = SubmenuBuilder::new(manager, "File")
            .item(&close_window)
            .build()?;
        let builder = MenuBuilder::new(manager).item(&file_menu);
        builder.build()
    }
}
