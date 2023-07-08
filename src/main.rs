use botm_web::{Botm, Configuration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    simple_logger::init_with_env()?;
    // let subscriber = get_subscriber("BOTM".into(), "info".into(), std::io::stdout);
    // init_subscriber(subscriber);

    let configuration = Configuration::new().expect("Failed to load configuration");

    let botm = Botm::build(configuration)
        .await
        .expect("Failed to build Botm app");

    botm.run_until_stopped().await?;

    Ok(())
}
