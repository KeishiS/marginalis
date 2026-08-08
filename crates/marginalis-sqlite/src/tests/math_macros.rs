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
            .read_math_macros(alice.identity())
            .await
            .expect("initial settings")
            .revision,
        0
    );
    let saved = database
        .replace_math_macros(alice.identity(), &macros, 0)
        .await
        .expect("save settings");
    assert_eq!(saved.macros, macros);
    assert_eq!(saved.revision, 1);
    assert_eq!(
        database
            .read_math_macros(alice.identity())
            .await
            .expect("saved settings"),
        saved
    );
    assert!(
        database
            .read_math_macros(bob.identity())
            .await
            .expect("other owner settings")
            .macros
            .is_empty()
    );
    assert_eq!(
        database.replace_math_macros(alice.identity(), &[], 0).await,
        Err(MathMacroRepositoryError::Conflict)
    );
    assert_eq!(
        database
            .replace_math_macros(alice.identity(), &[], 1)
            .await
            .expect("replace settings")
            .revision,
        2
    );
}
