use leptos::*;
use options_core::{fetch_chain, build_surfaces};

#[component]
pub fn App() -> impl IntoView {
    let ticker = create_signal("AAPL".to_string());
    let status = create_rw_signal(String::new());

    let plot = create_action(|ticker: &String| async move {
        status.set("loading…".into());
        match fetch_chain(ticker).await
            .and_then(|c| build_surfaces(&c, 0.045)) {
            Ok((_call, _put)) => {
                status.set("✓ ready".into());
            }
            Err(e) => status.set(format!("{e}")),
        }
    });

    view! {
        <main class="p-4 max-w-xl mx-auto">
            <h1 class="text-2xl mb-2">"Volatility Surface"</h1>

            <input class="border px-2 py-1"
                   prop:value=ticker
                   on:input=move |ev| ticker.set(event_target_value(&ev)) />

            <button class="ml-2 px-3 py-1 rounded bg-slate-600 text-white"
                    on:click=move |_| plot.dispatch(ticker.get())>
                "Plot"
            </button>

            <p class="mt-2 text-sm text-gray-600">{status}</p>
        </main>
    }
}
