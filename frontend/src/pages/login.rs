use dioxus::prelude::*;

#[component]
pub fn LoginPage(
    login_username: Signal<String>,
    login_password: Signal<String>,
    on_login: EventHandler<()>,
) -> Element {
    rsx! {
        section { class: "panel login-panel",
            div { class: "login-box",
                h2 { "Login" }
                div { class: "login-row",
                    label { "Email" }
                    input {
                        value: "{login_username.read()}",
                        oninput: move |evt| login_username.set(evt.value()),
                        placeholder: "you@example.com"
                    }
                }
                div { class: "login-row",
                    label { "Password" }
                    input {
                        value: "{login_password.read()}",
                        oninput: move |evt| login_password.set(evt.value()),
                        placeholder: "Password",
                        r#type: "password"
                    }
                }
                div { class: "register-actions",
                    button { onclick: move |_| on_login.call(()), "Login" }
                }
                div { class: "login-links",
                    a { href: "#", "Forgot your password?" }
                }
            }
        }
    }
}
