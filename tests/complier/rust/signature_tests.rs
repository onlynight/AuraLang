//! Signature (.sig) unit tests — TypeSig, ModuleSig, serialization, file I/O

use compiler::signature::*;

// ─── Constants ──────────────────────────────────────────────────────────────

#[test]
fn test_sig_constants() {
    assert_eq!(SIG_VERSION, 1);
    assert_eq!(SIG_MAGIC, b"ASIG");
}

// ─── TypeSig: to_string ────────────────────────────────────────────────────

#[test]
fn test_type_sig_primitives() {
    assert_eq!(TypeSig::Int.to_string(), "int");
    assert_eq!(TypeSig::Float.to_string(), "float");
    assert_eq!(TypeSig::Bool.to_string(), "bool");
    assert_eq!(TypeSig::Str.to_string(), "str");
    assert_eq!(TypeSig::Null.to_string(), "null");
}

#[test]
fn test_type_sig_ref() {
    let sig = TypeSig::Ref(Box::new(TypeSig::Int));
    assert_eq!(sig.to_string(), "&int");
}

#[test]
fn test_type_sig_tuple() {
    let sig = TypeSig::Tuple(vec![
        TypeSig::Int,
        TypeSig::Str,
    ]);
    assert_eq!(sig.to_string(), "(int, str)");
}

#[test]
fn test_type_sig_func() {
    let sig = TypeSig::Func(
        vec![
            TypeSig::Int,
            TypeSig::Str,
        ],
        Box::new(TypeSig::Bool),
    );
    assert_eq!(sig.to_string(), "(int, str) -> bool");
}

#[test]
fn test_type_sig_generic() {
    let sig = TypeSig::Generic("List".to_string(), vec![TypeSig::Int]);
    assert_eq!(sig.to_string(), "List<int>");
}

#[test]
fn test_type_sig_generic_multiple_params() {
    let sig = TypeSig::Generic(
        "Map".to_string(),
        vec![
            TypeSig::Str,
            TypeSig::Int,
        ],
    );
    assert_eq!(sig.to_string(), "Map<str, int>");
}

#[test]
fn test_type_sig_user_type_simple() {
    let sig = TypeSig::UserType {
        name: "Point".to_string(),
        type_params: vec![],
    };
    assert_eq!(sig.to_string(), "Point");
}

#[test]
fn test_type_sig_user_type_with_generics() {
    let sig = TypeSig::UserType {
        name: "Pair".to_string(),
        type_params: vec![
            "T".to_string(),
            "U".to_string(),
        ],
    };
    assert_eq!(sig.to_string(), "Pair<T, U>");
}

#[test]
fn test_type_sig_nested() {
    let sig = TypeSig::Func(
        vec![TypeSig::Ref(Box::new(TypeSig::Str))],
        Box::new(TypeSig::Tuple(vec![
            TypeSig::Int,
            TypeSig::Bool,
        ])),
    );
    assert_eq!(sig.to_string(), "(&str) -> (int, bool)");
}

// ─── TypeDefKind ────────────────────────────────────────────────────────────

#[test]
fn test_type_def_kind_serialization() {
    // Verify all variants can be serialized/deserialized
    for kind in [
        TypeDefKind::Struct,
        TypeDefKind::Class,
        TypeDefKind::Interface,
        TypeDefKind::Enum,
        TypeDefKind::TypeAlias,
    ] {
        let json = serde_json::to_string(&kind).unwrap();
        let loaded: TypeDefKind = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, kind);
    }
}

// ─── SymbolKind ─────────────────────────────────────────────────────────────

#[test]
fn test_symbol_kind_serialization() {
    for kind in [
        SymbolKind::Function,
        SymbolKind::Type,
        SymbolKind::Const,
    ] {
        let json = serde_json::to_string(&kind).unwrap();
        let loaded: SymbolKind = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, kind);
    }
}

// ─── ConstValue ─────────────────────────────────────────────────────────────

#[test]
fn test_const_value_serialization() {
    for val in [
        ConstValue::Int(42),
        ConstValue::Float(3.14),
        ConstValue::Bool(true),
        ConstValue::Str("hello".to_string()),
    ] {
        let json = serde_json::to_string(&val).unwrap();
        let loaded: ConstValue = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, val);
    }
}

// ─── ModuleSig: new and lookup ─────────────────────────────────────────────

