use super::RepositoryError;
use crate::repositories::label::Label;
use axum::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use validator::Validate;

#[async_trait]
pub trait TodoRepository: Clone + Send + Sync + 'static {
    async fn create(&self, payload: CreateTodo) -> anyhow::Result<TodoEntity>;
    async fn find(&self, id: i32) -> anyhow::Result<TodoEntity>;
    async fn all(&self) -> anyhow::Result<Vec<TodoEntity>>;
    async fn update(&self, id: i32, payload: UpdateTodo) -> anyhow::Result<TodoEntity>;
    async fn delete(&self, id: i32) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Eq, PartialEq, FromRow)]
pub struct TodoWithLabelFromRow {
    id: i32,
    text: String,
    completed: bool,
    label_id: Option<i32>,
    label_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
struct TodoFromRow {
    id: i32,
    text: String,
    completed: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct TodoEntity {
    pub id: i32,
    pub text: String,
    pub completed: bool,
    pub labels: Vec<Label>,
}

fn fold_entities(rows: Vec<TodoWithLabelFromRow>) -> Vec<TodoEntity> {
    let mut rows = rows.iter();
    let mut accum: Vec<TodoEntity> = vec![];
    'outer: while let Some(row) = rows.next() {
        let mut todos = accum.iter_mut();
        while let Some(todo) = todos.next() {
            if todo.id == row.id {
                todo.labels.push(Label {
                    id: row.label_id.unwrap(),
                    name: row.label_name.clone().unwrap(),
                });

                continue 'outer;
            }
        }

        let labels = if row.label_id.is_some() {
            vec![Label {
                id: row.label_id.unwrap(),
                name: row.label_name.clone().unwrap(),
            }]
        } else {
            vec![]
        };

        accum.push(TodoEntity {
            id: row.id,
            text: row.text.clone(),
            completed: row.completed,
            labels,
        });
    }

    accum
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Validate)]
pub struct CreateTodo {
    #[validate(length(min = 1, message = "Can not be empty"))]
    #[validate(length(max = 100, message = "Over text length"))]
    text: String,
    labels: Vec<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Validate)]
pub struct UpdateTodo {
    #[validate(length(min = 1, message = "Can not be empty"))]
    #[validate(length(max = 100, message = "Over text length"))]
    text: Option<String>,
    completed: Option<bool>,
    labels: Option<Vec<i32>>,
}

#[derive(Clone)]
pub struct TodoRepositoryForDb {
    pool: PgPool,
}

impl TodoRepositoryForDb {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TodoRepository for TodoRepositoryForDb {
    async fn create(&self, payload: CreateTodo) -> anyhow::Result<TodoEntity> {
        let tx = self.pool.begin().await?;
        let row = sqlx::query_as::<_, TodoFromRow>(
            r#"INSERT INTO todos (text, completed) VALUES ($1, false) RETURNING *;"#,
        )
        .bind(payload.text.clone())
        .fetch_one(&self.pool)
        .await?;

        sqlx::query(r#"INSERT INTO todo_labels (todo_id, label_id) SELECT $1, id FROM unnest($2) as t(id);"#)
            .bind(row.id)
            .bind(payload.labels)
            .execute(&self.pool)
            .await?;
        tx.commit().await?;

        let todo = self.find(row.id).await?;
        Ok(todo)
    }

    async fn find(&self, id: i32) -> anyhow::Result<TodoEntity> {
        let items = sqlx::query_as::<_, TodoWithLabelFromRow>(
            r#"
        SELECT todos.*, labels.id as label_id, labels.name as label_name FROM todos 
        LEFT OUTER JOIN todo_labels t1 on todos.id = t1.todo_id
        LEFT OUTER JOIN labels on labels.id = t1.label_id
        WHERE todos.id = $1;"#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => RepositoryError::NotFound(id),
            _ => RepositoryError::Unexpected(e.to_string()),
        })?;

        let todos = fold_entities(items);
        let todo = todos.first().ok_or(RepositoryError::NotFound(id))?;
        Ok(todo.clone())
    }

