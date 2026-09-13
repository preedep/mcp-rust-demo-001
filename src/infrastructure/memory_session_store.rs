use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, PoisonError};

use uuid::Uuid;

use crate::application::ports::SessionStore;
use crate::domain::SessionId;

/// In-memory store. A restart invalidates every session, which is acceptable here:
/// clients re-`initialize` when they are told the session is unknown.
#[derive(Default)]
pub struct MemorySessionStore {
    ids: Mutex<HashSet<String>>,
}

impl MemorySessionStore {
    /// A panicking handler must not poison the server for every later request.
    fn lock(&self) -> MutexGuard<'_, HashSet<String>> {
        self.ids.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SessionStore for MemorySessionStore {
    fn create(&self) -> SessionId {
        let id = Uuid::new_v4().to_string();
        self.lock().insert(id.clone());
        SessionId::new(id)
    }

    fn exists(&self, id: &SessionId) -> bool {
        self.lock().contains(id.as_str())
    }

    fn remove(&self, id: &SessionId) -> bool {
        self.lock().remove(id.as_str())
    }

    fn count(&self) -> usize {
        self.lock().len()
    }
}
