use corust_components::{
    network::{Activity, User, UserId},
    server::ServerError,
};
use rand::Rng;
use random_color::{color_dictionary::ColorDictionary, Color, Luminosity, RandomColor};
use serde::{Deserialize, Serialize};
use warp::{reject, Filter};

use crate::sessions::SharedSessionMap;

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
// There should almost always be enough names in [`NAMES`] to avoid this fallback.
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
struct MaxUsersError;

impl reject::Reject for MaxUsersError {}

#[derive(Debug)]
struct UnexpectedError;

impl reject::Reject for UnexpectedError {}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserJoinResponse {
    user_id: UserId,
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
    let num_users = server.users().len();
    let possible_names: Vec<String> = if num_users < NAMES.len() {
        NAMES.iter().map(|s| s.to_string()).collect()
    } else {
        let mut rustaceans = (0..num_users + 1 - NAMES.len())
            .map(|i| format!("Rustacean {}", RUST_FIRST_EDITION + i * 3))
            .collect::<Vec<_>>();
        rustaceans.extend(NAMES.iter().map(|s| s.to_string()));
        rustaceans
    };
    let mut username_index = rng.gen_range(0..NAMES.len());
    let mut num_usernames_checked = 0;

    // Ensure the username is not already taken
    while server
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

    let existing_colors = server
        .users()
        .values()
        .map(|user| user.color())
        .collect::<Vec<_>>();
    let color = random_color(existing_colors);

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
