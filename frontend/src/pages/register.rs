use dioxus::prelude::*;

#[component]
pub fn RegisterPage(
    register_username: Signal<String>,
    register_password: Signal<String>,
    register_confirm: Signal<String>,
    on_register: EventHandler<()>,
) -> Element {
    rsx! {
        section { class: "panel register-panel",
            h2 { "Register - Required Information" }
            div { class: "register-note",
                p { "Please fill in the required information below. JavaScript is required for the registration page." }
            }
            div { class: "register-grid",
                div { class: "register-labels",
                    label { "Email" }
                    label { "Password" }
                    label { "Verify password" }
                }
                div { class: "register-fields",
                    input {
                        value: "{register_username.read()}",
                        oninput: move |evt| register_username.set(evt.value()),
                        placeholder: "you@example.com"
                    }
                    input {
                        value: "{register_password.read()}",
                        oninput: move |evt| register_password.set(evt.value()),
                        placeholder: "Password",
                        r#type: "password"
                    }
                    input {
                        value: "{register_confirm.read()}",
                        oninput: move |evt| register_confirm.set(evt.value()),
                        placeholder: "Repeat password",
                        r#type: "password"
                    }
                }
            }
            div { class: "register-actions",
                button { onclick: move |_| on_register.call(()), "Register" }
            }
        }
    }
}
