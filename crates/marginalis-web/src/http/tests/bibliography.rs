use super::*;
use marginalis_application::{
    BibliographyApplication, BibliographyRepository, Clock, Random, StorageError,
};
use marginalis_domain::{BibliographyItem, BibliographyItemId, EntityId, ValidatedCslJson};

#[derive(Default)]
struct Library {
    items: Mutex<Vec<BibliographyItem>>,
    search_error: Mutex<Option<StorageError>>,
}

#[async_trait]
impl BibliographyRepository for Library {
    async fn search_owned_items(
        &self,
        actor: &Actor,
        query: &str,
    ) -> Result<Vec<BibliographyItem>, StorageError> {
        if let Some(error) = self.search_error.lock().expect("error lock").take() {
            return Err(error);
        }
        Ok(self
            .items
            .lock()
            .expect("items lock")
            .iter()
            .filter(|item| item.owner() == actor.identity() && item.csl_json().contains(query))
            .cloned()
            .collect())
    }

    async fn items_by_citation_keys(
        &self,
        owner: &Identity,
        citation_keys: &[String],
    ) -> Result<Vec<BibliographyItem>, StorageError> {
        Ok(self
            .items
            .lock()
            .expect("items lock")
            .iter()
            .filter(|item| {
                item.owner() == owner && citation_keys.iter().any(|key| key == item.citation_key())
            })
            .cloned()
            .collect())
    }

    async fn create_owned_item(&self, item: &BibliographyItem) -> Result<(), StorageError> {
        let mut items = self.items.lock().expect("items lock");
        if items.iter().any(|stored| {
            stored.owner() == item.owner() && stored.citation_key() == item.citation_key()
        }) {
            return Err(StorageError::Conflict);
        }
        items.push(item.clone());
        Ok(())
    }

    async fn update_owned_item(
        &self,
        actor: &Actor,
        item_id: BibliographyItemId,
        csl_json: &ValidatedCslJson,
        updated_at: UnixMillis,
        expected_revision: Revision,
    ) -> Result<BibliographyItem, StorageError> {
        let mut items = self.items.lock().expect("items lock");
        let Some(position) = items
            .iter()
            .position(|item| item.item_id() == item_id && item.owner() == actor.identity())
        else {
            return Err(StorageError::NotFound);
        };
        let current = &items[position];
        if current.revision() != expected_revision {
            return Err(StorageError::Conflict);
        }
        let revision = Revision::new(current.revision().get() + 1).expect("next revision");
        let updated = BibliographyItem::restore(
            item_id,
            actor.identity().clone(),
            csl_json.citation_key().into(),
            csl_json.encoded().into(),
            current.created_at(),
            updated_at,
            revision,
        )
        .expect("validated item");
        items[position] = updated.clone();
        Ok(updated)
    }

    async fn delete_owned_item(
        &self,
        actor: &Actor,
        item_id: BibliographyItemId,
        expected_revision: Revision,
    ) -> Result<(), StorageError> {
        let mut items = self.items.lock().expect("items lock");
        let Some(position) = items
            .iter()
            .position(|item| item.item_id() == item_id && item.owner() == actor.identity())
        else {
            return Err(StorageError::NotFound);
        };
        if items[position].revision() != expected_revision {
            return Err(StorageError::Conflict);
        }
        items.remove(position);
        Ok(())
    }
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> UnixMillis {
        UnixMillis::new(1_000)
    }
}

struct FixedRandom;

impl Random for FixedRandom {
    fn uuid_v7(&self) -> EntityId {
        "0197c9bc-0000-7000-8000-000000000111"
            .parse()
            .expect("UUIDv7")
    }

    fn opaque_token(&self) -> String {
        "unused".into()
    }
}

fn test_app(library: Arc<Library>) -> Router {
    TestApp::default()
        .authenticated()
        .bibliography(Arc::new(BibliographyApplication::new(
            library,
            Arc::new(FixedClock),
            Arc::new(FixedRandom),
        )))
        .router()
}

fn mcp_bibliography_app(library: Arc<Library>) -> Router {
    TestApp::default()
        .bibliography(Arc::new(BibliographyApplication::new(
            library,
            Arc::new(FixedClock),
            Arc::new(FixedRandom),
        )))
        .mcp(
            "https://example.test",
            vec!["https://chatgpt.com".into()],
            Arc::new(TestMcpAuthenticator),
        )
        .router()
}

fn mcp_call(name: &str, arguments: serde_json::Value) -> Request<Body> {
    Request::post("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, "Bearer valid-token")
        .body(Body::from(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments}
            })
            .to_string(),
        ))
        .expect("request")
}

fn mutation(
    method: &str,
    uri: &str,
    body: serde_json::Value,
    revision: Option<i64>,
) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "https://example.test")
        .header("sec-fetch-site", "same-origin")
        .header(
            header::COOKIE,
            "marginalis_session=active-session; marginalis_csrf=session-csrf",
        )
        .header("x-csrf-token", "session-csrf");
    if let Some(revision) = revision {
        request = request.header(header::IF_MATCH, format!("\"rev-{revision}\""));
    }
    request.body(Body::from(body.to_string())).expect("request")
}

