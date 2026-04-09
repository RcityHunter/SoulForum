use dioxus::prelude::*;
use gloo_timers::future::TimeoutFuture;
use js_sys::Date;
use web_sys::wasm_bindgen::JsValue;

const APP_BASE_PATH: &str = "/forum";

#[derive(Props, Clone, PartialEq)]
pub struct TopNavProps {
    pub is_admin: bool,
    pub is_register: bool,
    pub is_login: bool,
    pub is_logged_in: bool,
    pub welcome_text: String,
    pub blog_link: String,
    pub docs_link: String,
    pub on_home: EventHandler<()>,
    pub on_login: EventHandler<()>,
    pub on_register: EventHandler<()>,
    pub on_admin: EventHandler<()>,
    pub on_logout: EventHandler<()>,
}

pub fn TopNav(props: TopNavProps) -> Element {
    let is_home_active = !props.is_admin && !props.is_register && !props.is_login;
    let mut current_time = use_signal(|| {
        Date::new_0()
            .to_locale_string("en-US", &JsValue::UNDEFINED)
            .as_string()
            .unwrap_or_default()
    });

    use_effect(move || {
        let mut current_time = current_time.clone();
        spawn(async move {
            loop {
                current_time.set(
                    Date::new_0()
                        .to_locale_string("en-US", &JsValue::UNDEFINED)
                        .as_string()
                        .unwrap_or_default(),
                );
                TimeoutFuture::new(1000).await;
            }
        });
    });

    rsx! {
        nav { class: "top-nav",
            div { class: "top-strip",
                div { class: "brand",
                    span { class: "brand__dot" }
                    span { "SoulForum" }
                    span { class: "brand__tag", "simple machines forum" }
                }
                div { class: "top-meta",
                    span { "{props.welcome_text}" }
                    span { class: "top-date", "{current_time}" }
                }
            }
            div { class: "nav-tabs",
                a {
                    class: if is_home_active { "nav-tab active" } else { "nav-tab" },
                    href: "{APP_BASE_PATH}/",
                    onclick: move |_| props.on_home.call(()),
                    "Home"
                }
                a { class: "nav-tab", href: "#", "Help" }
                a { class: "nav-tab", href: "#", "Search" }

                {if !props.is_logged_in { rsx! {
                    a {
                        class: if props.is_login { "nav-tab active" } else { "nav-tab" },
                        href: "{APP_BASE_PATH}/login",
                        onclick: move |_| props.on_login.call(()),
                        "Login"
                    }
                    a {
                        class: if props.is_register { "nav-tab active" } else { "nav-tab" },
                        href: "{APP_BASE_PATH}/register",
                        onclick: move |_| props.on_register.call(()),
                        "Register"
                    }
                }} else { rsx! {} }}

                {if props.is_logged_in { rsx! {
                    button {
                        class: "nav-tab nav-tab--ghost",
                        onclick: move |_| props.on_logout.call(()),
                        "Logout"
                    }
                }} else { rsx! {} }}

                a {
                    class: if props.is_admin { "nav-tab active" } else { "nav-tab" },
                    href: "{APP_BASE_PATH}/admin",
                    onclick: move |_| props.on_admin.call(()),
                    "Admin"
                }
                a { class: "nav-tab", href: "{props.blog_link}", "Blog" }
                a { class: "nav-tab", href: "{props.docs_link}", "Docs" }
                a { class: "nav-tab", href: "#", "More" }

                div { class: "nav-search",
                    input { placeholder: "Search", value: "" }
                    button { class: "nav-search__btn", "Search" }
                }
            }
        }
    }
}
