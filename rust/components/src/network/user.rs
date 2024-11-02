use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Unique user id per session
pub type UserId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    inner: UserInner,
    // Users are marked inactive if they do not respond to PING requests
    // within an interval or when they disconnect from the server
    pub activity: Activity,
}

impl User {
    pub fn new(user_id: UserId, username: String, color: String, activity: Activity) -> Self {
        let user_inner = UserInner::new(user_id, username, color);
        User {
            inner: user_inner,
            activity,
        }
    }
}

impl User {
    pub fn user_id(&self) -> UserId {
        self.inner.user_id
    }

    pub fn username(&self) -> &str {
        &self.inner.username
    }

    pub fn color(&self) -> &str {
        &self.inner.color
    }
}

#[wasm_bindgen]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserInner {
    user_id: UserId,
    username: String,
    color: String,
}

#[wasm_bindgen]
impl UserInner {
    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    pub fn username(&self) -> String {
        // WASM cannot take rust references
        self.username.clone()
    }

    pub fn color(&self) -> String {
        // WASM cannot take rust references
        self.color.clone()
    }
}

impl UserInner {
    pub fn new(user_id: UserId, username: String, color: String) -> Self {
        UserInner {
            user_id,
            username,
            color,
        }
    }
}

/// Immutable snapshot of the users in the server at a point in time.
#[wasm_bindgen]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserList {
    users: Vec<UserInner>,
}

impl UserList {
    pub fn new<T: IntoIterator<Item = User>>(users: T) -> Self {
        let users = users.into_iter().map(|user| user.inner).collect();
        UserList { users }
    }
}

#[wasm_bindgen]
impl UserList {
    pub fn users(&self) -> Vec<UserInner> {
        // WASM cannot take rust slices
        self.users.clone()
    }

    // Ignore lint because this is a wasm_bindgen function
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[derive(Debug, Clone)]
pub struct Activity {
    pub active: bool,
    // Time of last received pong or user creation time
    pub last_activity: std::time::Instant,
}

impl PartialEq for Activity {
    fn eq(&self, other: &Self) -> bool {
        // A epsilon to account for rounding errors when computing the last activity time
        // (e.g. when [de]serializing from the database)
        const ACTIVITY_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(1);
        self.active == other.active
            && (self.last_activity - other.last_activity) < ACTIVITY_THRESHOLD
    }
}

impl Eq for Activity {}