#[tokio::test]
async fn rest_bibliography_crud_returns_etags_and_detects_revision_conflicts() {
    let library = Arc::new(Library::default());
    let app = test_app(library);
    let created = app
        .clone()
        .oneshot(mutation(
            "POST",
            "/api/v3/bibliography",
            serde_json::json!({"csl_json":{"id":"smith2026","type":"book"}}),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(created.headers()[header::ETAG], "\"rev-1\"");
    let body: serde_json::Value = serde_json::from_slice(
        &to_bytes(created.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("JSON");
    let item_id = body["item_id"].as_str().expect("item ID");

    let searched = app
        .clone()
        .oneshot(authenticated_request(
            "/api/v3/bibliography?query=smith2026",
        ))
        .await
        .expect("response");
    assert_eq!(searched.status(), StatusCode::OK);
    let searched: serde_json::Value = serde_json::from_slice(
        &to_bytes(searched.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("JSON");
    assert_eq!(searched[0]["item_id"], item_id);

    let updated = app
        .clone()
        .oneshot(mutation(
            "PUT",
            &format!("/api/v3/bibliography/{item_id}"),
            serde_json::json!({"csl_json":{"id":"smith2026","type":"book","title":"改訂"}}),
            Some(1),
        ))
        .await
        .expect("response");
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(updated.headers()[header::ETAG], "\"rev-2\"");

    let conflict = app
        .clone()
        .oneshot(mutation(
            "DELETE",
            &format!("/api/v3/bibliography/{item_id}"),
            serde_json::Value::Null,
            Some(1),
        ))
        .await
        .expect("response");
    assert_eq!(conflict.status(), StatusCode::CONFLICT);

    let deleted = app
        .clone()
        .oneshot(mutation(
            "DELETE",
            &format!("/api/v3/bibliography/{item_id}"),
            serde_json::Value::Null,
            Some(2),
        ))
        .await
        .expect("response");
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let missing = app
        .oneshot(mutation(
            "DELETE",
            &format!("/api/v3/bibliography/{item_id}"),
            serde_json::Value::Null,
            Some(2),
        ))
        .await
        .expect("response");
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rest_bibliography_enforces_authentication_csrf_and_error_mapping() {
    let unauthorized = TestApp::default()
        .router()
        .oneshot(
            Request::get("/api/v3/bibliography")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let library = Arc::new(Library::default());
    let app = test_app(library.clone());
    let without_csrf = app
        .clone()
        .oneshot(
            Request::post("/api/v3/bibliography")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, "marginalis_session=active-session")
                .body(Body::from(r#"{"csl_json":{"id":"smith","type":"book"}}"#))
                .unwrap(),
        )
        .await
        .expect("response");
    assert_eq!(without_csrf.status(), StatusCode::FORBIDDEN);

    let invalid = app
        .clone()
        .oneshot(mutation(
            "POST",
            "/api/v3/bibliography",
            serde_json::json!({"csl_json":{"id":"smith"}}),
            None,
        ))
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);

    *library.search_error.lock().expect("error lock") = Some(StorageError::CorruptData);
    let corrupt = app
        .oneshot(authenticated_request("/api/v3/bibliography"))
        .await
        .expect("response");
    assert_eq!(corrupt.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn mcp_bibliography_tools_cover_success_and_validation_failure() {
    let app = mcp_bibliography_app(Arc::new(Library::default()));
    let added = app
        .clone()
        .oneshot(mcp_call(
            "add_bibliography_item",
            serde_json::json!({"csl_json":{"id":"smith2026","type":"book"}}),
        ))
        .await
        .expect("response");
    let added: serde_json::Value =
        serde_json::from_slice(&to_bytes(added.into_body(), usize::MAX).await.expect("body"))
            .expect("JSON");
    assert_eq!(
        added["result"]["structuredContent"]["citation_key"],
        "smith2026"
    );
    let item_id = added["result"]["structuredContent"]["item_id"]
        .as_str()
        .expect("item ID");

    let found = app
        .clone()
        .oneshot(mcp_call(
            "search_bibliography",
            serde_json::json!({"query":"smith2026"}),
        ))
        .await
        .expect("response");
    let found: serde_json::Value =
        serde_json::from_slice(&to_bytes(found.into_body(), usize::MAX).await.expect("body"))
            .expect("JSON");
    assert_eq!(
        found["result"]["structuredContent"]["items"][0]["item_id"],
        item_id
    );

    let invalid = app
        .clone()
        .oneshot(mcp_call(
            "add_bibliography_item",
            serde_json::json!({"csl_json":{"id":"missing-type"}}),
        ))
        .await
        .expect("response");
    let invalid: serde_json::Value = serde_json::from_slice(
        &to_bytes(invalid.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("JSON");
    assert_eq!(invalid["result"]["isError"], true);
    assert_eq!(
        invalid["result"]["structuredContent"]["code"],
        "validation_failed"
    );

    let deleted = app
        .oneshot(mcp_call(
            "delete_bibliography_item",
            serde_json::json!({"item_id":item_id,"expected_revision":1}),
        ))
        .await
        .expect("response");
    let deleted: serde_json::Value = serde_json::from_slice(
        &to_bytes(deleted.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("JSON");
    assert_eq!(
        deleted["result"]["structuredContent"],
        serde_json::json!({})
    );
}
