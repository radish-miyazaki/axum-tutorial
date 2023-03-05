use crate::repositories::RepositoryError;
use axum::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use validator::Validate;

#[async_trait]
pub trait LabelRepository: Clone + Send + Sync + 'static {
    async fn create(&self, payload: CreateLabel) -> anyhow::Result<Label>;
    async fn all(&self) -> anyhow::Result<Vec<Label>>;
    async fn delete(&self, id: i32) -> anyhow::Result<()>;
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Label {
    pub id: i32,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Validate)]
pub struct CreateLabel {
    #[validate(length(min = 1, message = "Cannot be empty"))]
    #[validate(length(max = 100, message = "Over text length"))]
    name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, sqlx::FromRow)]
pub struct UpdateLabel {
    id: i32,
    name: String,
}

#[derive(Debug, Clone)]
pub struct LabelRepositoryForDb {
    pool: PgPool,
}

impl LabelRepositoryForDb {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl LabelRepository for LabelRepositoryForDb {
    async fn create(&self, payload: CreateLabel) -> anyhow::Result<Label> {
        let optional_label = sqlx::query_as::<_, Label>(r#"SELECT * FROM labels WHERE name = $1"#)
            .bind(payload.name.clone())
            .fetch_optional(&self.pool)
            .await?;

        if let Some(label) = optional_label {
            return Err(RepositoryError::Duplicate(label.id).into());
        }

        let label =
            sqlx::query_as::<_, Label>(r#"INSERT INTO labels (name) VALUES ($1) RETURNING *"#)
                .bind(payload.name.clone())
                .fetch_one(&self.pool)
                .await?;
        Ok(label)
    }

    async fn all(&self) -> anyhow::Result<Vec<Label>> {
        let labels = sqlx::query_as::<_, Label>(r#"SELECT * FROM labels ORDER BY labels.id ASC"#)
            .fetch_all(&self.pool)
            .await?;
        Ok(labels)
    }

    async fn delete(&self, id: i32) -> anyhow::Result<()> {
        sqlx::query(r#"DELETE FROM labels WHERE id = $1"#)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => RepositoryError::NotFound(id),
                _ => RepositoryError::Unexpected(e.to_string()),
            })?;

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

    #[tokio::test]
    async fn label_crud_scenario() {
        dotenv().ok();
        let database_url = &env::var("DATABASE_URL").expect("undefined [DATABASE_URL]");
        let pool = PgPool::connect(database_url)
            .await
            .expect(&format!("fail connect database. url is [{}]", database_url));
        let repo = LabelRepositoryForDb::new(pool.clone());
        let label_text = "test_label";

        // create
        let label = repo
            .create(CreateLabel::new(label_text.to_string()))
            .await
            .expect("[create] returned Err");
        assert_eq!(label.name, label_text);

        // all
        let labels = repo.all().await.expect("[all] returned Err");
        let label = labels.last().unwrap();
        assert_eq!(label.name, label_text);

        // delete
        repo.delete(label.id).await.expect("[delete] returned Err");
    }
}

#[cfg(test)]
pub mod test_utils {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

    impl Label {
        pub fn new(id: i32, name: String) -> Self {
            Self { id, name }
        }
    }

    impl CreateLabel {
        pub fn new(name: String) -> Self {
            Self { name }
        }
    }

    type LabelDatas = HashMap<i32, Label>;

    #[derive(Debug, Clone)]
    pub struct LabelRepositoryForMemory {
        store: Arc<RwLock<LabelDatas>>,
    }

    impl LabelRepositoryForMemory {
        pub fn new() -> Self {
            Self {
                store: Arc::default(),
            }
        }

        fn write_store_ref(&self) -> RwLockWriteGuard<LabelDatas> {
            self.store.write().unwrap()
        }

        fn read_store_ref(&self) -> RwLockReadGuard<LabelDatas> {
            self.store.read().unwrap()
        }
    }

    #[async_trait]
    impl LabelRepository for LabelRepositoryForMemory {
        async fn create(&self, payload: CreateLabel) -> anyhow::Result<Label> {
            let mut store = self.write_store_ref();
            if let Some((_key, label)) = store
                .iter()
                .find(|(_key, label)| label.name == payload.name)
            {
                return Ok(label.clone());
            };

            let id = (store.len() + 1) as i32;
            let label = Label::new(id, payload.name.clone());
            store.insert(id, label.clone());
            Ok(label)
        }

        async fn all(&self) -> anyhow::Result<Vec<Label>> {
            let store = self.read_store_ref();
            Ok(Vec::from_iter(store.values().cloned()))
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
        async fn label_crud_scenario() {
            let text = "label text".to_string();
            let id = 1;
            let expected = Label::new(id, text.clone());

            // create
            let repo = LabelRepositoryForMemory::new();
            let label = repo
                .create(CreateLabel::new(text.clone()))
                .await
                .expect("failed label create");
            assert_eq!(expected, label);

            // all
            let label = repo.all().await.unwrap();
            assert_eq!(vec![expected], label);

            // delete
            let res = repo.delete(id).await;
            assert!(res.is_ok());
        }
    }
}
