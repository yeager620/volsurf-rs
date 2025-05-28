pub mod app;

use app::App;
use wasm_bindgen::prelude::*;
use leptos::*;

#[wasm_bindgen(start)]
pub fn main() {
    mount_to_body(App);
}
