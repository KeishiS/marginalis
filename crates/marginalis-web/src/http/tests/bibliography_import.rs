use super::*;
use marginalis_application::{
    BibliographyImportCandidate, BibliographyImportClassification, BibliographyImportDecision,
    BibliographyImportDecisionKind, BibliographyImportEntry, BibliographyImportInput,
    BibliographyImportPreview, BibliographyImportResult, BibliographyImportSourceSelection,
    BibliographyImportUseCaseError, BibliographyImportUseCases,
};
use marginalis_domain::{
    BibliographyImportSource, BibliographyImportSourceId, BibliographyItemId, EntityId, Revision,
};

#[derive(Default)]
struct Imports {
    previews: Mutex<Vec<BibliographyImportInput>>,
    applications: Mutex<Vec<(BibliographyImportInput, Vec<BibliographyImportDecision>)>>,
}

fn source_id() -> BibliographyImportSourceId {
    BibliographyImportSourceId::new(
        "0197c9bc-0000-7000-8000-000000000101"
            .parse::<EntityId>()
            .expect("source ID"),
    )
}

fn item_id() -> BibliographyItemId {
    BibliographyItemId::new(
        "0197c9bc-0000-7000-8000-000000000102"
            .parse::<EntityId>()
            .expect("item ID"),
    )
}

#[async_trait]
impl BibliographyImportUseCases for Imports {
    async fn list_bibliography_import_sources(
        &self,
        _actor: Actor,
    ) -> Result<Vec<BibliographyImportSource>, BibliographyImportUseCaseError> {
        Ok(Vec::new())
    }

    async fn preview_bibliography_import(
        &self,
        _actor: Actor,
        input: BibliographyImportInput,
    ) -> Result<BibliographyImportPreview, BibliographyImportUseCaseError> {
        self.previews.lock().expect("preview lock").push(input);
        Ok(BibliographyImportPreview {
            source_id: Some(source_id()),
            source_revision: Some(Revision::new(3).expect("revision")),
            preview_token: "a".repeat(64),
            entries: vec![
                BibliographyImportEntry {
                    position: 0,
                    external_item_id: Some("external-1".into()),
                    citation_key: Some("smith2026".into()),
                    classification: BibliographyImportClassification::Conflict,
                    item_id: Some(item_id()),
                    item_revision: Some(Revision::new(4).expect("revision")),
                    current_csl_json: Some(serde_json::json!({
                        "id": "smith2026", "title": "Marginalis側の文献", "type": "book"
                    })),
                    candidates: Vec::new(),
                    rejection_code: None,
                },
                BibliographyImportEntry {
                    position: 1,
                    external_item_id: Some("external-2".into()),
                    citation_key: Some("jones2026".into()),
                    classification: BibliographyImportClassification::DuplicateCandidate,
                    item_id: None,
                    item_revision: None,
                    current_csl_json: None,
                    candidates: vec![BibliographyImportCandidate {
                        item_id: item_id(),
                        citation_key: "existing2026".into(),
                        title: Some("既存文献".into()),
                        revision: Revision::new(4).expect("revision"),
                        matched_by: vec!["doi".into()],
                    }],
                    rejection_code: None,
                },
            ],
        })
    }

    async fn apply_bibliography_import(
        &self,
        _actor: Actor,
        input: BibliographyImportInput,
        decisions: Vec<BibliographyImportDecision>,
        preview_token: String,
    ) -> Result<BibliographyImportResult, BibliographyImportUseCaseError> {
        if preview_token != "a".repeat(64) {
            return Err(BibliographyImportUseCaseError::Conflict);
        }
        self.applications
            .lock()
            .expect("application lock")
            .push((input, decisions));
        Ok(BibliographyImportResult {
            source_id: source_id(),
            source_revision: Revision::new(4).expect("revision"),
            created: 0,
            updated: 1,
            kept: 1,
            excluded: 0,
        })
    }
}

fn preview_body() -> serde_json::Value {
    serde_json::json!({
        "source": {
            "kind": "existing",
            "source_id": source_id().to_string()
        },
        "items": [
            {"id": "external-1", "title": "外部で更新した文献"},
            {"id": "external-2", "title": "重複候補"}
        ]
    })
}

fn authenticated_json_request(
    method: &str,
    path: &str,
    body: serde_json::Value,
    csrf: bool,
) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if csrf {
        request = request
            .header(header::ORIGIN, "https://example.test")
            .header("sec-fetch-site", "same-origin")
            .header(
                header::COOKIE,
                "__Host-marginalis_session=active-session; __Host-marginalis_csrf=session-csrf",
            )
            .header("x-csrf-token", "session-csrf");
    } else {
        request = request.header(header::COOKIE, "__Host-marginalis_session=active-session");
    }
    request.body(Body::from(body.to_string())).expect("request")
}

