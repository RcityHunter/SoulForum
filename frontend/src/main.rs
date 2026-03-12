mod api;
mod app;
mod components;
mod pages;
mod services;
mod state;
mod style;

fn main() {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&"btc-forum-frontend: main() start".into());
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();
    dioxus::prelude::launch(app::app);
}
