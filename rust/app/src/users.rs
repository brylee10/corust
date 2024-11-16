use std::path::PathBuf;

use corust_components::{
    network::{Activity, User, UserId},
    server::ServerError,
};
use rand::Rng;
use random_color::{color_dictionary::ColorDictionary, Color, Luminosity, RandomColor};
use serde::{Deserialize, Serialize};
use warp::{path, reject, Filter};

use crate::{
    db::{DocumentTable, DocumentTableKey, Table, UserTable, UserTableKey},
    sessions::{SharedSession, SharedSessionMap},
};

/// Possible user names to sample from. Common Rust crates and terms.
const NAMES: [&str; 50] = [
    "Ferris",
    "Serde",
    "Syn",
    "Quote",
    "Tokio",
    "Anyhow",
    "Chrono",
    "Fnv",
    "Crossbeam",
    "Strum",
    "Rayon",
    "Prost",
    "Bindgen",
    "Rtrb",
    "Rand",
    "ProcMacro2",
    "Warp",
    "Hashbrown",
    "Reqwest",
    "Zstd",
    "Memchr",
    "OnceCell",
    "LazyStatic",
    "Indexmap",
    "Thiserror",
    "Memoffset",
    "Socket2",
    "Mio",
    "Slab",
    "Futures",
    "Ahash",
    "Tracing",
    "PinUtils",
    "Hyper",
    "Tinyvec",
    "Spin",
    "Tempfile",
    "Nom",
    "Fastrand",
    "Nix",
    "EnvLogger",
    "Rustix",
    "H2",
    "Adler",
    "Flate2",
    "Either",
    "Humantime",
    "Instant",
    "StaticAssertions",
    "StructOpt",
];

// When [`NAMES`] is exhausted, generate user names `Rustacean {edition}` starting from this rust edition.
// For practical purposes, there should almost always be enough names in [`NAMES`] to avoid this fallback.
const RUST_FIRST_EDITION: usize = 2015;

// Possible color hues to sample from (does not allow monochrome or yellow)
const COLOR_SAMPLE: [Color; 6] = [
    Color::Red,
    Color::Orange,
    Color::Green,
    Color::Blue,
    Color::Purple,
    Color::Pink,
];

#[derive(Debug)]
struct DuplicateUserError {
    // The field is used in the custom warp Rejection
    #[allow(dead_code)]
    user_id: UserId,
}

impl reject::Reject for DuplicateUserError {}

#[derive(Debug)]
struct UnexpectedError;

impl reject::Reject for UnexpectedError {}

#[derive(Debug)]
struct DbError {
    // The field is used in the custom warp Rejection
    #[allow(dead_code)]
    error: String,
}

impl reject::Reject for DbError {}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserJoinResponse {
    user_id: UserId,
    username: String,
}

// Parse color in the format "rgb({}, {}, {})"
fn parse_color(color: &str) -> (f64, f64, f64) {
    let color = color.trim_start_matches("rgb(").trim_end_matches(')');
    let parts: Vec<&str> = color.split(',').collect();
    if parts.len() != 3 {
        panic!("Invalid color format");
    }

    let r = parts[0].trim().parse::<f64>().expect("Invalid red value");
    let g = parts[1].trim().parse::<f64>().expect("Invalid green value");
    let b = parts[2].trim().parse::<f64>().expect("Invalid blue value");

    (r, g, b)
}

// Calculate Euclidean distance between two rgb colors
fn color_distance(color1: &str, color2: &str) -> f64 {
    let (r1, g1, b1) = parse_color(color1);
    let (r2, g2, b2) = parse_color(color2);

    let dr = r1 - r2;
    let dg = g1 - g2;
    let db = b1 - b2;

    (dr * dr + dg * dg + db * db).sqrt()
}

// Returns a random color that is most different from all existing colors as a rgb string
fn random_color(existing_colors: Vec<&str>) -> String {
    let colors = COLOR_SAMPLE
        .iter()
        .map(|hue| {
            RandomColor::new()
                .hue(*hue)
                .luminosity(Luminosity::Dark) // Optional
                .alpha(1.0) // Optional
                .dictionary(ColorDictionary::new())
                .to_rgb_string()
        })
        .collect::<Vec<String>>();

    // Return generated color that is most different from all existing colors
    let mut max_distance = 0.0;
    let mut most_different_color = colors[0].clone();

    for color in colors {
        let distance = existing_colors
            .iter()
            .map(|&existing_color| color_distance(&color, existing_color))
            .sum();

        if distance > max_distance {
            max_distance = distance;
            most_different_color = color;
        }
    }

    most_different_color
}

