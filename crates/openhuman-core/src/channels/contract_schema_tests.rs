use super::*;

#[test]
fn converts_the_contract_schema_with_optional_types() {
    let s = contract_controller_schema("telegram_login_check");
    assert_eq!(s.namespace, "channels");
    assert_eq!(s.function, "telegram_login_check");
    let token = s.inputs.iter().find(|f| f.name == "linkToken").unwrap();
    assert!(token.required);
    assert!(matches!(token.ty, TypeSchema::String));

    let status = contract_controller_schema("status");
    assert!(status
        .inputs
        .iter()
        .any(|f| !f.required && matches!(f.ty, TypeSchema::Option(_))));
}
