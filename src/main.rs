#![windows_subsystem = "windows"]

mod app;
mod render;

fn main() -> anyhow::Result<()> {
    app::run()
}
