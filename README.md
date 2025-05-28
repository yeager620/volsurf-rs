# Options-RS Workspace

This project now contains a multi-crate workspace with a shared core library and two front-ends:

- **options_core** – business logic and API clients
- **viz_native** – existing egui desktop application
- **viz_web** – Leptos based web application

API keys are loaded on the server side from `.env` using `dotenvy`. The web front-end calls server functions so that secrets never reach the browser.
