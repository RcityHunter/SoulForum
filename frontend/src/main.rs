mod api;
mod app;
mod components;
mod pages;
mod services;
mod state;
mod style;

#[cfg(target_arch = "wasm32")]
fn install_favicon() {
    use wasm_bindgen::JsCast;

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Some(head) = document.head() else {
        return;
    };

    if document
        .query_selector("link[rel='icon']")
        .ok()
        .flatten()
        .is_some()
    {
        return;
    }

    let Ok(link) = document.create_element("link") else {
        return;
    };
    let _ = link.set_attribute("rel", "icon");
    let _ = link.set_attribute("type", "image/svg+xml");
    let _ = link.set_attribute("href", "/forum/favicon.svg");
    let _ = head.append_child(&link.unchecked_into());
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&"btc-forum-frontend: main() start".into());
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();
    #[cfg(target_arch = "wasm32")]
    install_favicon();
    dioxus::prelude::launch(app::app);
}
