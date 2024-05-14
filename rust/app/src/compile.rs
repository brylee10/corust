use std::{
    path::Path,
    process::{Command, Output},
};

use corust_components::RunnerOutput;
use warp::Filter;

use crate::messages::{CodeContainer, SharedAppState};

pub(crate) async fn compile_code(code: String, state: SharedAppState) -> impl warp::Reply {
    let join_handle_result = tokio::task::spawn_blocking({
        let code = code.clone();
        move || -> Output {
            // Create a file with this code inside a sandbox
            let sandbox = Path::new(r"C:\Users\bryan\Desktop\rust\sandbox");
            let sandbox_main = sandbox.join(r"src\main.rs");
            std::fs::write(&sandbox_main, code).unwrap_or_else(|e| {
                panic!(
                    "Error {}, Unable to write file to path {:?}",
                    e, &sandbox_main
                )
            });

            // Compiles and runs `rust_program`
            // Assumes `cargo` is in the `PATH`
            let output = Command::new("cargo")
                .current_dir(sandbox)
                .arg("run")
                .arg("--release")
                .output()
                .expect("Failed to run `cargo run`");

            if output.status.success() {
                log::info!("Compilation successful");
            } else {
                // Output stderr not guaranteed to be valid UTF-8
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                log::error!("Compilation failed: Error {stderr}");
            }

            // Always returns output regardless if compilation run succeeded or failed.
            // The output will have the stdout/stderr in either case.
            output
        }
    })
    .await;

    let (code, runner_output) = match join_handle_result {
        Ok(compile_output) => {
            let runner_output = RunnerOutput::new(
                String::from_utf8_lossy(&compile_output.stdout).to_string(),
                String::from_utf8_lossy(&compile_output.stderr).to_string(),
                compile_output.status.code().unwrap_or(-1),
            );
            // Update shared state for late joiners
            let mut state = state.lock().await;
            state.last_code = CodeContainer { code: code.clone() };

            (warp::http::StatusCode::OK, runner_output)
        }
        Err(_) => {
            let error_output = RunnerOutput::new(
                String::new(),
                String::from("An unexpected error occurred thread while compiling."),
                1,
            );
            (warp::http::StatusCode::INTERNAL_SERVER_ERROR, error_output)
        }
    };

    // Returns different statuses per compilation result
    // Previously was using `.map().with().with()` but these types in the branches would not match. Instead, return one type
    // and change the constructor args based on code.
    let reply = warp::reply::json(&runner_output);
    let reply = warp::reply::with_header(reply, "Access-Control-Allow-Origin", "*");

    warp::reply::with_status(reply, code)
}

pub(crate) fn compile_code_route(
    state: SharedAppState,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("compile")
        .and(warp::post())
        .and(warp::body::json())
        // Implicitly uses `serde_json` to convert bytes to the mapped type (`CodeContainer`)
        .then(move |code: CodeContainer| {
            println!("Received code: {code:?}");
            // Compile the code
            compile_code(code.code, state.clone())
        })
}
