use super::*;

fn mk(namespace: &'static str, function: &'static str) -> ControllerSchema {
    ControllerSchema {
        namespace,
        function,
        description: "",
        inputs: vec![],
        outputs: vec![],
    }
}

#[test]
fn method_name_joins_namespace_and_function_with_dot() {
    let s = mk("memory", "doc_put");
    assert_eq!(s.method_name(), "memory.doc_put");
}

#[test]
fn method_name_is_not_an_rpc_method_name() {
    // The dotted controller key and the `openhuman.<ns>_<fn>` RPC method
    // name are intentionally different — guard against drift.
    let s = mk("memory", "doc_put");
    assert_eq!(s.method_name(), "memory.doc_put");
    assert_eq!(
        crate::core::all::rpc_method_name(&s),
        "openhuman.memory_doc_put"
    );
}

#[test]
fn method_name_preserves_underscores_in_function() {
    let s = mk("team", "change_member_role");
    assert_eq!(s.method_name(), "team.change_member_role");
}

#[test]
fn controller_schema_equality_considers_all_fields() {
    let a = ControllerSchema {
        namespace: "a",
        function: "b",
        description: "x",
        inputs: vec![],
        outputs: vec![],
    };
    let b = ControllerSchema {
        namespace: "a",
        function: "b",
        description: "x",
        inputs: vec![],
        outputs: vec![],
    };
    let c = ControllerSchema {
        namespace: "a",
        function: "b",
        description: "different",
        inputs: vec![],
        outputs: vec![],
    };
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn type_schema_nesting_is_equality_comparable() {
    let a = TypeSchema::Array(Box::new(TypeSchema::Option(Box::new(TypeSchema::String))));
    let b = TypeSchema::Array(Box::new(TypeSchema::Option(Box::new(TypeSchema::String))));
    let c = TypeSchema::Array(Box::new(TypeSchema::Option(Box::new(TypeSchema::I64))));
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn field_schema_required_flag_changes_equality() {
    let a = FieldSchema {
        name: "x",
        ty: TypeSchema::Bool,
        comment: "",
        required: true,
    };
    let b = FieldSchema {
        name: "x",
        ty: TypeSchema::Bool,
        comment: "",
        required: false,
    };
    assert_ne!(a, b);
}

#[test]
fn controller_schema_serializes_to_json() {
    // Schema must be JSON-serializable: the /schema endpoint depends on it.
    let s = ControllerSchema {
        namespace: "health",
        function: "snapshot",
        description: "d",
        inputs: vec![FieldSchema {
            name: "limit",
            ty: TypeSchema::U64,
            comment: "cap",
            required: false,
        }],
        outputs: vec![FieldSchema {
            name: "ok",
            ty: TypeSchema::Bool,
            comment: "",
            required: true,
        }],
    };
    let json = serde_json::to_value(&s).unwrap();
    assert_eq!(json["namespace"], "health");
    assert_eq!(json["function"], "snapshot");
    assert_eq!(json["inputs"][0]["name"], "limit");
    assert_eq!(json["outputs"][0]["required"], true);
}
