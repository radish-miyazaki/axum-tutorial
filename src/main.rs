mod handlers;
mod repositories;

use crate::handlers::label::{all_label, create_label, delete_label};
use crate::handlers::todo::{all_todo, create_todo, delete_todo, find_todo, update_todo};
use crate::repositories::label::{LabelRepository, LabelRepositoryForDb};
use crate::repositories::todo::{TodoRepository, TodoRepositoryForDb};
use axum::http::HeaderValue;
use axum::{
    extract::Extension,
    routing::{delete, get, post},
    Router,
};
use dotenv::dotenv;
use hyper::header::CONTENT_TYPE;
use sqlx::PgPool;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[tokio::main]
async fn main() {
    let log_level = env::var("RUST_LOG").unwrap_or("info".to_string());
    env::set_var("RUST_LOG", log_level);
    tracing_subscriber::fmt::init();
    dotenv().ok();

    let database_url = &env::var("DATABASE_URL").expect("undefined DATABASE_URL");
    tracing::info!("start connect database ...");
    let pool = PgPool::connect(database_url)
        .await
        .unwrap_or_else(|_| panic!("fail connect database, url is [{}]", database_url));
    let app = create_app(
        TodoRepositoryForDb::new(pool.clone()),
        LabelRepositoryForDb::new(pool.clone()),
    );
    let addr = SocketAddr::from(([127, 0, 0, 1], 5000));
    tracing::info!("listening on {}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

fn create_app<Todo: TodoRepository, Label: LabelRepository>(
    todo_repo: Todo,
    label_repo: Label,
) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/todos", post(create_todo::<Todo>).get(all_todo::<Todo>))
        .route(
            "/todos/:id",
            get(find_todo::<Todo>)
                .delete(delete_todo::<Todo>)
                .patch(update_todo::<Todo>),
        )
        .route(
            "/labels",
            post(create_label::<Label>).get(all_label::<Label>),
        )
        .route("/labels/:id", delete(delete_label::<Label>))
        .layer(Extension(Arc::new(todo_repo)))
        .layer(Extension(Arc::new(label_repo)))
        .layer(
            CorsLayer::new()
                .allow_origin("http://localhost:3000".parse::<HeaderValue>().unwrap())
                .allow_methods(Any)
                .allow_headers(vec![CONTENT_TYPE]),
        )
}

async fn root() -> &'static str {
    "Hello, world!"
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::repositories::label::test_utils::LabelRepositoryForMemory;
    use crate::repositories::label::{CreateLabel, Label};
    use crate::repositories::todo::{test_utils::TodoRepositoryForMemory, CreateTodo, Todo};
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
        response::Response,
    };
    use tower::ServiceExt;

    fn build_req_with_json(path: &str, method: Method, json_body: String) -> Request<Body> {
        Request::builder()
            .uri(path)
            .method(method)
            .header(CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .body(Body::from(json_body))
            .unwrap()
    }

    fn build_req_with_empty(method: Method, path: &str) -> Request<Body> {
        Request::builder()
            .uri(path)
            .method(method)
            .body(Body::empty())
            .unwrap()
    }

    async fn res_to_todo(res: Response) -> Todo {
        let bytes = hyper::body::to_bytes(res.into_body()).await.unwrap();
        let body: String = String::from_utf8(bytes.to_vec()).unwrap();
        let todo: Todo = serde_json::from_str(&body)
            .unwrap_or_else(|_| panic!("cannot convert Todo instance. body: {}", body));
        todo
    }

    async fn res_to_label(res: Response) -> Label {
        let bytes = hyper::body::to_bytes(res.into_body()).await.unwrap();
        let body: String = String::from_utf8(bytes.to_vec()).unwrap();
        let label: Label = serde_json::from_str(&body)
            .unwrap_or_else(|_| panic!("cannot convert Label instance. body: {}", body));
        label
    }

    #[tokio::test]
    async fn should_return_hello_world() {
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let res = create_app(
            TodoRepositoryForMemory::new(),
            LabelRepositoryForMemory::new(),
        )
        .oneshot(req)
        .await
        .unwrap();
        let bytes = hyper::body::to_bytes(res.into_body()).await.unwrap();
        let body: String = String::from_utf8(bytes.to_vec()).unwrap();
        assert_eq!(body, "Hello, world!")
    }

    #[tokio::test]
    async fn should_created_todo() {
        let expected = Todo::new(1, "should_return_created_todo".to_string());
        let req = build_req_with_json(
            "/todos",
            Method::POST,
            r#"{ "text": "should_return_created_todo" }"#.to_string(),
        );
        let res = create_app(
            TodoRepositoryForMemory::new(),
            LabelRepositoryForMemory::new(),
        )
        .oneshot(req)
        .await
        .unwrap();

        let todo = res_to_todo(res).await;
        assert_eq!(expected, todo);
    }

    #[tokio::test]
    async fn should_find_todo() {
        let expected = Todo::new(1, "should_find_todo".to_string());
        let todo_repo = TodoRepositoryForMemory::new();
        todo_repo
            .create(CreateTodo::new("should_find_todo".to_string()))
            .await
            .expect("failed create todo");
        let req = build_req_with_empty(Method::GET, "/todos/1");
        let res = create_app(todo_repo, LabelRepositoryForMemory::new())
            .oneshot(req)
            .await
            .unwrap();

        let todo = res_to_todo(res).await;
        assert_eq!(expected, todo);
    }

    #[tokio::test]
    async fn should_get_all_todos() {
        let expected = vec![Todo::new(1, "should_get_all_todo".to_string())];
        let todo_repo = TodoRepositoryForMemory::new();
        todo_repo
            .create(CreateTodo::new("should_get_all_todo".to_string()))
            .await
            .expect("failed create todo");
        let req = build_req_with_empty(Method::GET, "/todos");
        let res = create_app(todo_repo, LabelRepositoryForMemory::new())
            .oneshot(req)
            .await
            .unwrap();

        let bytes = hyper::body::to_bytes(res.into_body()).await.unwrap();
        let body: String = String::from_utf8(bytes.to_vec()).unwrap();
        let todos: Vec<Todo> = serde_json::from_str(&body)
            .unwrap_or_else(|_| panic!("cannot convert Todo instances. body: {}", body));
        assert_eq!(expected, todos);
    }

    #[tokio::test]
    async fn should_update_todo() {
        let expected = Todo::new(1, "should_update_todo".to_string());
        let todo_repo = TodoRepositoryForMemory::new();
        todo_repo
            .create(CreateTodo::new("before_update_todo".to_string()))
            .await
            .expect("failed create todo");
        let req = build_req_with_json(
            "/todos/1",
            Method::PATCH,
            r#"
        {
            "text": "should_update_todo",
            "completed": false
        }"#
            .to_string(),
        );
        let res = create_app(todo_repo, LabelRepositoryForMemory::new())
            .oneshot(req)
            .await
            .unwrap();
        let todo = res_to_todo(res).await;
        assert_eq!(expected, todo);
    }

    #[tokio::test]
    async fn should_delete_todo() {
        let todo_repo = TodoRepositoryForMemory::new();
        todo_repo
            .create(CreateTodo::new("should_delete_todo".to_string()))
            .await
            .expect("failed create todo");
        let req = build_req_with_empty(Method::DELETE, "/todos/1");
        let res = create_app(todo_repo, LabelRepositoryForMemory::new())
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(StatusCode::NO_CONTENT, res.status());
    }

    #[tokio::test]
    async fn should_create_label() {
        let expected = Label::new(1, "should create label".to_string());
        let req = build_req_with_json(
            "/labels",
            Method::POST,
            r#"{ "name": "should create label" }"#.to_string(),
        );
        let res = create_app(
            TodoRepositoryForMemory::new(),
            LabelRepositoryForMemory::new(),
        )
        .oneshot(req)
        .await
        .unwrap();

        let label = res_to_label(res).await;
        assert_eq!(expected, label);
    }

    #[tokio::test]
    async fn should_get_all_labels() {
        let expected = vec![Label::new(1, "should get all labels".to_string())];
        let label_repo = LabelRepositoryForMemory::new();
        label_repo
            .create(CreateLabel::new("should get all labels".to_string()))
            .await
            .expect("failed create label");
        let req = build_req_with_empty(Method::GET, "/labels");
        let res = create_app(TodoRepositoryForMemory::new(), label_repo)
            .oneshot(req)
            .await
            .unwrap();

        let bytes = hyper::body::to_bytes(res.into_body()).await.unwrap();
        let body: String = String::from_utf8(bytes.to_vec()).unwrap();
        let labels: Vec<Label> = serde_json::from_str(&body)
            .unwrap_or_else(|_| panic!("cannot convert Label instances. body: {}", body));
        assert_eq!(expected, labels);
    }

    #[tokio::test]
    async fn should_delete_label() {
        let label_repo = LabelRepositoryForMemory::new();
        label_repo
            .create(CreateLabel::new("should delete label".to_string()))
            .await
            .expect("failed create label");
        let req = build_req_with_empty(Method::DELETE, "/labels/1");
        let res = create_app(TodoRepositoryForMemory::new(), label_repo)
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(StatusCode::NO_CONTENT, res.status());
    }
}
