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

pub struct Context {
    mode: ContextMode,
    parent: Option<Arc<Context>>,
    db: &'static SqlitePool,
    user: Option<Arc<BasicInfo>>,
    bindings: RwLock<FxHashMap<String, Value>>,
    extra: RwLock<FxHashMap<TypeId, Box<dyn Any>>>,
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
            ctx.bind("user_id", u.user_id.to_string())?;
            ctx.bind("user_handle", u.display_handle())?;
            ctx.bind("user_staff", u.is_staff || u.is_superuser)?
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
    pub fn bind(&self, name: impl Into<String>, value: impl Display) -> Result<(), Error> {
        self.bindings
            .write()
            .insert(name.into(), value.to_string().into());
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
        T: Any + Clone + 'static,
    {
        let mut guard = self.extra.write();

        guard.insert(value.type_id(), Box::new(value));

        Ok(())
    }

    /// Retrieves extra typed data from the context
    pub fn get<T>(&self) -> Result<Option<T>, Error>
    where
        T: Any + Clone + 'static,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    #[test]
    fn test_binding_simple() {}
}