#[tokio::test]
async fn preview_is_read_only_and_exposes_conflicts_and_duplicate_candidates() {
    let imports = Arc::new(Imports::default());
    let response = TestApp::default()
        .authenticated()
        .bibliography_import(imports.clone())
        .router()
        .oneshot(authenticated_json_request(
            "POST",
            "/api/v3/bibliography/import-previews",
            preview_body(),
            false,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(body["source_revision"], 3);
    assert_eq!(body["preview_token"], "a".repeat(64));
    assert_eq!(body["entries"][0]["classification"], "conflict");
    assert_eq!(
        body["entries"][0]["current_csl_json"]["title"],
        "Marginalis側の文献"
    );
    assert_eq!(body["entries"][1]["classification"], "duplicate_candidate");
    assert_eq!(body["entries"][1]["candidates"][0]["matched_by"][0], "doi");
    let previews = imports.previews.lock().expect("preview lock");
    assert_eq!(previews.len(), 1);
    assert!(matches!(
        previews[0].source,
        BibliographyImportSourceSelection::Existing { source_id: id } if id == source_id()
    ));
}

#[tokio::test]
async fn apply_requires_csrf_and_forwards_explicit_decisions() {
    let imports = Arc::new(Imports::default());
    let mut body = preview_body();
    body["preview_token"] = serde_json::json!("a".repeat(64));
    body["decisions"] = serde_json::json!([
        {"position": 0, "action": "use_external", "candidate_item_id": null},
        {
            "position": 1,
            "action": "link_existing_keep_local",
            "candidate_item_id": item_id().to_string()
        }
    ]);

    let app = TestApp::default()
        .authenticated()
        .bibliography_import(imports.clone())
        .router();
    let forbidden = app
        .clone()
        .oneshot(authenticated_json_request(
            "POST",
            "/api/v3/bibliography/imports",
            body.clone(),
            false,
        ))
        .await
        .expect("response");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert!(
        imports
            .applications
            .lock()
            .expect("application lock")
            .is_empty()
    );

    let applied = app
        .oneshot(authenticated_json_request(
            "POST",
            "/api/v3/bibliography/imports",
            body,
            true,
        ))
        .await
        .expect("response");
    assert_eq!(applied.status(), StatusCode::OK);
    let response = to_bytes(applied.into_body(), usize::MAX)
        .await
        .expect("response body");
    let response: serde_json::Value = serde_json::from_slice(&response).expect("JSON");
    assert_eq!(response["source_revision"], 4);
    assert_eq!(response["updated"], 1);
    assert_eq!(response["kept"], 1);

    let applications = imports.applications.lock().expect("application lock");
    assert_eq!(applications.len(), 1);
    assert_eq!(applications[0].1.len(), 2);
    assert_eq!(
        applications[0].1[0].kind,
        BibliographyImportDecisionKind::UseExternal
    );
    assert_eq!(
        applications[0].1[1],
        BibliographyImportDecision {
            position: 1,
            kind: BibliographyImportDecisionKind::LinkExistingKeepLocal,
            candidate_item_id: Some(item_id()),
        }
    );
}

#[tokio::test]
async fn stale_preview_and_invalid_source_id_have_stable_problem_responses() {
    let imports = Arc::new(Imports::default());
    let app = TestApp::default()
        .authenticated()
        .bibliography_import(imports)
        .router();
    let invalid = app
        .clone()
        .oneshot(authenticated_json_request(
            "POST",
            "/api/v3/bibliography/import-previews",
            serde_json::json!({
                "source": {"kind": "existing", "source_id": "not-an-id"},
                "items": [{"id": "external-1"}]
            }),
            false,
        ))
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let mut stale_body = preview_body();
    stale_body["preview_token"] = serde_json::json!("b".repeat(64));
    stale_body["decisions"] = serde_json::json!([
        {"position": 0, "action": "use_external", "candidate_item_id": null},
        {"position": 1, "action": "exclude", "candidate_item_id": null}
    ]);
    let stale = app
        .oneshot(authenticated_json_request(
            "POST",
            "/api/v3/bibliography/imports",
            stale_body,
            true,
        ))
        .await
        .expect("response");
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let body = to_bytes(stale.into_body(), usize::MAX)
        .await
        .expect("response body");
    let problem: serde_json::Value = serde_json::from_slice(&body).expect("problem JSON");
    assert_eq!(problem["code"], "conflict");
}