// Groups commands to create new session and add session to the database.
// If the session already exists in the database then it is retrieved.
async fn get_or_create_session(
    session_map: &SharedSessionMap,
    session_id: &str,
    db_path: PathBuf,
) -> Result<SharedSession, warp::Rejection> {
    let document_table = DocumentTable::new(db_path.clone());
    let document_key = DocumentTableKey {
        session_id: session_id.to_string(),
    };
    // Query if document exists in the database
    let document_state = document_table.get_all(session_id).unwrap();

    // The only location a session is created
    // Load a session from the database if it is present and not already in memory
    let session = if session_map.get_session(session_id).is_none() && !document_state.is_empty() {
        log::debug!("Loading session with id {} from database", session_id);
        // Returned document states should have exactly one state for a given session id
        debug_assert!(document_state.len() == 1);
        session_map.get_or_create_session_with_document_state(session_id, document_state[0].clone())
    } else {
        log::debug!("Creating a new empty session with id {}", session_id);
        session_map.get_or_create_session(session_id)
    };
    let current_document_state = session
        .server()
        .read()
        .await
        .current_document_state()
        .clone();
    if let Err(e) = document_table.insert_or_update(document_key, current_document_state) {
        log::error!("Error inserting or updating document: {}", e);
        return Err(warp::reject::custom(DbError {
            error: e.to_string(),
        }));
    }
    Ok(session)
}

// Groups commands to add user to the server and database
async fn add_user(
    session: SharedSession,
    user: User,
    db_path: PathBuf,
) -> Result<(), warp::Rejection> {
    let user_table = UserTable::new(db_path);
    let user_key = UserTableKey {
        session_id: session.session_id().to_string(),
        user_id: user.user_id(),
    };
    session
        .server()
        .write()
        .await
        .add_user(user.clone())
        .map_err(|err| {
            log::error!("Error adding user to server: {err:?}");
            match err {
                ServerError::DuplicateUserId(user_id) => {
                    warp::reject::custom(DuplicateUserError { user_id })
                }
                _ => warp::reject::custom(UnexpectedError),
            }
        })?;

    if let Err(e) = user_table.insert_or_update(user_key, user) {
        log::error!("Error inserting or updating user: {}", e);
        return Err(warp::reject::custom(DbError {
            error: e.to_string(),
        }));
    }
    Ok(())
}

async fn handle_user_join(
    session_map: SharedSessionMap,
    session_id: String,
    user_id: Option<UserId>,
    db_path: PathBuf,
) -> Result<impl warp::Reply, warp::Rejection> {
    let session = get_or_create_session(&session_map, &session_id, db_path.clone()).await?;
    let server = session.server();
    log::debug!("User join request with ID: {:?}", user_id);
    let user_id = match user_id {
        Some(user_id) => {
            // Will try to rejoin with the same user_id
            if let Some(user) = server.write().await.users_mut().get_mut(&user_id) {
                user.activity.active = true;
                user.activity.last_activity = std::time::Instant::now();
                log::debug!("User rejoining with ID: {:?}", user_id);
                return Ok(warp::reply::json(&UserJoinResponse {
                    user_id,
                    username: user.username().to_string(),
                }));
            }
            // If not present, then will join with a new user_id
            server.write().await.next_user_id()
        }
        None => {
            // If not present, then will join with a new user_id
            server.write().await.next_user_id()
        }
    };
    log::debug!("User join with new ID: {:?}", user_id);

    let num_users = server.read().await.users().len();
    let possible_names: Vec<String> = if num_users < NAMES.len() {
        NAMES.iter().map(|s| s.to_string()).collect()
    } else {
        let mut rustaceans = (0..num_users + 1 - NAMES.len())
            .map(|i| format!("Rustacean {}", RUST_FIRST_EDITION + i * 3))
            .collect::<Vec<_>>();
        rustaceans.extend(NAMES.iter().map(|s| s.to_string()));
        rustaceans
    };
    let mut username_index = {
        let mut rng = rand::thread_rng();
        rng.gen_range(0..NAMES.len())
    };
    let mut num_usernames_checked = 0;

    // Ensure the username is not already taken
    while server
        .read()
        .await
        .users()
        .values()
        .any(|user| user.username() == possible_names[username_index])
    {
        username_index = (username_index + 1) % possible_names.len();
        num_usernames_checked += 1;
        if num_usernames_checked >= possible_names.len() {
            panic!("All possible usernames are taken - this should be unreachable");
        }
    }
    let username = &possible_names[username_index];

    let color = {
        let server = server.read().await;
        let existing_colors = server
            .users()
            .values()
            .map(|user| user.color())
            .collect::<Vec<_>>();
        random_color(existing_colors)
    };

    let activity = Activity {
        active: true,
        last_activity: std::time::Instant::now(),
    };
    let user = User::new(user_id, username.to_string(), color, activity);
    add_user(session.clone(), user, db_path.clone()).await?;

    Ok(warp::reply::json(&UserJoinResponse {
        user_id,
        username: username.to_string(),
    }))
}

pub fn user_join_route(
    session_map: SharedSessionMap,
    db_path: PathBuf,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let prefix = path!("join" / String / ..);
    let opt = warp::path::param::<UserId>().map(Some).or_else(|_| async {
        Ok::<(std::option::Option<UserId>,), std::convert::Infallible>((None,))
    });
    prefix
        .and(opt)
        .and_then(move |session_id: String, user_id: Option<UserId>| {
            handle_user_join(session_map.clone(), session_id, user_id, db_path.clone())
        })
}
