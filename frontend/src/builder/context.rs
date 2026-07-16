use std::{
    any::{Any, TypeId},
    fmt::Display,
    sync::Arc,
};

use anyhow::Error;
use common::message::user::BasicInfo;
use fxhash::FxHashMap;
use parking_lot::RwLock;
use sqlx::SqlitePool;

use super::value::Value;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum ContextMode {
    Editing,
    Presenting,
}

/// Where the page being rendered was requested from.
///
/// Page-builder pages became request-dependent once they could hold a
/// [`crate::builder::widget::Widget`]: a widget that filters needs to know what
/// the reader asked for. Keeping that state in the URL is what lets its filter
/// form be an ordinary GET that works with no JavaScript, with HTMX only
/// upgrading the round trip — and the widget needs `path` to point that form
/// back at the page it lives on, since it cannot know its own address.
///
/// Carried as typed extra data rather than a binding so it cannot collide with
/// a component's params.
#[derive(Clone, Debug, Default)]
pub struct RequestUrl {
    pub path: String,
    pub query: String,
}

pub struct Context {
    mode: ContextMode,
    parent: Option<Arc<Context>>,
    db: &'static SqlitePool,
    user: Option<Arc<BasicInfo>>,
    bindings: RwLock<FxHashMap<String, Value>>,
    extra: RwLock<FxHashMap<TypeId, Box<dyn Any + Send + Sync>>>,
}

impl Context {
    /// Create a root context
    pub fn new(
        db: &'static SqlitePool,
        user: &Option<Arc<BasicInfo>>,
        mode: ContextMode,
    ) -> Result<Arc<Context>, Error> {
        let ctx = Context {
            mode,
            parent: None,
            db,
            user: user.clone(),
            bindings: RwLock::new(FxHashMap::default()),
            extra: RwLock::new(FxHashMap::default()),
        };

        if let Some(u) = user {
            ctx.bind_string("user_id", u.user_id.to_string())?;
            ctx.bind_string("user_handle", u.display_handle())?;
            ctx.bind_string("user_staff", u.is_staff || u.is_superuser)?
        }

        Ok(Arc::new(ctx))
    }

    /// Create a child context
    pub fn from_parent(parent: &Arc<Context>) -> Result<Arc<Context>, Error> {
        let ctx = Context {
            mode: parent.mode,
            parent: Some(parent.clone()),
            db: parent.db,
            user: parent.user.clone(),
            bindings: RwLock::new(FxHashMap::default()),
            extra: RwLock::new(FxHashMap::default()),
        };

        Ok(Arc::new(ctx))
    }

    pub fn mode(&self) -> ContextMode {
        self.mode
    }

