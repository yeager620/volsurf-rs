//! server entry point for SSR
#![cfg(feature = "ssr")]

use axum::Router;
use dotenvy::dotenv;
use leptos::*;
use leptos_axum::{generate_route_list, LeptosRoutes};
use tracing_subscriber::FmtSubscriber;

pub fn get_leptos_options() -> LeptosOptions {
    LeptosOptions::builder()
        .site_addr(([127, 0, 0, 1], 8080))
        .output_name("viz_web")
        .site_root("static")
        .build()
        .unwrap()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let fmt_layer = FmtSubscriber::builder().with_max_level(tracing::Level::INFO).finish();
    tracing::subscriber::set_global_default(fmt_layer)?;

    let leptos_options = get_leptos_options();
    let routes = generate_route_list(|| view! { <crate::app::App/> });

    let app = Router::new().merge(LeptosRoutes::new(leptos_options.clone(), routes));
    let addr = leptos_options.site_addr;
    tracing::info!("listening on http://{addr}");
    axum::Server::bind(&addr).serve(app.into_make_service()).await?;
    Ok(())
}
