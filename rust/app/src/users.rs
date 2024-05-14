use corust_components::{
    network::{Activity, User, UserId},
    server::ServerError,
};
use rand::Rng;
use random_color::{color_dictionary::ColorDictionary, Luminosity, RandomColor};
use serde::{Deserialize, Serialize};
use warp::{reject, Filter};

use crate::sessions::SharedSessionMap;

// Sessions currently support up to 20 unique users
// For realistic use cases, this should be sufficient
const NAMES: [&str; 18] = [
    "Rustacean 2015",
    "Rustacean 2018",
    "Rustacean 2021",
    "Ferris",
    "Serde",
    "Syn",
    "Quote",
    "Tokio",
    "Anyhow",
    "Vec",
    "Chrono",
    "Fnv",
    "Crossbeam",
    "Strum",
    "Rayon",
    "Prost",
    "Bindgen",
    "Rtrb",
];

#[derive(Debug)]
struct DuplicateUserError {
    // The field is used in the custom warp Rejection
    #[allow(dead_code)]
    user_id: UserId,
}

impl reject::Reject for DuplicateUserError {}

#[derive(Debug)]
struct MaxUsersError;

impl reject::Reject for MaxUsersError {}

#[derive(Debug)]
struct UnexpectedError;

impl reject::Reject for UnexpectedError {}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserJoinResponse {
    user_id: UserId,
}

fn random_color() -> String {
    let color = RandomColor::new()
        .luminosity(Luminosity::Bright) // Optional
        .alpha(1.0) // Optional
        .dictionary(ColorDictionary::new())
        .to_rgb_string();
    color
}

async fn handle_user_join(
    session_map: SharedSessionMap,
    session_id: String,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut session_map = session_map.lock().await;
    // The only location a session is created
    let session = session_map.get_or_create_session(&session_id);
    let session = session.lock().await;
    let server = session.server();
    let mut server = server.lock().await;
    let user_id = server.next_user_id();

    let mut rng = rand::thread_rng();
    let mut username_index = rng.gen_range(0..NAMES.len());
    let mut num_usernames_checked = 0;
    // Ensure the username is not already taken
    while server
        .users()
        .values()
        .any(|user| user.username() == NAMES[username_index])
    {
        username_index = (username_index + 1) % NAMES.len();
        num_usernames_checked += 1;
        if num_usernames_checked >= NAMES.len() {
            return Err(warp::reject::custom(MaxUsersError));
        }
    }
    let username = NAMES[username_index];
    let color = random_color();

    let activity = Activity {
        active: true,
        last_activity: std::time::Instant::now(),
    };
    let user = User::new(user_id, username.to_string(), color, activity);
    server.add_user(user).map_err(|err| {
        log::error!("Error adding user to server: {err:?}");
        match err {
            ServerError::DuplicateUserId(user_id) => {
                warp::reject::custom(DuplicateUserError { user_id })
            }
            _ => warp::reject::custom(UnexpectedError),
        }
    })?;
    Ok(warp::reply::json(&UserJoinResponse { user_id }))
}

pub(crate) fn user_join_route(
    session_map: SharedSessionMap,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("join")
        .and(warp::path::param())
        .and_then(move |session_id: String| handle_user_join(session_map.clone(), session_id))
}
