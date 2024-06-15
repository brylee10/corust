#[cfg(feature = "js")]
use wasm_bindgen::prelude::*;

#[cfg(all(feature = "metrics", feature = "js"))]
pub fn now() -> f64 {
    web_sys::window()
        .expect("should have a Window")
        .performance()
        .expect("should have a Performance")
        .now()
}

#[cfg(feature = "js")]
// JS logging
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    pub fn error(s: &str);
    #[wasm_bindgen(js_namespace = console)]
    pub fn log(s: &str);
    #[wasm_bindgen(js_namespace = console)]
    pub fn debug(s: &str);
}
