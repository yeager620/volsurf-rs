
## Project Tree
```
├── Cargo.lock
├── Cargo.toml
├── README.md
└── src
    ├── api
    │   ├── alpaca.md
    │   ├── mod.rs
    │   ├── rest.rs
    │   └── websocket.rs
    ├── bin
    │   ├── live_volsurf_plot.rs
    │   └── test_websocket.rs
    ├── config.rs
    ├── error.rs
    ├── lib.rs
    ├── models
    │   ├── mod.rs
    │   ├── option.rs
    │   └── volatility.rs
    └── utils
        ├── black_scholes.rs
        ├── mod.rs
        └── plotting.rs
```

### Running the Yew Plotly GUI

Install `trunk` and run the following command to start the web interface:

```bash
trunk serve --open --release -- bin/web_gui.rs
```

This will open a browser displaying interactive volatility surfaces rendered with Plotly.
