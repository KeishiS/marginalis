use super::*;

#[tokio::test]
async fn math_macros_are_private_and_revision_guarded() {
    let database = database().await;
    let alice = actor("https://id.example.test", "alice");
    let bob = actor("https://id.example.test", "bob");
    let macros = vec![
        MathMacro {
            name: "argmax".into(),
            replacement: r"\operatorname*{arg\,max}".into(),
            argument_count: 0,
        },
        MathMacro {
            name: "bm".into(),
            replacement: r"\boldsymbol{#1}".into(),
            argument_count: 1,
        },
    ];

    assert_eq!(
        database
            .read_math_macros(alice.principal())
            .await
            .expect("initial settings")
            .revision,
        0
    );
    let saved = database
        .replace_math_macros(alice.principal(), &macros, 0)
        .await
        .expect("save settings");
    assert_eq!(saved.macros, macros);
    assert_eq!(saved.revision, 1);
    assert_eq!(
        database
            .read_math_macros(alice.principal())
            .await
            .expect("saved settings"),
        saved
    );
    assert!(
        database
            .read_math_macros(bob.principal())
            .await
            .expect("other owner settings")
            .macros
            .is_empty()
    );
    assert_eq!(
        database
            .replace_math_macros(alice.principal(), &[], 0)
            .await,
        Err(StorageError::Conflict)
    );
    assert_eq!(
        database
            .replace_math_macros(alice.principal(), &[], 1)
            .await
            .expect("replace settings")
            .revision,
        2
    );
}

#[tokio::test]
async fn legacy_tex_unsafe_macro_is_restored_for_display_boundary_filtering() {
    let database = database().await;
    let alice = actor("https://id.example.test", "alice");
    let legacy = vec![MathMacro {
        name: "unused".into(),
        replacement: "{broken".into(),
        argument_count: 0,
    }];

    database
        .replace_math_macros(alice.principal(), &legacy, 0)
        .await
        .expect("store legacy settings fixture");

    assert_eq!(
        database
            .read_math_macros(alice.principal())
            .await
            .expect("restore legacy settings")
            .macros,
        legacy
    );
}
