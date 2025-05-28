use leptos::*;

#[component]
pub fn App() -> impl IntoView {
    let ticker = create_signal("AAPL".to_string());
    let status = create_rw_signal(String::new());

    let plot = create_action(|ticker: &String| async move {
        status.set(format!("loading {}...", ticker));
        // TODO call server function
    });

    view! {
        <main class="p-4 max-w-xl mx-auto">
            <h1 class="text-2xl mb-2">"Volatility Surface"</h1>
            <input class="border px-2 py-1" prop:value=ticker on:input=move |ev| ticker.set(event_target_value(&ev)) />
            <button class="ml-2 px-3 py-1 rounded bg-slate-600 text-white" on:click=move |_| plot.dispatch(ticker.get())>
                "Plot"
            </button>
            <p class="mt-2 text-sm text-gray-600">{status}</p>
        </main>
    }
}