#[test]
fn test_module_sig_new() {
    let sig = ModuleSig::new("math-lib", "1.0.0", [1u8; 16]);
    assert_eq!(sig.module_name, "math-lib");
    assert_eq!(sig.module_version, "1.0.0");
    assert_eq!(sig.uuid, [1u8; 16]);
    assert_eq!(sig.version, SIG_VERSION);
    assert!(sig.functions.is_empty());
    assert!(sig.types.is_empty());
    assert!(sig.constants.is_empty());
    assert!(sig.imports.is_empty());
    assert!(sig.dependencies.is_empty());
}

#[test]
fn test_module_sig_find_function() {
    let mut sig = ModuleSig::new("test", "1.0", [0u8; 16]);
    sig.functions = vec![
        FuncSig {
            name: "add".to_string(),
            params: vec![],
            return_type: TypeSig::Int,
            is_public: true,
            type_params: vec![],
        },
        FuncSig {
            name: "sub".to_string(),
            params: vec![],
            return_type: TypeSig::Int,
            is_public: true,
            type_params: vec![],
        },
    ];

    assert!(sig.find_function("add").is_some());
    assert!(sig.find_function("sub").is_some());
    assert!(sig.find_function("mul").is_none());
}

#[test]
fn test_module_sig_find_type() {
    let mut sig = ModuleSig::new("test", "1.0", [0u8; 16]);
    sig.types = vec![
        compiler::signature::TypeDefSig {
            name: "Point".to_string(),
            kind: TypeDefKind::Struct,
            type_params: vec![],
            fields: vec![],
            methods: vec![],
            variants: vec![],
            super_types: vec![],
            is_public: true,
        },
    ];

    assert!(sig.find_type("Point").is_some());
    assert!(sig.find_type("Line").is_none());
}

#[test]
fn test_module_sig_find_constant() {
    let mut sig = ModuleSig::new("test", "1.0", [0u8; 16]);
    sig.constants = vec![ConstSig {
        name: "PI".to_string(),
        type_sig: TypeSig::Float,
        value: Some(ConstValue::Float(3.14159)),
        is_public: true,
    }];

    assert!(sig.find_constant("PI").is_some());
    assert!(sig.find_constant("E").is_none());
}

// ─── Serialization roundtrip ───────────────────────────────────────────────

#[test]
fn test_sig_to_bytes_roundtrip() {
    let sig = ModuleSig {
        module_name: "math-lib".to_string(),
        module_version: "1.0.0".to_string(),
        uuid: [0u8; 16],
        version: SIG_VERSION,
        functions: vec![FuncSig {
            name: "add".to_string(),
            params: vec![
                TypeSig::Int,
                TypeSig::Int,
            ],
            return_type: TypeSig::Int,
            is_public: true,
            type_params: vec![],
        }],
        types: vec![],
        constants: vec![ConstSig {
            name: "PI".to_string(),
            type_sig: TypeSig::Float,
            value: Some(ConstValue::Float(3.14159)),
            is_public: true,
        }],
        imports: vec![],
        dependencies: vec![],
    };

    let bytes = to_bytes(&sig).unwrap();
    assert!(bytes.len() > 10); // At least magic + version + json_len + json

    // Verify magic
    assert_eq!(&bytes[0..4], SIG_MAGIC);

    let loaded = from_bytes(&bytes).unwrap();
    assert_eq!(loaded.module_name, "math-lib");
    assert_eq!(loaded.module_version, "1.0.0");
    assert_eq!(loaded.functions.len(), 1);
    assert_eq!(loaded.functions[0].name, "add");
    assert_eq!(loaded.constants.len(), 1);
    assert_eq!(loaded.constants[0].name, "PI");
}

#[test]
fn test_sig_from_bytes_too_small() {
    let bytes = b"ASIG";
    assert!(from_bytes(bytes).is_err());
}

#[test]
fn test_sig_from_bytes_bad_magic() {
    let bytes = b"XXXX\x00\x01\x00\x00\x00\x00";
    assert!(from_bytes(bytes).is_err());
}

#[test]
fn test_sig_from_bytes_bad_version() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"ASIG");
    bytes.extend_from_slice(&2u16.to_le_bytes()); // unsupported version
    bytes.extend_from_slice(&0u32.to_le_bytes());
    assert!(from_bytes(&bytes).is_err());
}

#[test]
fn test_sig_from_bytes_truncated_json() {
    let sig = ModuleSig::new("test", "1.0", [0u8; 16]);
    let bytes = to_bytes(&sig).unwrap();
    // Truncate the bytes to cut off the JSON
    let truncated = &bytes[..bytes.len() - 5];
    assert!(from_bytes(truncated).is_err());
}

// ─── File I/O ──────────────────────────────────────────────────────────────