    async fn all(&self) -> anyhow::Result<Vec<TodoEntity>> {
        let items = sqlx::query_as::<_, TodoWithLabelFromRow>(
            r#"
        SELECT todos.*, labels.id as label_id, labels.name as label_name FROM todos 
        LEFT OUTER JOIN todo_labels t1 on todos.id = t1.todo_id
        LEFT OUTER JOIN labels on labels.id = t1.label_id
        ORDER BY id desc;"#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(fold_entities(items))
    }

    async fn update(&self, id: i32, payload: UpdateTodo) -> anyhow::Result<TodoEntity> {
        let tx = self.pool.begin().await?;
        let old_todo = self.find(id).await?;
        sqlx::query(r#"UPDATE todos SET text = $1, completed = $2 WHERE id = $3"#)
            .bind(payload.text.unwrap_or(old_todo.text))
            .bind(payload.completed.unwrap_or(old_todo.completed))
            .bind(id)
            .execute(&self.pool)
            .await?;

        if let Some(labels) = payload.labels {
            sqlx::query(r#"DELETE FROM todo_labels WHERE todo_id = $1"#)
                .bind(id)
                .execute(&self.pool)
                .await?;
            sqlx::query(r#"INSERT INTO todo_labels (todo_id, label_id) SELECT $1, id FROM unnest($2) as t(id);"#)
                .bind(id)
                .bind(labels)
                .execute(&self.pool)
                .await?;
        };
        tx.commit().await?;

        let todo = self.find(id).await?;
        Ok(todo)
    }

    async fn delete(&self, id: i32) -> anyhow::Result<()> {
        let tx = self.pool.begin().await?;
        sqlx::query(r#"DELETE FROM todo_labels WHERE todo_id = $1"#)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => RepositoryError::NotFound(id),
                _ => RepositoryError::Unexpected(e.to_string()),
            })?;

        sqlx::query(r#"DELETE FROM todos WHERE id = $1"#)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => RepositoryError::NotFound(id),
                _ => RepositoryError::Unexpected(e.to_string()),
            })?;
        tx.commit().await?;

        Ok(())
    }
}

#[cfg(test)]
#[cfg(feature = "database-test")]
mod test {
    use super::*;
    use dotenv::dotenv;
    use sqlx::PgPool;
    use std::env;

    #[test]
    fn fold_entities_test() {
        let label_1 = Label {
            id: 1,
            name: "Label 1".to_string(),
        };
        let label_2 = Label {
            id: 2,
            name: "Label 2".to_string(),
        };

        let rows = vec![
            TodoWithLabelFromRow {
                id: 1,
                text: "Todo 1".to_string(),
                completed: false,
                label_id: Some(label_1.id),
                label_name: Some(label_1.name.clone()),
            },
            TodoWithLabelFromRow {
                id: 1,
                text: "Todo 1".to_string(),
                completed: false,
                label_id: Some(label_2.id),
                label_name: Some(label_2.name.clone()),
            },
            TodoWithLabelFromRow {
                id: 2,
                text: "Todo 2".to_string(),
                completed: false,
                label_id: Some(label_1.id),
                label_name: Some(label_1.name.clone()),
            },
        ];
        let res = fold_entities(rows);
        assert_eq!(
            res,
            vec![
                TodoEntity {
                    id: 1,
                    text: "Todo 1".to_string(),
                    completed: false,
                    labels: vec![label_1.clone(), label_2.clone()]
                },
                TodoEntity {
                    id: 2,
                    text: "Todo 2".to_string(),
                    completed: false,
                    labels: vec![label_1.clone()]
                },
            ]
        );
    }

    #[tokio::test]
    async fn crud_scenario() {
        dotenv().ok();
        let database_url = &env::var("DATABASE_URL").expect("undefined [DATABASE_URL]");
        let pool = PgPool::connect(database_url)
            .await
            .expect(&format!("fail connect database. url is [{}]", database_url));

        // label data prepare
        let label_name = "test label".to_string();
        let optional_label = sqlx::query_as::<_, Label>(r#"SELECT * FROM labels WHERE name = $1"#)
            .bind(label_name.clone())
            .fetch_optional(&pool)
            .await
            .expect("Failed to prepare label data.");
        let label_1 = if let Some(label) = optional_label {
            label
        } else {
            let label = sqlx::query_as::<_, Label>(
                r#"INSERT INTO labels ( name ) VALUES ( $1 ) RETURNING *"#,
            )
            .bind(label_name)
            .fetch_one(&pool)
            .await
            .expect("Failed to insert label data");

            label
        };

        let repo = TodoRepositoryForDb::new(pool.clone());
        let todo_text = "[crud_scenario] text";

        // create
        let created = repo
            .create(CreateTodo::new(todo_text.to_string(), vec![label_1.id]))
            .await
            .expect("[create] returned Err");
        assert_eq!(created.text, todo_text);
        assert!(!created.completed);
        assert_eq!(*created.labels.first().unwrap(), label_1);

        // find
        let todo = repo.find(created.id).await.expect("[find] returned Err");
        assert_eq!(created, todo);

        // all
        let todos = repo.all().await.expect("[all] returned Err");
        let todo = todos.first().unwrap();
        assert_eq!(created, *todo);

        // update
        let updated_text = "[crud_scenario] updated text";
        let todo = repo
            .update(
                todo.id,
                UpdateTodo::new(Some(updated_text.to_string()), Some(true), Some(vec![])),
            )
            .await
            .expect("[update] returned Err");
        assert_eq!(created.id, todo.id);
        assert_eq!(todo.text, updated_text);

        // delete
        repo.delete(todo.id).await.expect("[delete] returned Err");
        let res = repo.find(created.id).await;
        assert!(res.is_err());

        let todo_rows = sqlx::query(r#"SELECT * FROM todos WHERE id = $1"#)
            .bind(todo.id)
            .fetch_all(&pool)
            .await
            .expect("[delete] todo_labels fetch error");
        assert_eq!(todo_rows.len(), 0);
    }
}

#[cfg(test)]
pub mod test_utils {
    use super::*;
    use anyhow::Context;
    use axum::async_trait;
    use std::{
        collections::HashMap,
        sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    };

    impl TodoEntity {
        pub fn new(id: i32, text: String, completed: bool, labels: Vec<Label>) -> Self {
            Self {
                id,
                text,
                completed,
                labels,
            }
        }
    }

    impl CreateTodo {
        pub fn new(text: String, labels: Vec<i32>) -> Self {
            Self { text, labels }
        }
    }

    impl UpdateTodo {
        pub fn new(
            text: Option<String>,
            completed: Option<bool>,
            labels: Option<Vec<i32>>,
        ) -> Self {
            Self {
                text,
                completed,
                labels,
            }
        }
    }

    type TodoDatas = HashMap<i32, TodoEntity>;

    #[derive(Debug, Clone)]
    pub struct TodoRepositoryForMemory {
        store: Arc<RwLock<TodoDatas>>,
        labels: Vec<Label>,
    }

    impl TodoRepositoryForMemory {
        pub fn new(labels: Vec<Label>) -> Self {
            TodoRepositoryForMemory {
                store: Arc::default(),
                labels,
            }
        }

        fn write_store_ref(&self) -> RwLockWriteGuard<TodoDatas> {
            self.store.write().unwrap()
        }

        fn read_store_ref(&self) -> RwLockReadGuard<TodoDatas> {
            self.store.read().unwrap()
        }

        fn resolve_labels(&self, labels: Vec<i32>) -> Vec<Label> {
            let mut label_list = self.labels.iter().cloned();
            let labels = labels
                .iter()
                .map(|id| label_list.find(|label| label.id == *id).unwrap())
                .collect();
            labels
        }
    }

    #[async_trait]
    impl TodoRepository for TodoRepositoryForMemory {
        async fn create(&self, payload: CreateTodo) -> anyhow::Result<TodoEntity> {
            let mut store = self.write_store_ref();
            let id = (store.len() + 1) as i32;
            let labels = self.resolve_labels(payload.labels);
            let todo = TodoEntity::new(id, payload.text.clone(), false, labels);
            store.insert(id, todo.clone());
            Ok(todo)
        }

        async fn find(&self, id: i32) -> anyhow::Result<TodoEntity> {
            let store = self.read_store_ref();
            let todo = store
                .get(&id)
                .cloned()
                .ok_or(RepositoryError::NotFound(id))?;
            Ok(todo)
        }

        async fn all(&self) -> anyhow::Result<Vec<TodoEntity>> {
            let store = self.read_store_ref();
            Ok(Vec::from_iter(store.values().map(|todo| todo.clone())))
        }

        async fn update(&self, id: i32, payload: UpdateTodo) -> anyhow::Result<TodoEntity> {
            let mut store = self.write_store_ref();
            let todo = store.get(&id).context(RepositoryError::NotFound(id))?;
            let text = payload.text.unwrap_or(todo.text.clone());
            let completed = payload.completed.unwrap_or(todo.completed);
            let labels = match payload.labels {
                Some(label_ids) => self.resolve_labels(label_ids),
                None => todo.labels.clone(),
            };
            let todo = TodoEntity::new(id, text, completed, labels);
            store.insert(id, todo.clone());

            Ok(todo)
        }

        async fn delete(&self, id: i32) -> anyhow::Result<()> {
            let mut store = self.write_store_ref();
            store.remove(&id).ok_or(RepositoryError::NotFound(id))?;
            Ok(())
        }
    }

    mod test {
        use super::*;

        #[tokio::test]
        async fn todo_crud_scenario() {
            let label_data = Label::new(1, "test label".to_string());
            let labels = vec![label_data.clone()];
            let id = 1;
            let text = "todo text".to_string();
            let repo = TodoRepositoryForMemory::new(labels.clone());

            // create
            let expected = TodoEntity::new(id, text.clone(), false, labels.clone());
            let todo = repo
                .create(CreateTodo::new(text, vec![label_data.id]))
                .await
                .expect("failed create todo");
            assert_eq!(expected, todo);

            // find
            let todo = repo.find(todo.id).await.unwrap();
            assert_eq!(expected, todo);

            // all
            let todos = repo.all().await.expect("failed get all todos");
            assert_eq!(vec![expected], todos);

            // update
            let text = "update todo text".to_string();
            let todo = repo
                .update(1, UpdateTodo::new(Some(text.clone()), Some(true), None))
                .await
                .expect("failed update todo.");
            assert_eq!(
                TodoEntity {
                    id,
                    text,
                    completed: true,
                    labels: labels.clone()
                },
                todo
            );

            // delete
            let res = repo.delete(id).await;
            assert!(res.is_ok());
        }
    }
}
