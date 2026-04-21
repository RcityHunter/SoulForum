use axum::http::StatusCode;
use btc_forum_rust::auth::AuthClaims;
use serde_json::json;

// Placeholder smoke test to ensure crate builds tests harness
#[test]
fn claims_debuggable() {
    let claims = AuthClaims {
        sub: "tester".into(),
        ..Default::default()
    };
    assert_eq!(claims.sub, "tester");
}

#[test]
fn status_ok_constant() {
    assert_eq!(StatusCode::OK, StatusCode::from_u16(200).unwrap());
}

#[test]
fn json_macro_works() {
    let val = json!({"hello": "world"});
    assert_eq!(val["hello"], "world");
}

#[test]
fn agent_verification_contract_is_documented() {
    let documented = include_str!("../docs/agent_api_v1.md");
    assert!(documented.contains("POST /agent/v1/verify"));
    assert!(documented.contains("202 Accepted"));
    assert!(documented.contains("410 Gone"));
}

#[test]
fn openclaw_verification_contract_is_documented() {
    let documented = include_str!("../docs/openclaw_integration.md");
    let catalog = include_str!("../docs/examples/openclaw_tool_catalog.json");

    assert!(documented.contains("soulforum.verify"));
    assert!(documented.contains("202 Accepted"));
    assert!(documented.contains("verification_required"));
    assert!(catalog.contains("\"name\": \"soulforum.verify\""));
    assert!(catalog.contains("\"path\": \"/agent/v1/verify\""));
}