#[test]
fn test_sig_write_and_read_file() {
    let tmp = tempfile::tempdir().unwrap();
    let sig_path = tmp.path().join("test.sig");

    let sig = ModuleSig {
        module_name: "math".to_string(),
        module_version: "1.0.0".to_string(),
        uuid: [42u8; 16],
        version: SIG_VERSION,
        functions: vec![FuncSig {
            name: "add".to_string(),
            params: vec![
                TypeSig::Int,
                TypeSig::Int,
            ],
            return_type: TypeSig::Int,
            is_public: true,
            type_params: vec![],
        }],
        types: vec![],
        constants: vec![],
        imports: vec![],
        dependencies: vec![],
    };

    write_sig(sig_path.to_str().unwrap(), &sig).unwrap();
    assert!(sig_path.exists());

    let loaded = read_sig(sig_path.to_str().unwrap()).unwrap();
    assert_eq!(loaded.module_name, "math");
    assert_eq!(loaded.module_version, "1.0.0");
    assert_eq!(loaded.uuid, [42u8; 16]);
    assert_eq!(loaded.functions.len(), 1);
    assert_eq!(loaded.functions[0].name, "add");
}

#[test]
fn test_sig_read_file_not_found() {
    let result = read_sig("/nonexistent/path.sig");
    assert!(result.is_err());
}

// ─── Complex nested sig ────────────────────────────────────────────────────

#[test]
fn test_sig_complex_nested_types() {
    let sig = ModuleSig {
        module_name: "complex".to_string(),
        module_version: "2.0.0".to_string(),
        uuid: [99u8; 16],
        version: SIG_VERSION,
        functions: vec![FuncSig {
            name: "process".to_string(),
            params: vec![
                TypeSig::Ref(Box::new(TypeSig::Str)),
                TypeSig::Generic("List".to_string(), vec![TypeSig::Int]),
                TypeSig::Func(vec![TypeSig::Int], Box::new(TypeSig::Bool)),
            ],
            return_type: TypeSig::Tuple(vec![
                TypeSig::Int,
                TypeSig::Str,
            ]),
            is_public: true,
            type_params: vec!["T".to_string()],
        }],
        types: vec![
            TypeDefSig {
                name: "Container".to_string(),
                kind: TypeDefKind::Class,
                type_params: vec!["T".to_string()],
                fields: vec![FieldSig {
                    name: "items".to_string(),
                    type_sig: TypeSig::Generic(
                        "Vec".to_string(),
                        vec![
                            TypeSig::UserType {
                                name: "T".to_string(),
                                type_params: vec![],
                            },
                        ],
                    ),
                    is_public: true,
                    is_mutable: true,
                }],
                methods: vec![FuncSig {
                    name: "new".to_string(),
                    params: vec![],
                    return_type: TypeSig::UserType {
                        name: "Container".to_string(),
                        type_params: vec!["T".to_string()],
                    },
                    is_public: true,
                    type_params: vec![],
                }],
                variants: vec![],
                super_types: vec![],
                is_public: true,
            },
        ],
        constants: vec![],
        imports: vec![
            ImportSig {
                module: "math".to_string(),
                symbols: vec![
                    ImportSymbolSig {
                        name: "add".to_string(),
                        kind: SymbolKind::Function,
                    },
                ],
                aliases: std::collections::BTreeMap::new(),
            },
        ],
        dependencies: vec![
            "math".to_string(),
            "string".to_string(),
        ],
    };

    let bytes = to_bytes(&sig).unwrap();
    let loaded = from_bytes(&bytes).unwrap();

    assert_eq!(loaded.module_name, "complex");
    assert_eq!(loaded.module_version, "2.0.0");
    assert_eq!(loaded.functions[0].params.len(), 3);
    assert_eq!(loaded.types[0].name, "Container");
    assert_eq!(loaded.types[0].type_params, vec!["T"]);
    assert_eq!(loaded.types[0].fields.len(), 1);
    assert_eq!(loaded.types[0].methods.len(), 1);
    assert_eq!(loaded.imports.len(), 1);
    assert_eq!(loaded.imports[0].module, "math");
    assert_eq!(loaded.dependencies.len(), 2);
}

// ─── SigError Display ──────────────────────────────────────────────────────

#[test]
fn test_sig_error_display_io() {
    let e = SigError::Io("read failed".to_string());
    assert!(e.to_string().contains("read failed"));
}

#[test]
fn test_sig_error_display_format() {
    let e = SigError::Format("bad format".to_string());
    assert!(e.to_string().contains("bad format"));
}

#[test]
fn test_sig_error_display_parse() {
    let e = SigError::Parse("parse error".to_string());
    assert!(e.to_string().contains("parse error"));
}
