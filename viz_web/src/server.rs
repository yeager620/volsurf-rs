#[cfg(feature = "ssr")]
use leptos::*;
#[cfg(feature = "ssr")]
use dotenvy::dotenv;
#[cfg(feature = "ssr")]
use leptos_axum::{generate_route_list, LeptosRoutes};
#[cfg(feature = "ssr")]
use axum::Router;

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let options = LeptosOptions::builder().site_root("./").output_name("viz_web").site_addr(([127,0,0,1], 3000)).build()?;
    let routes = generate_route_list(|cx| view! { cx, <crate::app::App/> });
    let app = Router::new().merge(LeptosRoutes::new(options.clone(), routes));
    axum::Server::bind(&options.site_addr).serve(app.into_make_service()).await?;
    Ok(())
}
