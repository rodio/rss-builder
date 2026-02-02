use clap::Parser;

use crate::app::{App, Cli};

mod app;

fn main() -> Result<(), anyhow::Error> {
    env_logger::init();
    let cli = Cli::parse();
    let app = App::new(cli)?;
    app.run()
}