    pub fn db(&self) -> &'static SqlitePool {
        self.db
    }

    pub fn user(&self) -> Option<Arc<BasicInfo>> {
        self.user.clone()
    }

    /// Bind a template variable
    ///
    /// Shadows but does not replace any same-named bindings in ancestor contexts
    pub fn bind_string(&self, name: impl Into<String>, value: impl Display) -> Result<(), Error> {
        self.bindings
            .write()
            .insert(name.into(), value.to_string().into());
        Ok(())
    }

    /// Bind a template variable to a Value directly
    ///
    /// Shadows but does not replace any same-named bindings in ancestor contexts
    pub fn bind(&self, name: impl Into<String>, value: Value) -> Result<(), Error> {
        self.bindings.write().insert(name.into(), value);
        Ok(())
    }

    /// Unbind a template variable
    ///
    /// Only impacts the self context, bindings in ancestor contexts are not removed
    pub fn unbind(&self, name: impl AsRef<str>) -> Result<(), Error> {
        self.bindings.write().remove(name.as_ref());
        Ok(())
    }

    /// Retrieves the value of a template variable
    ///
    /// Checks in the self context and then recursively in parent contexts until the binding is found or there are no more ancestor contexts
    pub fn value(&self, name: impl AsRef<str>) -> Result<Option<Value>, Error> {
        let guard = self.bindings.read();

        if let Some(v) = guard.get(name.as_ref()) {
            Ok(Some(v.to_owned()))
        } else {
            drop(guard);

            if let Some(ref parent) = self.parent {
                parent.value(name)
            } else {
                Ok(None)
            }
        }
    }

    /// Retrieves whether a value is bound
    pub fn bound(&self, name: impl AsRef<str>) -> Result<bool, Error> {
        Ok(self.value(name)?.is_some())
    }

    /// Retrieves the truthness of a (possibly) bound value
    pub fn truth(&self, name: impl AsRef<str>) -> Result<bool, Error> {
        if let Some(v) = self.value(name)? {
            Ok(v.truth())
        } else {
            Ok(false)
        }
    }

    /// Stores extra typed data in the context
    pub fn set<T>(&self, value: T) -> Result<(), Error>
    where
        T: Any + Clone + Send + Sync + 'static,
    {
        let mut guard = self.extra.write();

        guard.insert(value.type_id(), Box::new(value));

        Ok(())
    }

    /// Retrieves extra typed data from the context
    pub fn get<T>(&self) -> Result<Option<T>, Error>
    where
        T: Any + Clone + Send + Sync + 'static,
    {
        let guard = self.extra.read();

        if let Some(value) = guard.get(&TypeId::of::<T>()) {
            Ok(value.downcast_ref::<T>().map(|v| v.clone()))
        } else {
            drop(guard);

            if let Some(ref parent) = self.parent {
                parent.get()
            } else {
                Ok(None)
            }
        }
    }

    /// Dispatch a function call, optionally updating local bindings
    ///
    /// The args are value names to look up in the context and pass to the function.
    pub async fn dispatch(&self, _name: &str, _args: &[&str]) -> Result<(), Error> {
        // TODO: Implement function registry and dispatch
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;
    use sqlx::SqlitePool;
    use tokio::sync::OnceCell;

    static TEST_POOL: OnceCell<SqlitePool> = OnceCell::const_new();

    async fn test_db() -> &'static SqlitePool {
        TEST_POOL
            .get_or_init(|| async { SqlitePool::connect(":memory:").await.unwrap() })
            .await
    }

    async fn test_ctx() -> Arc<Context> {
        Context::new(test_db().await, &None, ContextMode::Presenting).unwrap()
    }

    #[tokio::test]
    async fn test_bind_and_retrieve() {
        let ctx = test_ctx().await;
        ctx.bind_string("foo", "bar").unwrap();

        let value = ctx.value("foo").unwrap();
        check!(value.is_some());
        check!(value.unwrap().to_string() == "bar");
    }

    #[tokio::test]
    async fn test_bind_value_directly() {
        let ctx = test_ctx().await;
        ctx.bind(
            "items",
            Value::List(vec![Value::from("a"), Value::from("b")]),
        )
        .unwrap();

        let value = ctx.value("items").unwrap().unwrap();
        check!(value.to_string() == "[a, b]");
    }

    #[tokio::test]
    async fn test_unbound_returns_none() {
        let ctx = test_ctx().await;
        let value = ctx.value("missing").unwrap();
        check!(value.is_none());
    }

    #[tokio::test]
    async fn test_bound_check() {
        let ctx = test_ctx().await;
        ctx.bind_string("exists", "yes").unwrap();

        check!(ctx.bound("exists").unwrap());
        check!(!ctx.bound("missing").unwrap());
    }

    #[tokio::test]
    async fn test_truth_truthy_values() {
        let ctx = test_ctx().await;
        ctx.bind_string("yes", "true").unwrap();
        ctx.bind_string("one", "1").unwrap();
        ctx.bind_string("text", "hello").unwrap();

        check!(ctx.truth("yes").unwrap());
        check!(ctx.truth("one").unwrap());
        check!(ctx.truth("text").unwrap());
    }

    #[tokio::test]
    async fn test_truth_falsy_values() {
        let ctx = test_ctx().await;
        ctx.bind_string("empty", "").unwrap();
        ctx.bind_string("false_str", "false").unwrap();

        check!(!ctx.truth("empty").unwrap());
        check!(!ctx.truth("false_str").unwrap());
        check!(!ctx.truth("unbound").unwrap());
    }

    #[tokio::test]
    async fn test_unbind_removes_binding() {
        let ctx = test_ctx().await;
        ctx.bind_string("temp", "value").unwrap();
        check!(ctx.bound("temp").unwrap());

        ctx.unbind("temp").unwrap();
        check!(!ctx.bound("temp").unwrap());
    }

    #[tokio::test]
    async fn test_child_inherits_parent_bindings() {
        let parent = test_ctx().await;
        parent.bind_string("inherited", "from_parent").unwrap();

        let child = Context::from_parent(&parent).unwrap();
        let value = child.value("inherited").unwrap();
        check!(value.is_some());
        check!(value.unwrap().to_string() == "from_parent");
    }

    #[tokio::test]
    async fn test_child_shadows_parent_bindings() {
        let parent = test_ctx().await;
        parent.bind_string("name", "parent_value").unwrap();

        let child = Context::from_parent(&parent).unwrap();
        child.bind_string("name", "child_value").unwrap();

        // Child sees its own value
        check!(child.value("name").unwrap().unwrap().to_string() == "child_value");
        // Parent still sees original value
        check!(parent.value("name").unwrap().unwrap().to_string() == "parent_value");
    }

    #[tokio::test]
    async fn test_child_unbind_does_not_affect_parent() {
        let parent = test_ctx().await;
        parent.bind_string("shared", "value").unwrap();

        let child = Context::from_parent(&parent).unwrap();
        child.bind_string("shared", "child_value").unwrap();
        child.unbind("shared").unwrap();

        // Child now falls back to parent value
        check!(child.value("shared").unwrap().unwrap().to_string() == "value");
        // Parent unaffected
        check!(parent.value("shared").unwrap().unwrap().to_string() == "value");
    }

    #[tokio::test]
    async fn test_extra_typed_data() {
        let ctx = test_ctx().await;

        #[derive(Clone, Debug, PartialEq)]
        struct MyData(i32);

        ctx.set(MyData(42)).unwrap();

        let retrieved: Option<MyData> = ctx.get().unwrap();
        check!(retrieved == Some(MyData(42)));
    }

    #[tokio::test]
    async fn test_extra_typed_data_unset() {
        let ctx = test_ctx().await;

        #[derive(Clone, Debug, PartialEq)]
        struct MyData(i32);

        let retrieved: Option<MyData> = ctx.get().unwrap();
        check!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_extra_typed_data_inherited() {
        let parent = test_ctx().await;

        #[derive(Clone, Debug, PartialEq)]
        struct MyData(i32);

        parent.set(MyData(100)).unwrap();

        let child = Context::from_parent(&parent).unwrap();
        let retrieved: Option<MyData> = child.get().unwrap();
        check!(retrieved == Some(MyData(100)));
    }

    #[tokio::test]
    async fn test_extra_typed_data_shadowed() {
        let parent = test_ctx().await;

        #[derive(Clone, Debug, PartialEq)]
        struct MyData(i32);

        parent.set(MyData(100)).unwrap();

        let child = Context::from_parent(&parent).unwrap();
        child.set(MyData(200)).unwrap();

        let parent_value: Option<MyData> = parent.get().unwrap();
        let child_value: Option<MyData> = child.get().unwrap();

        check!(parent_value == Some(MyData(100)));
        check!(child_value == Some(MyData(200)));
    }

    #[tokio::test]
    async fn test_mode_preserved_in_child() {
        let parent = Context::new(test_db().await, &None, ContextMode::Editing).unwrap();
        let child = Context::from_parent(&parent).unwrap();

        check!(parent.mode() == ContextMode::Editing);
        check!(child.mode() == ContextMode::Editing);
    }

    #[tokio::test]
    async fn test_deep_nesting() {
        let root = test_ctx().await;
        root.bind_string("level", "0").unwrap();

        let level1 = Context::from_parent(&root).unwrap();
        level1.bind_string("level", "1").unwrap();

        let level2 = Context::from_parent(&level1).unwrap();
        level2.bind_string("level", "2").unwrap();

        let level3 = Context::from_parent(&level2).unwrap();
        // level3 doesn't override, should get level2's value

        check!(level3.value("level").unwrap().unwrap().to_string() == "2");
    }
}
