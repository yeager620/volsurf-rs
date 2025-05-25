use yew::prelude::*;
use yew_plotly::Plotly;
use plotly::Plot;
use options_rs::models::volatility::VolatilitySurface;
use options_rs::webapp::surface_to_plot;
use ndarray::Array2;
use chrono::Utc;

#[function_component(SurfacePlot)]
fn surface_plot() -> Html {
    let surface = VolatilitySurface {
        symbol: "DEMO".into(),
        expirations: vec![],
        strikes: vec![],
        volatilities: Array2::zeros((2, 2)),
        timestamp: Utc::now(),
        version: 1,
    };

    let mut plot = surface_to_plot(&surface);

    html! {
        <div>
            <h1>{ format!("Volatility Surface - {}", surface.symbol) }</h1>
            <Plotly plot={plot} />
        </div>
    }
}

#[function_component(App)]
fn app() -> Html {
    html! {
        <SurfacePlot />
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
